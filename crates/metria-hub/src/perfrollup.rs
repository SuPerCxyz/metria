//! 小时级性能预聚合：把 `model_calls` 的延迟/速度/热力图指标按 UTC 小时固化，
//! 让页面刷新不必每次把十几万行明细拉进应用层。
//!
//! 设计要点：
//! * **一份累加逻辑两种来源**：[`PerfHour::observe`] 既用于构建预聚合，也用于明细回退
//!   路径，保证「预聚合结果」与「明细结果」来自同一段代码，不会出现口径漂移。
//! * **分位数用固定对数边界直方图合并**：分位数无法从桶均值还原，因此按桶存 256 个
//!   对数分箱，合并后取分箱几何中心，误差约 ±3.2%；响应必须标注为近似值。
//! * **平均值与样本数保持精确**：`sum` 与 `count` 直接累加，不经过分箱。
//! * 覆盖不到的时间桶（当前未完成小时、预聚合缺失）由明细回退补齐，绝不填 0。

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use metria_storage::rusqlite::{params_from_iter, types::Value as SqlValue};
use metria_storage::StorageError;
use serde::{Deserialize, Serialize};

use crate::db::HubDb;

/// 直方图分箱数。256 箱覆盖 7 个数量级，每十进制约 36 箱，分箱跨度约 6.5%，
/// 取几何中心后分位数误差约 ±3.2%。
pub const HIST_BINS: usize = 256;
/// 毫秒类指标的上界（约 2.8 小时），超出的样本被夹到最后一箱并计入饱和计数。
const MS_HIST_MAX: f64 = 10_000_000.0;
/// 输出速度上界（token/s）。
const SPEED_HIST_MAX: f64 = 100_000.0;
/// payload 结构版本；变更字段语义时递增，旧版本行会被重建覆盖。
const PAYLOAD_VERSION: u32 = 1;

/// 单个分箱的样本数。
pub type BinCounts = [u32; HIST_BINS];

/// 样本值落到第几箱（对数等比边界）。
fn bin_index(value: f64, max: f64) -> usize {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let span = (max + 1.0).ln();
    let idx = (HIST_BINS as f64 * (value + 1.0).ln() / span).floor();
    (idx as usize).min(HIST_BINS - 1)
}

/// 该分箱代表值：区间内 (lo+1)*(hi+1) 的几何中心减一，保证等比区间内误差对称。
fn bin_representative(idx: usize, max: f64) -> f64 {
    let span = (max + 1.0).ln();
    let lo = (span * idx as f64 / HIST_BINS as f64).exp() - 1.0;
    let hi = (span * (idx + 1) as f64 / HIST_BINS as f64).exp() - 1.0;
    ((lo + 1.0) * (hi + 1.0)).sqrt() - 1.0
}

/// 整数指标：样本数、精确总和、分位数分箱。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntMetric {
    pub count: u64,
    pub sum: i64,
    pub max: i64,
    #[serde(default)]
    pub hist: Vec<u32>,
}

impl IntMetric {
    fn push(&mut self, value: i64) {
        if self.hist.is_empty() {
            self.hist = vec![0; HIST_BINS];
        }
        let idx = bin_index(value as f64, MS_HIST_MAX);
        self.hist[idx] = self.hist[idx].saturating_add(1);
        self.count += 1;
        self.sum = self.sum.saturating_add(value);
        self.max = self.max.max(value);
    }

    fn merge(&mut self, other: &IntMetric) {
        if other.count == 0 {
            return;
        }
        if self.hist.is_empty() {
            self.hist = other.hist.clone();
        } else if !other.hist.is_empty() {
            for (dst, src) in self.hist.iter_mut().zip(other.hist.iter()) {
                *dst = dst.saturating_add(*src);
            }
        }
        self.count += other.count;
        self.sum = self.sum.saturating_add(other.sum);
        self.max = self.max.max(other.max);
    }

    /// 精确平均值：与既有明细路径一致地做整数除法。
    pub fn avg(&self) -> Option<i64> {
        (self.count > 0).then(|| self.sum / self.count as i64)
    }

    /// 分位数（近似）：最近秩落在哪个分箱，就取该分箱的几何中心。
    pub fn quantile(&self, q: f64) -> Option<i64> {
        histogram_quantile(&self.hist, q, MS_HIST_MAX).map(|v| v.round() as i64)
    }
}

/// 浮点指标（输出速度 token/s），总和用 f64 精确累加，不经过分箱。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FloatMetric {
    pub count: u64,
    pub sum: f64,
    pub max: f64,
    #[serde(default)]
    pub hist: Vec<u32>,
}

impl FloatMetric {
    fn push(&mut self, value: f64) {
        if self.hist.is_empty() {
            self.hist = vec![0; HIST_BINS];
        }
        let idx = bin_index(value, SPEED_HIST_MAX);
        self.hist[idx] = self.hist[idx].saturating_add(1);
        self.count += 1;
        self.sum += value;
        self.max = self.max.max(value);
    }

    fn merge(&mut self, other: &FloatMetric) {
        if other.count == 0 {
            return;
        }
        if self.hist.is_empty() {
            self.hist = other.hist.clone();
        } else if !other.hist.is_empty() {
            for (dst, src) in self.hist.iter_mut().zip(other.hist.iter()) {
                *dst = dst.saturating_add(*src);
            }
        }
        self.count += other.count;
        self.sum += other.sum;
        self.max = self.max.max(other.max);
    }

    pub fn avg(&self) -> Option<f64> {
        (self.count > 0).then(|| self.sum / self.count as f64)
    }

    pub fn quantile(&self, q: f64) -> Option<f64> {
        histogram_quantile(&self.hist, q, SPEED_HIST_MAX)
    }
}

/// 合并后的分位数：返回最近秩所在分箱的几何中心；无样本返回 `None`（诚实 null）。
fn histogram_quantile(hist: &[u32], q: f64, max: f64) -> Option<f64> {
    let total: u64 = hist.iter().map(|&c| c as u64).sum();
    if total == 0 {
        return None;
    }
    let target = ((total as f64 * q).ceil() as u64).max(1);
    let mut cumulative = 0u64;
    for (idx, &count) in hist.iter().enumerate() {
        cumulative += count as u64;
        if cumulative >= target {
            return Some(bin_representative(idx, max));
        }
    }
    Some(bin_representative(HIST_BINS - 1, max))
}

/// 观测来源分布（运行时观测 / 日志推导 / 未知）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceCount {
    pub source: String,
    pub quality: String,
    pub count: i64,
}

/// 来源分布集合：按 (source, quality) 合并计数。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceCounts(#[serde(default)] pub Vec<SourceCount>);

impl SourceCounts {
    fn bump(&mut self, source: &str, quality: &str) {
        if let Some(entry) = self
            .0
            .iter_mut()
            .find(|e| e.source == source && e.quality == quality)
        {
            entry.count += 1;
        } else {
            self.0.push(SourceCount {
                source: source.to_string(),
                quality: quality.to_string(),
                count: 1,
            });
        }
    }

    fn merge(&mut self, other: &SourceCounts) {
        for entry in &other.0 {
            if let Some(found) = self
                .0
                .iter_mut()
                .find(|e| e.source == entry.source && e.quality == entry.quality)
            {
                found.count += entry.count;
            } else {
                self.0.push(entry.clone());
            }
        }
    }

    /// 按计数倒序输出（与既有响应顺序一致）。
    pub fn into_sorted_rows(self) -> Vec<serde_json::Value> {
        let mut rows: Vec<serde_json::Value> = self
            .0
            .into_iter()
            .map(|e| serde_json::json!({ "source": e.source, "quality": e.quality, "count": e.count }))
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row["count"].as_i64().unwrap_or(0)));
        rows
    }
}

/// 热力图所需的 Token / 费用聚合（明细回退与预聚合共用）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeatmapAgg {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub reported_cost: i64,
    pub calculated_cost: i64,
    pub estimated_cost: i64,
    /// 参与热力图的调用数（仅统计 `started_at` 可解析的行）。
    pub model_calls: u64,
    pub duration_ms: i64,
    /// 明确记录了 `duration_ms` 的调用数：没有它无法区分「时长为 0」与「无时长」，
    /// 而明细路径对后者返回 null，不能伪造 0。
    #[serde(default)]
    pub duration_count: u64,
    /// 该小时最新的调用起点，供热力图提示展示区间。
    pub latest_started_at: Option<String>,
}

impl HeatmapAgg {
    pub(crate) fn merge(&mut self, other: &HeatmapAgg) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.reported_cost += other.reported_cost;
        self.calculated_cost += other.calculated_cost;
        self.estimated_cost += other.estimated_cost;
        self.model_calls += other.model_calls;
        self.duration_ms += other.duration_ms;
        self.duration_count += other.duration_count;
        self.latest_started_at = match (&self.latest_started_at, &other.latest_started_at) {
            (Some(a), Some(b)) => Some(if a > b { a.clone() } else { b.clone() }),
            (Some(a), None) => Some(a.clone()),
            (None, b) => b.clone(),
        };
    }
}

/// 单个 UTC 小时的完整性能聚合结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerfHour {
    /// payload 版本，旧版本行在重建时被覆盖。
    #[serde(default)]
    pub v: u32,
    /// 范围内全部调用数（与 `/usage/performance` 的 `total_calls` 同口径）。
    pub call_count: u64,
    pub success: u64,
    pub errors: u64,
    pub duration: IntMetric,
    pub first_byte: IntMetric,
    pub ttft: IntMetric,
    pub generation: IntMetric,
    pub speed: FloatMetric,
    pub inter_token: IntMetric,
    pub stalls: IntMetric,
    /// 观测到的请求/响应 payload 与 wire 字节数：[(bytes, count); 4]。
    #[serde(default)]
    pub observed_bytes: Vec<(i64, u64)>,
    /// 范围内出现过的 Agent（含空串，与 `COUNT(DISTINCT client_id)` 同口径）。
    #[serde(default)]
    pub distinct_clients: Vec<String>,
    /// 出现过的模型（排除空串，与 `/overview` 的 models 计数同口径）。
    #[serde(default)]
    pub distinct_models: Vec<String>,
    /// 出现过的项目（排除空串，与 `/overview` 的 projects 计数同口径）。
    #[serde(default)]
    pub distinct_projects: Vec<String>,
    /// 至少带一个 Token 字段的调用数（与 `/overview` 的 token_calls 同口径）。
    #[serde(default)]
    pub token_call_count: u64,
    /// 同时带 input/output Token 的调用数（与 `/overview` 的计价分母同口径）。
    #[serde(default)]
    pub token_pair_call_count: u64,
    /// 上述调用中已带任一费用口径的条数（与 `/overview` 的 priced_calls 同口径）。
    #[serde(default)]
    pub priced_call_count: u64,
    /// 未计价调用按 (model, provider) 的分组计数，供 `/overview` 的 unpriced_models。
    ///
    /// 保留原始 Option 以便按 SQL 的 `GROUP BY model_normalized, provider_normalized`
    /// 分组，渲染时再做 COALESCE，避免把 NULL 与字面量 '(unknown)' 合并成同一组。
    #[serde(default)]
    pub unpriced_by_model: Vec<(Option<String>, Option<String>, u64)>,
    pub sources: SourceCounts,
    pub first_byte_sources: SourceCounts,
    pub ttft_sources: SourceCounts,
    pub generation_sources: SourceCounts,
    pub speed_sources: SourceCounts,
    pub inter_token_sources: SourceCounts,
    pub heatmap: HeatmapAgg,
}

impl Default for PerfHour {
    /// 必须带版本号：derive 出来的 `v` 是 0，会被 [`PerfHour::from_payload`] 判为
    /// 版本过期，预聚合就会永远回退明细、快速路径形同虚设。
    fn default() -> Self {
        Self {
            v: PAYLOAD_VERSION,
            observed_bytes: vec![(0, 0); 4],
            call_count: 0,
            success: 0,
            errors: 0,
            duration: IntMetric::default(),
            first_byte: IntMetric::default(),
            ttft: IntMetric::default(),
            generation: IntMetric::default(),
            speed: FloatMetric::default(),
            inter_token: IntMetric::default(),
            stalls: IntMetric::default(),
            sources: SourceCounts::default(),
            first_byte_sources: SourceCounts::default(),
            ttft_sources: SourceCounts::default(),
            generation_sources: SourceCounts::default(),
            speed_sources: SourceCounts::default(),
            inter_token_sources: SourceCounts::default(),
            heatmap: HeatmapAgg::default(),
            distinct_clients: Vec::new(),
            distinct_models: Vec::new(),
            distinct_projects: Vec::new(),
            token_call_count: 0,
            token_pair_call_count: 0,
            priced_call_count: 0,
            unpriced_by_model: Vec::new(),
        }
    }
}

impl PerfHour {
    pub fn new() -> Self {
        Self::default()
    }

    /// 消费一条 `model_calls` 明细；与 `/usage/performance` 的逐行逻辑完全一致。
    ///
    /// `total` 由调用方在解析前累加（保持「解析失败也计入 total_calls」的既有口径）。
    pub fn observe_call(&mut self, row: &CallRow) {
        self.call_count += 1;
        // DISTINCT 与 Token 计数必须在解析 started_at 之前统计：对应的 SQL 不关心
        // 时间戳能否解析，先统计才能与 /overview 既有的 COUNT(DISTINCT) 结果一致。
        self.distinct_clients.push(row.client_id.clone());
        if let Some(model) = row.model_normalized.as_deref().filter(|v| !v.is_empty()) {
            self.distinct_models.push(model.to_string());
        }
        if let Some(project) = row.project_id.as_deref().filter(|v| !v.is_empty()) {
            self.distinct_projects.push(project.to_string());
        }
        if row.has_tokens {
            self.token_call_count += 1;
        }
        // 计价分母与未计价分组：与 /overview 的两个原始查询同口径
        if row.has_token_pair {
            self.token_pair_call_count += 1;
            if row.has_any_cost {
                self.priced_call_count += 1;
            } else {
                let model = row.model_normalized.clone();
                let provider = row.provider_normalized.clone();
                match self
                    .unpriced_by_model
                    .iter_mut()
                    .find(|(m, p, _)| *m == model && *p == provider)
                {
                    Some(entry) => entry.2 += 1,
                    None => self.unpriced_by_model.push((model, provider, 1)),
                }
            }
        }
        let started = match parse_ts(&row.started_at) {
            Some(ts) => ts,
            None => return,
        };
        if is_success(&row.status) {
            self.success += 1;
        } else {
            self.errors += 1;
        }
        for (slot, value) in self.observed_bytes.iter_mut().zip(row.observed_bytes()) {
            if let Some(value) = value.filter(|value| *value >= 0) {
                slot.0 += value;
                slot.1 += 1;
            }
        }
        if let Some(value) = row.duration_ms {
            self.duration.push(value);
        }
        let byte_at = row.first_byte_at.as_deref().and_then(parse_ts);
        let token_at = row.first_token_at.as_deref().and_then(parse_ts);
        let legacy_first = row.first_response_at.as_deref().and_then(parse_ts);
        let source = row
            .observability_source
            .clone()
            .or_else(|| row.timing_source.clone())
            .unwrap_or_else(|| "unknown".into());
        let quality = row
            .observability_quality
            .clone()
            .or_else(|| row.timing_quality.clone())
            .unwrap_or_else(|| "unknown".into());
        if let Some(byte_at) = byte_at.filter(|at| *at >= started) {
            self.first_byte.push((byte_at - started).num_milliseconds());
            self.first_byte_sources.bump(&source, &quality);
        }
        let first = token_at.or(legacy_first);
        if let Some(first) = first.filter(|first| *first >= started) {
            self.ttft.push((first - started).num_milliseconds());
            self.sources.bump(&source, &quality);
            self.ttft_sources.bump(&source, &quality);
            // 生成耗时与输出速度只信任运行时观测字段；日志条目完成时间无法测量生成区间。
            let generation_ms = row.generation_duration_ms.filter(|value| *value > 0);
            if let Some(value) = generation_ms {
                self.generation.push(value);
                self.generation_sources.bump(&source, &quality);
            }
            if let Some(value) = row.output_tokens_per_second_milli.filter(|v| *v > 0) {
                self.speed.push(value as f64 / 1000.0);
                self.speed_sources.bump(&source, &quality);
            } else if let (Some(generation_ms), Some(tokens)) =
                (generation_ms, row.output_tokens.filter(|v| *v > 0))
            {
                self.speed
                    .push(tokens as f64 * 1000.0 / generation_ms as f64);
                self.speed_sources.bump(&source, &quality);
            }
        }
        if let Some(value) = row.inter_token_latency_avg_ms.filter(|v| *v >= 0) {
            self.inter_token.push(value);
            self.inter_token_sources.bump(&source, &quality);
        }
        if let Some(value) = row.inter_token_latency_p95_ms.filter(|v| *v >= 0) {
            self.inter_token.push(value);
            self.inter_token_sources.bump(&source, &quality);
        }
        if let Some(value) = row.stall_count.filter(|v| *v >= 0) {
            self.stalls.push(value);
        }
        if row.rate_limited == Some(true) {
            self.sources.bump("rate_limited", "observed");
        }
    }

    /// 合并另一个小时（或明细回退结果）。
    pub fn merge(&mut self, other: &PerfHour) {
        self.call_count += other.call_count;
        self.success += other.success;
        self.token_call_count += other.token_call_count;
        self.token_pair_call_count += other.token_pair_call_count;
        self.priced_call_count += other.priced_call_count;
        for (model, provider, count) in &other.unpriced_by_model {
            match self
                .unpriced_by_model
                .iter_mut()
                .find(|(m, p, _)| m == model && p == provider)
            {
                Some(entry) => entry.2 += count,
                None => self
                    .unpriced_by_model
                    .push((model.clone(), provider.clone(), *count)),
            }
        }
        self.distinct_clients
            .extend(other.distinct_clients.iter().cloned());
        self.distinct_models
            .extend(other.distinct_models.iter().cloned());
        self.distinct_projects
            .extend(other.distinct_projects.iter().cloned());
        // 逐小时合并会产生重复项，就地去重保证与 COUNT(DISTINCT) 等价且内存有界
        self.distinct_clients.sort_unstable();
        self.distinct_clients.dedup();
        self.distinct_models.sort_unstable();
        self.distinct_models.dedup();
        self.distinct_projects.sort_unstable();
        self.distinct_projects.dedup();
        self.errors += other.errors;
        self.duration.merge(&other.duration);
        self.first_byte.merge(&other.first_byte);
        self.ttft.merge(&other.ttft);
        self.generation.merge(&other.generation);
        self.speed.merge(&other.speed);
        self.inter_token.merge(&other.inter_token);
        self.stalls.merge(&other.stalls);
        if self.observed_bytes.len() < 4 {
            self.observed_bytes = vec![(0, 0); 4];
        }
        for (slot, src) in self
            .observed_bytes
            .iter_mut()
            .zip(other.observed_bytes.iter())
        {
            slot.0 += src.0;
            slot.1 += src.1;
        }
        self.sources.merge(&other.sources);
        self.first_byte_sources.merge(&other.first_byte_sources);
        self.ttft_sources.merge(&other.ttft_sources);
        self.generation_sources.merge(&other.generation_sources);
        self.speed_sources.merge(&other.speed_sources);
        self.inter_token_sources.merge(&other.inter_token_sources);
        self.heatmap.merge(&other.heatmap);
    }

    /// 去重 DISTINCT 集合。
    ///
    /// `merge` 路径会顺带去重，但明细回退路径（`scan_calls`）从不 merge，
    /// 直接按行累加会把同值重复 push，`len()` 就不再是 COUNT(DISTINCT)。
    pub fn finalize(&mut self) {
        self.distinct_clients.sort_unstable();
        self.distinct_clients.dedup();
        self.distinct_models.sort_unstable();
        self.distinct_models.dedup();
        self.distinct_projects.sort_unstable();
        self.distinct_projects.dedup();
    }

    /// 序列化为 payload（版本号随写入更新）。
    pub fn to_payload(&self) -> Result<String, StorageError> {
        let mut stamped = self.clone();
        stamped.v = PAYLOAD_VERSION;
        stamped.finalize();
        serde_json::to_string(&stamped).map_err(|e| StorageError::Query(e.to_string()))
    }

    /// 反序列化；版本不符视为不可用，由调用方回退明细。
    pub fn from_payload(payload: &str) -> Option<Self> {
        let value: PerfHour = serde_json::from_str(payload).ok()?;
        (value.v == PAYLOAD_VERSION).then_some(value)
    }
}

/// `model_calls` 中与性能相关的原始字段。
#[derive(Debug, Clone, Default)]
pub struct CallRow {
    pub started_at: String,
    pub duration_ms: Option<i64>,
    pub first_byte_at: Option<String>,
    pub first_token_at: Option<String>,
    pub first_response_at: Option<String>,
    pub output_tokens: Option<i64>,
    pub output_tokens_per_second_milli: Option<i64>,
    pub generation_duration_ms: Option<i64>,
    pub inter_token_latency_avg_ms: Option<i64>,
    pub inter_token_latency_p95_ms: Option<i64>,
    pub stall_count: Option<i64>,
    pub observability_source: Option<String>,
    pub observability_quality: Option<String>,
    pub timing_source: Option<String>,
    pub timing_quality: Option<String>,
    pub observed_bytes: [Option<i64>; 4],
    pub status: String,
    pub rate_limited: Option<bool>,
    pub client_id: String,
    pub model_normalized: Option<String>,
    pub project_id: Option<String>,
    pub provider_normalized: Option<String>,
    /// input/output Token 至少一个非空（计价分母）。
    pub has_token_pair: bool,
    /// 三种费用口径至少一个非空（已计价）。
    pub has_any_cost: bool,
    /// 是否至少有一个非空 Token 字段（用于 `/overview` 的 token_calls）。
    pub has_tokens: bool,
}

impl CallRow {
    fn observed_bytes(&self) -> [Option<i64>; 4] {
        self.observed_bytes
    }
}

fn parse_ts(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|ts| ts.with_timezone(&Utc))
}

fn is_success(status: &str) -> bool {
    status == "success" || status == "ok" || status == "completed"
}

/// UTC 整点小时串（`YYYY-MM-DDTHH:00:00+00:00`），与 rollup 分桶格式一致。
pub fn hour_bucket(ts: DateTime<Utc>) -> String {
    Utc.with_ymd_and_hms(ts.year(), ts.month(), ts.day(), ts.hour(), 0, 0)
        .single()
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| ts.to_rfc3339())
}

/// 读取预聚合：按小时区间返回 payload 行。
///
/// 返回 `None` 表示区间内存在缺失或版本过期的小时，调用方应回退明细，
/// 绝不能把缺失当成 0。
pub fn load_range(db: &HubDb, from: DateTime<Utc>, to: DateTime<Utc>) -> Option<Vec<PerfHour>> {
    if to <= from {
        return Some(Vec::new());
    }
    let c = db.conn();
    let mut stmt = c
        .prepare("SELECT bucket, payload FROM hourly_performance_rollups WHERE bucket >= ?1 AND bucket < ?2 ORDER BY bucket")
        .ok()?;
    let rows = stmt
        .query_map([from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .ok()?;
    let mut out = Vec::new();
    for row in rows.flatten() {
        match PerfHour::from_payload(&row.1) {
            Some(value) => out.push(value),
            None => return None,
        }
    }
    Some(out)
}

/// 覆盖历史区间的整点小时序列（不含当前未完成小时）。
pub fn complete_hours(from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let floor = |ts: DateTime<Utc>| {
        Utc.with_ymd_and_hms(ts.year(), ts.month(), ts.day(), ts.hour(), 0, 0)
            .single()
            .unwrap_or(ts)
    };
    let mut cursor = floor(from);
    if cursor < from {
        cursor += chrono::Duration::hours(1);
    }
    let end = floor(to);
    let mut out = Vec::new();
    while cursor < end {
        out.push(cursor);
        cursor += chrono::Duration::hours(1);
    }
    out
}

/// 绑定 `?1/?2` 为时间范围、其后为筛选参数（与 handlers 的 `range_args` 顺序一致）。
fn bind_range(from: DateTime<Utc>, to: DateTime<Utc>, extra_args: &[SqlValue]) -> Vec<SqlValue> {
    let mut args = Vec::with_capacity(extra_args.len() + 2);
    args.push(SqlValue::Text(from.to_rfc3339()));
    args.push(SqlValue::Text(to.to_rfc3339()));
    args.extend_from_slice(extra_args);
    args
}

/// 按 UTC 小时返回 `[from, to)` 的聚合：完整小时读预聚合，两端不整点片段回退明细。
///
/// 三段互不重叠且并集恰为 `[from, to)`：
///   * S1 `[from, mid_start)`：头部不整点片段与「查询早于预聚合起点」的空白区间，
///     一律走明细扫描（空白区间没有行，索引区间 seek 即返回，代价可忽略）；
///   * S2 `[mid_start, mid_end)`：完整小时读预聚合，**缺行即整体回退**，绝不把缺失当 0；
///   * S3 `[mid_end, to)`：当前未完成小时，走明细扫描，因此**新数据立即可见**。
///
/// 返回 `None` 表示本次必须整体回退明细（带维度筛选、或预聚合缺桶/版本过期）。
pub fn fast_hours(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    filter: &str,
) -> Option<Vec<(DateTime<Utc>, PerfHour)>> {
    if !filter.trim().is_empty() {
        return None;
    }
    if to <= from {
        return Some(Vec::new());
    }
    let first_bucket = min_bucket(db)?;
    let head_end = ceil_hour(from);
    let mid_start = head_end.max(first_bucket);
    let mid_end = floor_hour(to).min(floor_hour(Utc::now()));

    // 没有可用的完整小时：只有整个区间确实落在**同一个** UTC 小时时，才能安全地把结果
    // 压进单个桶——桶 key 会被热力图与日趋势用来定格。跨小时的短区间（如 10:30→11:30，
    // 此时 ceil(from)==floor(to)）若压成一个桶，11:00 之后的行会被算进 10:00 那一格，
    // 跨日界时更会记到前一天；这时返回 None，让调用方按小时分组扫明细。
    if mid_start >= mid_end {
        if floor_hour(from) != floor_hour(to - chrono::Duration::nanoseconds(1)) {
            return None;
        }
        return Some(vec![(
            floor_hour(from),
            scan_calls(db, from, to, "", &[]).ok()?,
        )]);
    }

    let mut out: Vec<(DateTime<Utc>, PerfHour)> = Vec::new();
    if from < mid_start {
        out.push((
            floor_hour(from),
            scan_calls(db, from, mid_start, "", &[]).ok()?,
        ));
    }

    let hours = (mid_end.timestamp() - mid_start.timestamp()) / 3600;
    if hours > 0 {
        let present = count_buckets(db, mid_start, mid_end)?;
        if present != hours {
            return None;
        }
        let rows = load_range(db, mid_start, mid_end)?;
        if rows.len() as i64 != hours {
            return None;
        }
        let mut bucket = mid_start;
        for row in rows {
            out.push((bucket, row));
            bucket += chrono::Duration::hours(1);
        }
    }

    if mid_end < to {
        out.push((
            floor_hour(mid_end),
            scan_calls(db, mid_end, to, "", &[]).ok()?,
        ));
    }
    Some(out)
}

/// 把时间夹到所在 UTC 整点（供 rollup CTE 计算完整小时边界）。
///
/// 预聚合中最早的一个小时；表为空时返回 `None`（尚未构建，回退明细）。
fn min_bucket(db: &HubDb) -> Option<DateTime<Utc>> {
    let c = db.conn();
    let raw: Option<String> = c
        .query_row(
            "SELECT MIN(bucket) FROM hourly_performance_rollups",
            [],
            |r| r.get(0),
        )
        .ok()?;
    raw.as_deref().and_then(parse_ts)
}

/// 把 [`fast_hours`] 的结果合并成整段聚合（延迟/性能/总览分位数用）。
pub fn fast_aggregate(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    filter: &str,
) -> Option<PerfHour> {
    let hours = fast_hours(db, from, to, filter)?;
    let mut merged = PerfHour::new();
    for (_, hour) in hours {
        merged.merge(&hour);
    }
    merged.finalize();
    Some(merged)
}

pub fn floor_hour(ts: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(ts.year(), ts.month(), ts.day(), ts.hour(), 0, 0)
        .single()
        .unwrap_or(ts)
}

pub fn ceil_hour(ts: DateTime<Utc>) -> DateTime<Utc> {
    let floored = floor_hour(ts);
    if floored == ts {
        ts
    } else {
        floored + chrono::Duration::hours(1)
    }
}

/// 区间内已存在的预聚合行数；缺行时返回 `None` 触发回退。
fn count_buckets(db: &HubDb, from: DateTime<Utc>, to: DateTime<Utc>) -> Option<i64> {
    let c = db.conn();
    c.query_row(
        "SELECT COUNT(*) FROM hourly_performance_rollups WHERE bucket >= ?1 AND bucket < ?2",
        [from.to_rfc3339(), to.to_rfc3339()],
        |r| r.get::<_, i64>(0),
    )
    .ok()
}

/// 重建全部历史预聚合（启动时与周期维护调用）。
///
/// 全量重建保证：Agent 尾部补采、历史回填、重新计价等改动都能被吸收，
/// 避免「预聚合与明细不一致」。按窗口分批推进，内存与总行数解耦。
pub fn rebuild_history(db: &HubDb) -> Result<usize, StorageError> {
    let Some(earliest) = earliest_call_time(db)? else {
        return Ok(0);
    };
    rebuild(db, earliest, Utc::now())
}

/// 最早的调用时间；没有调用数据时返回 `None`。
fn earliest_call_time(db: &HubDb) -> Result<Option<DateTime<Utc>>, StorageError> {
    let c = db.conn();
    let raw: Option<String> = c
        .query_row("SELECT MIN(started_at) FROM model_calls", [], |r| r.get(0))
        .map_err(StorageError::from)?;
    Ok(raw.as_deref().and_then(parse_ts))
}

/// 明细回退路径：扫描 `[from, to)` 的 `model_calls`，用与预聚合相同的逻辑聚合。
///
/// 带维度筛选、或预聚合缺失时走这里，保证结果口径与预聚合完全一致。
pub fn scan_calls(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    extra_filter: &str,
    extra_args: &[SqlValue],
) -> Result<PerfHour, StorageError> {
    let mut acc = PerfHour::new();
    {
        let c = db.conn();
        let sql = format!(
            "SELECT started_at, duration_ms, first_byte_at, first_token_at, first_response_at,
                    output_tokens, output_tokens_per_second_milli, generation_duration_ms,
                    inter_token_latency_avg_ms, inter_token_latency_p95_ms, stall_count,
                    observability_source, observability_quality, timing_source, timing_quality,
                    observed_request_payload_bytes, observed_response_payload_bytes,
                    observed_request_wire_bytes, observed_response_wire_bytes, status, rate_limited,
                    client_id, model_normalized, project_id,
                    CASE WHEN (input_tokens IS NOT NULL OR output_tokens IS NOT NULL
                           OR cache_read_tokens IS NOT NULL OR cache_write_tokens IS NOT NULL
                           OR reasoning_tokens IS NOT NULL) THEN 1 ELSE 0 END
                    , provider_normalized
                    , CASE WHEN (input_tokens IS NOT NULL OR output_tokens IS NOT NULL) THEN 1 ELSE 0 END
                    , CASE WHEN (reported_cost_micro_usd IS NOT NULL OR calculated_cost_micro_usd IS NOT NULL
                              OR estimated_cost_micro_usd IS NOT NULL) THEN 1 ELSE 0 END
             FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 {extra_filter}"
        );
        let mut stmt = c.prepare(&sql)?;
        let args = bind_range(from, to, extra_args);
        let mut rows = stmt.query(params_from_iter(&args))?;
        while let Some(row) = rows.next()? {
            acc.observe_call(&CallRow {
                started_at: row.get(0)?,
                duration_ms: row.get(1)?,
                first_byte_at: row.get(2)?,
                first_token_at: row.get(3)?,
                first_response_at: row.get(4)?,
                output_tokens: row.get(5)?,
                output_tokens_per_second_milli: row.get(6)?,
                generation_duration_ms: row.get(7)?,
                inter_token_latency_avg_ms: row.get(8)?,
                inter_token_latency_p95_ms: row.get(9)?,
                stall_count: row.get(10)?,
                observability_source: row.get(11)?,
                observability_quality: row.get(12)?,
                timing_source: row.get(13)?,
                timing_quality: row.get(14)?,
                observed_bytes: [row.get(15)?, row.get(16)?, row.get(17)?, row.get(18)?],
                status: row.get::<_, Option<String>>(19)?.unwrap_or_default(),
                rate_limited: row.get(20)?,
                client_id: row.get::<_, Option<String>>(21)?.unwrap_or_default(),
                model_normalized: row.get(22)?,
                project_id: row.get(23)?,
                has_tokens: row.get::<_, Option<i64>>(24)?.unwrap_or(0) != 0,
                provider_normalized: row.get(25)?,
                has_token_pair: row.get::<_, Option<i64>>(26)?.unwrap_or(0) != 0,
                has_any_cost: row.get::<_, Option<i64>>(27)?.unwrap_or(0) != 0,
            });
        }
    }
    for (_, cell) in scan_heatmap_grouped(db, from, to, extra_filter, extra_args)? {
        acc.heatmap.merge(&cell);
    }
    acc.finalize();
    Ok(acc)
}

/// 热力图所需的 Token / 费用聚合，按 UTC 小时分组返回。
///
/// 子查询结构与 `/usage/heatmap` 逐字段一致：Token 先取 `usage_events` 求和、
/// 费用先取 `model_calls` 自身值，保证与明细路径口径相同。
pub fn scan_heatmap_grouped(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    extra_filter: &str,
    extra_args: &[SqlValue],
) -> Result<BTreeMap<String, HeatmapAgg>, StorageError> {
    let mut out: BTreeMap<String, HeatmapAgg> = BTreeMap::new();
    let sub = |col: &str| {
        format!("(SELECT SUM(u.{col}) FROM usage_events u WHERE u.model_call_id = m.id)")
    };
    let c = db.conn();
    let sql = format!(
        "SELECT m.started_at, m.duration_ms,
                COALESCE({s_input}, m.input_tokens), COALESCE({s_output}, m.output_tokens),
                COALESCE({s_cache_read}, m.cache_read_tokens), COALESCE({s_cache_write}, m.cache_write_tokens),
                COALESCE({s_reasoning}, m.reasoning_tokens),
                COALESCE(m.reported_cost_micro_usd, {s_reported}),
                COALESCE(m.calculated_cost_micro_usd, {s_calculated}),
                COALESCE(m.estimated_cost_micro_usd, {s_estimated})
         FROM model_calls m
         WHERE m.started_at >= ?1 AND m.started_at < ?2 {extra_filter}",
        s_input = sub("input_tokens"),
        s_output = sub("output_tokens"),
        s_cache_read = sub("cache_read_tokens"),
        s_cache_write = sub("cache_write_tokens"),
        s_reasoning = sub("reasoning_tokens"),
        s_reported = sub("reported_cost_micro_usd"),
        s_calculated = sub("calculated_cost_micro_usd"),
        s_estimated = sub("estimated_cost_micro_usd"),
    );
    let mut stmt = c.prepare(&sql)?;
    let args = bind_range(from, to, extra_args);
    let mut rows = stmt.query(params_from_iter(&args))?;
    while let Some(row) = rows.next()? {
        let started: Option<String> = row.get(0)?;
        let Some(started) = started else { continue };
        let Some(ts) = parse_ts(&started) else {
            continue;
        };
        let cell = out.entry(hour_bucket(ts)).or_default();
        cell.model_calls += 1;
        if let Some(duration) = row.get::<_, Option<i64>>(1)? {
            cell.duration_ms += duration;
            cell.duration_count += 1;
        }
        cell.input_tokens += row.get::<_, Option<i64>>(2)?.unwrap_or(0);
        cell.output_tokens += row.get::<_, Option<i64>>(3)?.unwrap_or(0);
        cell.cache_read_tokens += row.get::<_, Option<i64>>(4)?.unwrap_or(0);
        cell.cache_write_tokens += row.get::<_, Option<i64>>(5)?.unwrap_or(0);
        cell.reasoning_tokens += row.get::<_, Option<i64>>(6)?.unwrap_or(0);
        cell.reported_cost += row.get::<_, Option<i64>>(7)?.unwrap_or(0);
        cell.calculated_cost += row.get::<_, Option<i64>>(8)?.unwrap_or(0);
        cell.estimated_cost += row.get::<_, Option<i64>>(9)?.unwrap_or(0);
        cell.latest_started_at = match (&cell.latest_started_at, &started) {
            (Some(cur), next) if cur >= next => Some(cur.clone()),
            (_, next) => Some(next.clone()),
        };
    }
    Ok(out)
}

/// 单个窗口（默认一天）内的聚合，写入前只在内存里保留该窗口的结果。
fn build_window(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<BTreeMap<String, PerfHour>, StorageError> {
    let mut buckets: BTreeMap<String, PerfHour> = BTreeMap::new();
    let fallback_bucket = hour_bucket(from);
    {
        let c = db.conn();
        let sql =
            "SELECT started_at, duration_ms, first_byte_at, first_token_at, first_response_at, \
                   output_tokens, output_tokens_per_second_milli, generation_duration_ms, \
                   inter_token_latency_avg_ms, inter_token_latency_p95_ms, stall_count, \
                   observability_source, observability_quality, timing_source, timing_quality, \
                   observed_request_payload_bytes, observed_response_payload_bytes, \
                   observed_request_wire_bytes, observed_response_wire_bytes, status, rate_limited,
                    client_id, model_normalized, project_id,
                    CASE WHEN (input_tokens IS NOT NULL OR output_tokens IS NOT NULL
                           OR cache_read_tokens IS NOT NULL OR cache_write_tokens IS NOT NULL
                           OR reasoning_tokens IS NOT NULL) THEN 1 ELSE 0 END
                    , provider_normalized
                    , CASE WHEN (input_tokens IS NOT NULL OR output_tokens IS NOT NULL) THEN 1 ELSE 0 END
                    , CASE WHEN (reported_cost_micro_usd IS NOT NULL OR calculated_cost_micro_usd IS NOT NULL
                              OR estimated_cost_micro_usd IS NOT NULL) THEN 1 ELSE 0 END \
                   FROM model_calls WHERE started_at >= ?1 AND started_at < ?2";
        let mut stmt = c.prepare(sql)?;
        let mut rows = stmt.query([from.to_rfc3339(), to.to_rfc3339()])?;
        while let Some(row) = rows.next()? {
            let started: Option<String> = row.get(0)?;
            let key = started
                .as_deref()
                .and_then(parse_ts)
                .map(hour_bucket)
                .unwrap_or_else(|| fallback_bucket.clone());
            let acc = buckets.entry(key).or_default();
            acc.observe_call(&CallRow {
                started_at: started.unwrap_or_default(),
                duration_ms: row.get(1)?,
                first_byte_at: row.get(2)?,
                first_token_at: row.get(3)?,
                first_response_at: row.get(4)?,
                output_tokens: row.get(5)?,
                output_tokens_per_second_milli: row.get(6)?,
                generation_duration_ms: row.get(7)?,
                inter_token_latency_avg_ms: row.get(8)?,
                inter_token_latency_p95_ms: row.get(9)?,
                stall_count: row.get(10)?,
                observability_source: row.get(11)?,
                observability_quality: row.get(12)?,
                timing_source: row.get(13)?,
                timing_quality: row.get(14)?,
                observed_bytes: [row.get(15)?, row.get(16)?, row.get(17)?, row.get(18)?],
                status: row.get::<_, Option<String>>(19)?.unwrap_or_default(),
                rate_limited: row.get(20)?,
                client_id: row.get::<_, Option<String>>(21)?.unwrap_or_default(),
                model_normalized: row.get(22)?,
                project_id: row.get(23)?,
                has_tokens: row.get::<_, Option<i64>>(24)?.unwrap_or(0) != 0,
                provider_normalized: row.get(25)?,
                has_token_pair: row.get::<_, Option<i64>>(26)?.unwrap_or(0) != 0,
                has_any_cost: row.get::<_, Option<i64>>(27)?.unwrap_or(0) != 0,
            });
        }
    }
    // 热力图按小时归属到同一 bucket，便于与调用指标合并
    for (bucket, cell) in scan_heatmap_grouped(db, from, to, "", &[])? {
        buckets.entry(bucket).or_default().heatmap.merge(&cell);
    }
    for hour in buckets.values_mut() {
        hour.finalize();
    }
    Ok(buckets)
}

/// 分批重建 `[from, to)` 的预聚合并整体覆盖写入。
///
/// 按窗口推进（默认 24 小时），单批只保留一个窗口的聚合结果，内存与历史行数解耦，
/// 满足 `hub-runtime-memory` 对有界处理的要求。
pub fn rebuild(db: &HubDb, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<usize, StorageError> {
    if to <= from {
        return Ok(0);
    }
    const WINDOW_HOURS: i64 = 24;
    // 窗口必须从整点起步：否则窗口边界落在如 03:57，`03:00-03:57` 与 `03:57-04:00`
    // 会分属两个窗口却写同一个小时桶，后一个窗口的 ON CONFLICT 会覆盖前一个，
    // 该小时就只剩最后几分钟的样本（实测缺口 11,557 行，全部集中在每天 03:00）。
    let mut cursor = floor_hour(from);
    let mut written = 0usize;
    while cursor < to {
        let end = (cursor + chrono::Duration::hours(WINDOW_HOURS)).min(to);
        let mut buckets = build_window(db, cursor, end)?;
        // 每个完整小时都要有一行（含没有调用的空小时），否则快速路径的覆盖检查
        // 会因为“缺行”永久失败，预聚合就永远用不上。
        let mut hour = floor_hour(cursor);
        let last_hour = floor_hour(end);
        while hour < last_hour {
            buckets.entry(hour_bucket(hour)).or_default();
            hour += chrono::Duration::hours(1);
        }
        if !buckets.is_empty() {
            let mut c = db.conn();
            let tx = c.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO hourly_performance_rollups (bucket, payload, built_at) \
                     VALUES (?1, ?2, ?3) \
                     ON CONFLICT(bucket) DO UPDATE SET payload = excluded.payload, built_at = excluded.built_at",
                )?;
                let built_at = Utc::now().to_rfc3339();
                for (bucket, hour) in &buckets {
                    stmt.execute(metria_storage::rusqlite::params![
                        bucket,
                        hour.to_payload()?,
                        built_at
                    ])?;
                }
            }
            tx.commit()?;
            written += buckets.len();
        }
        cursor = end;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row(started: &str, duration: Option<i64>) -> CallRow {
        CallRow {
            started_at: started.to_string(),
            duration_ms: duration,
            first_byte_at: Some(started.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn histogram_quantile_is_monotonic_and_bounded() {
        let mut metric = IntMetric::default();
        for value in 1..=10_000i64 {
            metric.push(value);
        }
        let p50 = metric.quantile(0.50).unwrap();
        let p95 = metric.quantile(0.95).unwrap();
        let p99 = metric.quantile(0.99).unwrap();
        // 1..10000 的真实分位数约为 5000 / 9500 / 9900，直方图近似应落在 ±3.2% 内
        assert!((p50 - 5000).abs() <= 5000 * 32 / 1000, "p50={p50} 偏离过大");
        assert!((p95 - 9500).abs() <= 9500 * 32 / 1000, "p95={p95}");
        assert!((p99 - 9900).abs() <= 9900 * 32 / 1000, "p99={p99}");
        assert!(p50 <= p95 && p95 <= p99, "分位数必须单调");
        assert_eq!(metric.count, 10_000);
        assert_eq!(metric.sum, (1..=10_000i64).sum::<i64>(), "总和必须精确");
        assert_eq!(metric.avg().unwrap(), metric.sum / metric.count as i64);
    }

    #[test]
    fn empty_metric_returns_null_quantiles() {
        let metric = IntMetric::default();
        assert_eq!(metric.quantile(0.5), None, "无样本必须返回 null 而非 0");
        assert_eq!(metric.avg(), None);
        let speed = FloatMetric::default();
        assert_eq!(speed.quantile(0.95), None);
        assert_eq!(speed.avg(), None);
    }

    #[test]
    fn merge_is_equivalent_to_pushing_all_samples() {
        let mut a = IntMetric::default();
        let mut b = IntMetric::default();
        let mut merged = IntMetric::default();
        for value in 1..=5_000i64 {
            a.push(value);
            merged.push(value);
        }
        for value in 5_001..=12_000i64 {
            b.push(value);
            merged.push(value);
        }
        let mut combined = IntMetric::default();
        combined.merge(&a);
        combined.merge(&b);
        assert_eq!(combined.count, merged.count);
        assert_eq!(combined.sum, merged.sum);
        assert_eq!(combined.hist, merged.hist, "分箱必须可无损合并");
        for q in [0.5, 0.95, 0.99] {
            assert_eq!(combined.quantile(q), merged.quantile(q));
        }
    }

    #[test]
    fn observe_matches_performance_row_semantics() {
        let started = "2026-09-22T10:00:00+00:00";
        let mut hour = PerfHour::new();
        hour.observe_call(&sample_row(started, Some(1_500)));
        // started_at 不可解析：计入 total_calls 但不产生样本
        hour.observe_call(&CallRow {
            started_at: "not-a-time".into(),
            duration_ms: Some(99),
            ..Default::default()
        });
        assert_eq!(hour.call_count, 2, "解析失败的行仍计入 total_calls");
        assert_eq!(hour.duration.count, 1, "不可解析的行不进入 duration 样本");
        assert_eq!(hour.duration.sum, 1_500);
    }

    #[test]
    fn success_classification_matches_handler() {
        let mut hour = PerfHour::new();
        for (i, status) in ["success", "ok", "completed"].into_iter().enumerate() {
            hour.observe_call(&CallRow {
                started_at: format!("2026-09-22T10:{i:02}:00+00:00"),
                status: status.into(),
                ..Default::default()
            });
        }
        hour.observe_call(&CallRow {
            started_at: "2026-09-22T10:03:00+00:00".into(),
            status: "error".into(),
            ..Default::default()
        });
        assert_eq!(hour.success, 3);
        assert_eq!(hour.errors, 1);
        assert_eq!(hour.call_count, 4);
    }

    #[test]
    fn payload_roundtrip_and_version_guard() {
        let mut hour = PerfHour::new();
        hour.observe_call(&sample_row("2026-09-22T10:00:00+00:00", Some(800)));
        hour.heatmap.input_tokens = 42;
        let payload = hour.to_payload().unwrap();
        let restored = PerfHour::from_payload(&payload).expect("应能反序列化");
        assert_eq!(restored, hour);

        let mut stale = serde_json::from_str::<serde_json::Value>(&payload).unwrap();
        stale["v"] = serde_json::json!(0);
        assert!(
            PerfHour::from_payload(&stale.to_string()).is_none(),
            "版本不符必须判为不可用以触发回退"
        );
        assert!(
            PerfHour::from_payload("{ broken json").is_none(),
            "损坏 payload 必须判为不可用"
        );
    }

    #[test]
    fn complete_hours_excludes_partial_edges() {
        let from = Utc.with_ymd_and_hms(2026, 9, 22, 10, 30, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 9, 22, 13, 10, 0).unwrap();
        let hours = complete_hours(from, to);
        let buckets: Vec<String> = hours.iter().map(|h| hour_bucket(*h)).collect();
        assert_eq!(
            buckets,
            vec![
                "2026-09-22T11:00:00+00:00".to_string(),
                "2026-09-22T12:00:00+00:00".to_string(),
            ],
            "起止的不完整小时必须排除在外"
        );
    }
}
