//! Rollup 引擎：事件写入后增量更新 hourly/daily 汇总。

use chrono::{DateTime, Utc};
use metria_storage::rusqlite::params;
use metria_storage::StorageError;
use serde_json::Value;
use std::collections::HashMap;

use crate::db::HubDb;

const ROLLUP_BATCH_SIZE: i64 = 512;

/// SQL 侧分桶表达式（与 `metria_core::time::bucket_hour` / `bucket_day` 的 UTC 形式对齐）。
const HOURLY_BUCKET: &str = "substr({ts},1,14)||'00:00+00:00'";
const DAILY_BUCKET: &str = "substr({ts},1,10)||'T00:00:00+00:00'";

/// 汇总增量类型。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RollupKind {
    Session,
    Call,
    Usage,
    Traffic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RollupDims {
    node: String,
    collector: String,
    client: String,
    source: String,
    project: String,
    provider: String,
    model: String,
    usage_source: String,
    usage_granularity: String,
    pricing_source: String,
    traffic_source: String,
    traffic_confidence: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RollupDelta {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    reported_cost: i64,
    calculated_cost: i64,
    estimated_cost: i64,
    estimated_request_bytes: i64,
    estimated_response_bytes: i64,
    estimated_total_bytes: i64,
    estimated_lower_bound_bytes: i64,
    estimated_upper_bound_bytes: i64,
    session_count: i64,
    model_call_count: i64,
    turn_count: i64,
    message_count: i64,
    tool_call_count: i64,
    subagent_count: i64,
}

impl RollupDelta {
    fn add(&mut self, other: &Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.reported_cost += other.reported_cost;
        self.calculated_cost += other.calculated_cost;
        self.estimated_cost += other.estimated_cost;
        self.estimated_request_bytes += other.estimated_request_bytes;
        self.estimated_response_bytes += other.estimated_response_bytes;
        self.estimated_total_bytes += other.estimated_total_bytes;
        self.estimated_lower_bound_bytes += other.estimated_lower_bound_bytes;
        self.estimated_upper_bound_bytes += other.estimated_upper_bound_bytes;
        self.session_count += other.session_count;
        self.model_call_count += other.model_call_count;
        self.turn_count += other.turn_count;
        self.message_count += other.message_count;
        self.tool_call_count += other.tool_call_count;
        self.subagent_count += other.subagent_count;
    }
}

/// 把一条事件拆成 rollup 维度与增量；批量路径按维度合并后再写入。
fn rollup_parts(kind: RollupKind, v: &Value) -> (RollupDims, RollupDelta) {
    let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
    let provider = g("provider_normalized").if_empty(|| g("provider"));
    let model = g("model_normalized").if_empty(|| g("model"));

    let (usage_source, granularity, pricing_source) = if kind == RollupKind::Usage {
        let q = v.get("quality").unwrap_or(&Value::Null);
        let cost = v.get("cost").unwrap_or(&Value::Null);
        let ps = if cost
            .get("reported_micro_usd")
            .and_then(|x| x.as_i64())
            .is_some()
        {
            "reported"
        } else if cost
            .get("calculated_micro_usd")
            .and_then(|x| x.as_i64())
            .is_some()
        {
            "calculated"
        } else if cost
            .get("estimated_micro_usd")
            .and_then(|x| x.as_i64())
            .is_some()
        {
            "estimated"
        } else {
            ""
        };
        (
            q.get("usage_source").and_then(|x| x.as_str()).unwrap_or(""),
            q.get("granularity").and_then(|x| x.as_str()).unwrap_or(""),
            ps,
        )
    } else {
        ("", "", "")
    };

    let (traffic_source, conf_level) = if kind == RollupKind::Traffic {
        (
            g("estimation_source"),
            confidence_level(v.get("confidence").and_then(|x| x.as_f64())),
        )
    } else {
        ("", "")
    };

    let getn = |k: &str| v.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
    // model_call_count 仅由 call 事件累计，避免与 session 携带值重复计数。
    // 子 Agent 会话不计入会话汇总（与 rebuild 的 `parent_session_id IS NULL` 同口径），
    // 否则增量路径与重建结果互相矛盾，对账会把一致的数据误判为漂移。
    let child_session = kind == RollupKind::Session && !g("parent_session_id").is_empty();
    let (session_count, message_count, tool_count, subagent_count, call_count) = match kind {
        RollupKind::Session if child_session => (0, 0, 0, 0, 0),
        RollupKind::Session => (
            1,
            getn("message_count"),
            getn("tool_call_count"),
            getn("subagent_count"),
            0,
        ),
        RollupKind::Call => (0, 0, 0, 0, 1),
        _ => (0, 0, 0, 0, 0),
    };

    let (input, output, cr, cw, rea) = if kind == RollupKind::Usage {
        let u = v.get("usage").unwrap_or(&Value::Null);
        let un = |k: &str| u.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        (
            un("input"),
            un("output"),
            un("cache_read"),
            un("cache_write"),
            un("reasoning"),
        )
    } else {
        (0, 0, 0, 0, 0)
    };

    let (rep, calc, est) = if kind == RollupKind::Usage {
        let c = v.get("cost").unwrap_or(&Value::Null);
        let cn = |k: &str| c.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        (
            cn("reported_micro_usd"),
            cn("calculated_micro_usd"),
            cn("estimated_micro_usd"),
        )
    } else {
        (0, 0, 0)
    };

    let (req_bytes, resp_bytes, total_bytes, lo, hi) = if kind == RollupKind::Traffic {
        (
            getn("estimated_request_wire_bytes"),
            getn("estimated_response_wire_bytes"),
            getn("estimated_total_wire_bytes"),
            getn("lower_bound_bytes"),
            getn("upper_bound_bytes"),
        )
    } else {
        (0, 0, 0, 0, 0)
    };

    (
        RollupDims {
            node: g("node_id").to_string(),
            collector: g("collector_id").to_string(),
            client: g("client_id").to_string(),
            source: g("source_id").to_string(),
            project: g("project_id").to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            usage_source: usage_source.to_string(),
            usage_granularity: granularity.to_string(),
            pricing_source: pricing_source.to_string(),
            traffic_source: traffic_source.to_string(),
            traffic_confidence: conf_level.to_string(),
        },
        RollupDelta {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cr,
            cache_write_tokens: cw,
            reasoning_tokens: rea,
            reported_cost: rep,
            calculated_cost: calc,
            estimated_cost: est,
            estimated_request_bytes: req_bytes,
            estimated_response_bytes: resp_bytes,
            estimated_total_bytes: total_bytes,
            estimated_lower_bound_bytes: lo,
            estimated_upper_bound_bytes: hi,
            session_count,
            model_call_count: call_count,
            turn_count: 0,
            message_count,
            tool_call_count: tool_count,
            subagent_count,
        },
    )
}

impl HubDb {
    /// 对一条事件做增量 rollup（hourly + daily）。
    pub fn rollup_event(&self, kind: &str, v: &Value) -> Result<(), StorageError> {
        self.rollup_events(&[(kind, v)])
    }

    /// 批量增量 rollup：先按 (表, 桶, 维度) 累加，再一次事务写入。
    ///
    /// 同一批次内落在同一桶/维度的事件只产生一条 UPSERT；调用方只应传入
    /// 已确认新落库的事件，重复批次不会重复累计。
    pub fn rollup_events(&self, events: &[(&str, &Value)]) -> Result<(), StorageError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut hourly: HashMap<(String, RollupDims), RollupDelta> = HashMap::new();
        let mut daily: HashMap<(String, RollupDims), RollupDelta> = HashMap::new();
        for (kind, v) in events {
            let rollup_kind = match *kind {
                "session" => RollupKind::Session,
                "call" => RollupKind::Call,
                "usage" => RollupKind::Usage,
                "traffic" => RollupKind::Traffic,
                _ => continue,
            };
            let ts = event_time(v);
            let (dims, delta) = rollup_parts(rollup_kind, v);
            let hourly_bucket = metria_core::time::bucket_hour(ts, chrono_tz::Tz::UTC).to_rfc3339();
            let daily_bucket = metria_core::time::bucket_day(ts, chrono_tz::Tz::UTC).to_rfc3339();
            hourly
                .entry((hourly_bucket, dims.clone()))
                .or_default()
                .add(&delta);
            daily.entry((daily_bucket, dims)).or_default().add(&delta);
        }
        if hourly.is_empty() && daily.is_empty() {
            return Ok(());
        }

        let c = self.conn();
        let tx = c.unchecked_transaction().map_err(StorageError::from)?;
        for (table, buckets) in [("hourly_rollups", &hourly), ("daily_rollups", &daily)] {
            if buckets.is_empty() {
                continue;
            }
            let sql = format!(
                "INSERT INTO {table} (
                    bucket, node_id, collector_id, client_id, source_id, project_id, provider, model,
                    usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    reported_cost, calculated_cost, estimated_cost,
                    estimated_request_bytes, estimated_response_bytes, estimated_total_bytes,
                    estimated_lower_bound_bytes, estimated_upper_bound_bytes,
                    session_count, model_call_count, turn_count, message_count, tool_call_count, subagent_count
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32)
                ON CONFLICT(bucket, node_id, collector_id, client_id, source_id, project_id, provider, model,
                    usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level)
                DO UPDATE SET
                    input_tokens = input_tokens + excluded.input_tokens,
                    output_tokens = output_tokens + excluded.output_tokens,
                    cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
                    cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
                    reasoning_tokens = reasoning_tokens + excluded.reasoning_tokens,
                    reported_cost = reported_cost + excluded.reported_cost,
                    calculated_cost = calculated_cost + excluded.calculated_cost,
                    estimated_cost = estimated_cost + excluded.estimated_cost,
                    estimated_request_bytes = estimated_request_bytes + excluded.estimated_request_bytes,
                    estimated_response_bytes = estimated_response_bytes + excluded.estimated_response_bytes,
                    estimated_total_bytes = estimated_total_bytes + excluded.estimated_total_bytes,
                    estimated_lower_bound_bytes = estimated_lower_bound_bytes + excluded.estimated_lower_bound_bytes,
                    estimated_upper_bound_bytes = estimated_upper_bound_bytes + excluded.estimated_upper_bound_bytes,
                    session_count = session_count + excluded.session_count,
                    model_call_count = model_call_count + excluded.model_call_count,
                    turn_count = turn_count + excluded.turn_count,
                    message_count = message_count + excluded.message_count,
                    tool_call_count = tool_call_count + excluded.tool_call_count,
                    subagent_count = subagent_count + excluded.subagent_count
                "
            );
            let mut stmt = tx.prepare(&sql).map_err(StorageError::from)?;
            for ((bucket, dims), delta) in buckets {
                stmt.execute(params![
                    bucket.as_str(),
                    dims.node.as_str(),
                    dims.collector.as_str(),
                    dims.client.as_str(),
                    dims.source.as_str(),
                    dims.project.as_str(),
                    dims.provider.as_str(),
                    dims.model.as_str(),
                    dims.usage_source.as_str(),
                    dims.usage_granularity.as_str(),
                    dims.pricing_source.as_str(),
                    dims.traffic_source.as_str(),
                    dims.traffic_confidence.as_str(),
                    delta.input_tokens,
                    delta.output_tokens,
                    delta.cache_read_tokens,
                    delta.cache_write_tokens,
                    delta.reasoning_tokens,
                    delta.reported_cost,
                    delta.calculated_cost,
                    delta.estimated_cost,
                    delta.estimated_request_bytes,
                    delta.estimated_response_bytes,
                    delta.estimated_total_bytes,
                    delta.estimated_lower_bound_bytes,
                    delta.estimated_upper_bound_bytes,
                    delta.session_count,
                    delta.model_call_count,
                    delta.turn_count,
                    delta.message_count,
                    delta.tool_call_count,
                    delta.subagent_count,
                ])
                .map_err(StorageError::from)?;
            }
        }
        tx.commit().map_err(StorageError::from)?;
        Ok(())
    }

    /// 重算指定时间范围（先删后插——M1 简化：全量重建通过从事件表聚合）。
    #[allow(dead_code)]
    pub fn rebuild_range(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        // 占位：完整重建实现见后续里程碑
        Ok(())
    }

    /// Rollup 对账：对比 raw 事件表与 rollup 汇总的计数/字节，返回差异摘要。
    ///
    /// 逐 bucket 对比 hourly_rollups 与 sessions/model_calls/usage_events/traffic_estimates
    /// 的聚合值。差异过大时记录告警（写入 server_meta），并可通过 [`Self::rebuild_rollups`]
    /// 触发重建。每次扫描限制在最近 N 天，避免全库扫描。
    pub fn reconcile_rollups(&self, days: i64) -> Result<ReconcileReport, StorageError> {
        let since = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let mut report = ReconcileReport::default();

        // 1. 每个 bucket 的 rollup 汇总值（作用域内借用 conn，离开即释放锁）
        let rollups: Vec<(String, i64, i64, i64, i64, i64)> = {
            let c = self.conn();
            let mut stmt = c
                .prepare(
                    "SELECT bucket,
                    SUM(session_count), SUM(model_call_count), SUM(input_tokens),
                    SUM(output_tokens), SUM(estimated_total_bytes)
                 FROM hourly_rollups WHERE julianday(bucket) >= julianday(?1) GROUP BY bucket ORDER BY bucket",
                )
                .map_err(StorageError::from)?;
            let rows = stmt
                .query_map([&since], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                })
                .map_err(StorageError::from)?;
            rows.filter_map(|r| r.ok()).collect()
        };

        for (bucket, rsess, rcall, rinput, routput, rtraffic) in &rollups {
            // 2. raw 事件表对应 bucket 的计数（按 UTC 分桶对齐）
            let actual = self
                .conn()
                .query_row(
                    // 会话计数与 rebuild 同为根会话口径：子 Agent 会话不计入 session_count，
                    // 否则只要窗口内有子会话，对账就会永久误报漂移并触发无效重建。
                    "SELECT
                        (SELECT COUNT(*) FROM sessions
                         WHERE started_at >= ?1 AND started_at < ?2
                           AND parent_session_id IS NULL),
                        (SELECT COUNT(*) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2),
                        (SELECT COALESCE(SUM(input_tokens),0) FROM usage_events WHERE timestamp >= ?1 AND timestamp < ?2),
                        (SELECT COALESCE(SUM(output_tokens),0) FROM usage_events WHERE timestamp >= ?1 AND timestamp < ?2),
                        (SELECT COALESCE(SUM(t.estimated_total_wire_bytes),0)
                         FROM model_calls m JOIN traffic_estimates t ON t.id = m.traffic_estimate_id
                         WHERE m.started_at >= ?1 AND m.started_at < ?2)",
                    params![bucket, next_bucket(bucket).to_rfc3339()],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, i64>(4)?,
                        ))
                    },
                )
                .map_err(StorageError::from)?;

            let (asess, acall, ainput, aoutput, atraffic) = actual;
            let sess_drift = asess.abs_diff(*rsess);
            let call_drift = acall.abs_diff(*rcall);
            let input_drift = ainput.abs_diff(*rinput);
            let output_drift = aoutput.abs_diff(*routput);
            let traffic_drift = atraffic.abs_diff(*rtraffic);
            report.buckets += 1;
            if sess_drift + call_drift + input_drift + output_drift + traffic_drift > 0 {
                report.drift_buckets += 1;
                tracing::warn!(
                    bucket = %bucket,
                    rollup_sessions = rsess, actual_sessions = asess,
                    rollup_calls = rcall, actual_calls = acall,
                    rollup_input = rinput, actual_input = ainput,
                    rollup_output = routput, actual_output = aoutput,
                    rollup_traffic = rtraffic, actual_traffic = atraffic,
                    "rollup 对账差异"
                );
            }
        }
        Ok(report)
    }

    /// 重建 rollup（session/call 计数 + usage 的 Token 与费用 + 流量估算字节）：先删后插，单事务批量聚合。
    ///
    /// 同一时间只允许一个重建任务；`try_rebuild_rollups` 用于周期任务，避免与启动全量
    /// 重建互相删写。批量 SQL 替代逐行重放，使全量重建从小时级降到秒级。
    pub fn rebuild_rollups(&self, days: i64) -> Result<usize, StorageError> {
        let _guard = self.rebuild_guard();
        self.rebuild_rollups_locked(days)
    }

    /// 尝试重建；已有重建在执行时返回 `Ok(None)`，不阻塞调用方。
    pub fn try_rebuild_rollups(&self, days: i64) -> Result<Option<usize>, StorageError> {
        let Some(_guard) = self.try_rebuild_guard() else {
            return Ok(None);
        };
        self.rebuild_rollups_locked(days).map(Some)
    }

    fn rebuild_rollups_locked(&self, days: i64) -> Result<usize, StorageError> {
        let _heap_release = crate::memory::HeapReleaseGuard;
        let since = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        // 归档水位下界：已归档区间的明细已删除，若允许重建先删这些 rollup 再从明细回填，
        // 只会写成 0/空，静默毁掉「聚合数据完整保留」的承诺。
        // 必须在取得连接锁**之前**读取——Mutex 不可重入，持锁再取锁会永久死锁。
        let floor = crate::archive::watermark(self)
            .map(|w| format!(" AND bucket >= '{w}'"))
            .unwrap_or_default();
        let c = self.conn();
        c.execute("BEGIN IMMEDIATE", [])
            .map_err(StorageError::from)?;
        let result = (|| -> Result<usize, StorageError> {
            c.execute(
                &format!(
                    "DELETE FROM hourly_rollups WHERE julianday(bucket) >= julianday(?1){floor}"
                ),
                [&since],
            )
            .map_err(StorageError::from)?;
            c.execute(
                &format!(
                    "DELETE FROM daily_rollups WHERE julianday(bucket) >= julianday(?1){floor}"
                ),
                [&since],
            )
            .map_err(StorageError::from)?;

            let pk = "bucket,node_id,collector_id,client_id,source_id,project_id,provider,model,\
                      usage_source,usage_granularity,pricing_source,traffic_estimation_source,\
                      traffic_confidence_level";
            let mut rebuilt = 0usize;
            for (table, ts) in [
                ("hourly_rollups", HOURLY_BUCKET),
                ("daily_rollups", DAILY_BUCKET),
            ] {
                let session_bucket = ts.replace("{ts}", "s.started_at");
                let call_bucket = ts.replace("{ts}", "started_at");
                let usage_bucket = ts.replace("{ts}", "timestamp");
                rebuilt += c
                    .execute(
                        &format!(
                            "INSERT INTO {table} (bucket, node_id, collector_id, client_id, source_id, \
                             project_id, provider, model, session_count, message_count, tool_call_count, subagent_count) \
                             SELECT {session_bucket}, s.node_id, s.collector_id, s.client_id, s.source_id, \
                               COALESCE(s.project_id,''), COALESCE(s.provider_normalized,''), \
                               COALESCE(s.primary_model_normalized,''), \
                               COUNT(*), SUM(COALESCE(s.message_count,0)), SUM(COALESCE(s.tool_call_count,0)), \
                               SUM(COALESCE(s.subagent_count,0)) \
                             FROM sessions s WHERE s.started_at >= ?1 AND s.parent_session_id IS NULL GROUP BY 1,2,3,4,5,6,7,8 \
                             ON CONFLICT({pk}) DO UPDATE SET \
                               session_count = session_count + excluded.session_count, \
                               message_count = message_count + excluded.message_count, \
                               tool_call_count = tool_call_count + excluded.tool_call_count, \
                               subagent_count = subagent_count + excluded.subagent_count"
                        ),
                        [&since],
                    )
                    .map_err(StorageError::from)?;
                rebuilt += c
                    .execute(
                        &format!(
                            "INSERT INTO {table} (bucket, node_id, collector_id, client_id, source_id, model, model_call_count) \
                             SELECT {call_bucket}, node_id, collector_id, client_id, source_id, \
                               COALESCE(model_normalized,''), COUNT(*) \
                             FROM model_calls WHERE started_at >= ?1 GROUP BY 1,2,3,4,5,6 \
                             ON CONFLICT({pk}) DO UPDATE SET \
                               model_call_count = model_call_count + excluded.model_call_count"
                        ),
                        [&since],
                    )
                    .map_err(StorageError::from)?;
                rebuilt += c
                    .execute(
                        &format!(
                            "INSERT INTO {table} (bucket, node_id, collector_id, client_id, source_id, provider, model, \
                             usage_source, usage_granularity, pricing_source, input_tokens, output_tokens, \
                             cache_read_tokens, cache_write_tokens, reasoning_tokens, reported_cost, \
                             calculated_cost, estimated_cost) \
                             SELECT {usage_bucket}, node_id, collector_id, client_id, source_id, \
                               COALESCE(provider_normalized,''), COALESCE(model_normalized,''), \
                               COALESCE(usage_source,''), COALESCE(usage_granularity,''), \
                               CASE WHEN reported_cost_micro_usd IS NOT NULL THEN 'reported' \
                                    WHEN calculated_cost_micro_usd IS NOT NULL THEN 'calculated' \
                                    WHEN estimated_cost_micro_usd IS NOT NULL THEN 'estimated' ELSE '' END, \
                               SUM(COALESCE(input_tokens,0)), SUM(COALESCE(output_tokens,0)), \
                               SUM(COALESCE(cache_read_tokens,0)), SUM(COALESCE(cache_write_tokens,0)), \
                               SUM(COALESCE(reasoning_tokens,0)), \
                               SUM(COALESCE(reported_cost_micro_usd,0)), \
                               SUM(COALESCE(calculated_cost_micro_usd,0)), \
                               SUM(COALESCE(estimated_cost_micro_usd,0)) \
                             FROM usage_events WHERE timestamp >= ?1 GROUP BY 1,2,3,4,5,6,7,8,9,10 \
                             ON CONFLICT({pk}) DO UPDATE SET \
                               input_tokens = input_tokens + excluded.input_tokens, \
                               output_tokens = output_tokens + excluded.output_tokens, \
                               cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens, \
                               cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens, \
                               reasoning_tokens = reasoning_tokens + excluded.reasoning_tokens, \
                               reported_cost = reported_cost + excluded.reported_cost, \
                               calculated_cost = calculated_cost + excluded.calculated_cost, \
                               estimated_cost = estimated_cost + excluded.estimated_cost"
                        ),
                        [&since],
                    )
                    .map_err(StorageError::from)?;
                let traffic_bucket = ts.replace("{ts}", "m.started_at");
                rebuilt += c
                    .execute(
                        &format!(
                            "INSERT INTO {table} (bucket, node_id, collector_id, client_id, source_id, provider, model, \
                             traffic_estimation_source, traffic_confidence_level, \
                             estimated_request_bytes, estimated_response_bytes, estimated_total_bytes, \
                             estimated_lower_bound_bytes, estimated_upper_bound_bytes) \
                             SELECT {traffic_bucket}, t.node_id, '', t.client_id, '', \
                               COALESCE(t.provider,''), COALESCE(t.model,''), \
                               COALESCE(t.estimation_source,''), \
                               CASE WHEN t.confidence IS NULL THEN '' \
                                    WHEN t.confidence >= 0.7 THEN 'high' \
                                    WHEN t.confidence >= 0.4 THEN 'medium' ELSE 'low' END, \
                               SUM(COALESCE(t.estimated_request_wire_bytes,0)), \
                               SUM(COALESCE(t.estimated_response_wire_bytes,0)), \
                               SUM(COALESCE(t.estimated_total_wire_bytes,0)), \
                               SUM(COALESCE(t.lower_bound_bytes,0)), \
                               SUM(COALESCE(t.upper_bound_bytes,0)) \
                             FROM model_calls m JOIN traffic_estimates t ON t.id = m.traffic_estimate_id \
                             WHERE m.started_at >= ?1 GROUP BY 1,2,3,4,5,6,7 \
                             ON CONFLICT({pk}) DO UPDATE SET \
                               estimated_request_bytes = estimated_request_bytes + excluded.estimated_request_bytes, \
                               estimated_response_bytes = estimated_response_bytes + excluded.estimated_response_bytes, \
                               estimated_total_bytes = estimated_total_bytes + excluded.estimated_total_bytes, \
                               estimated_lower_bound_bytes = estimated_lower_bound_bytes + excluded.estimated_lower_bound_bytes, \
                               estimated_upper_bound_bytes = estimated_upper_bound_bytes + excluded.estimated_upper_bound_bytes"
                        ),
                        [&since],
                    )
                    .map_err(StorageError::from)?;
            }
            c.execute("COMMIT", []).map_err(StorageError::from)?;
            Ok(rebuilt)
        })();
        if result.is_err() {
            let _ = c.execute("ROLLBACK", []);
        }
        result
    }
    /// 重建流量估算 rollup：先删后插（幂等）。
    ///
    /// 从 `traffic_estimates` 重放 traffic 事件，恢复 `hourly/daily_rollups`
    /// 的 `estimated_*_bytes` 等列。`rebuild_rollups` 已同时重放 session/call/usage/traffic，
    /// 本函数保留用于仅重建流量维度的场景。
    pub fn rebuild_traffic_rollups(&self, days: i64) -> Result<usize, StorageError> {
        let since = Utc::now() - chrono::Duration::days(days);
        self.rebuild_traffic_rollups_since(since)
    }

    /// 重建全部历史当前版本的估算流量汇总。
    pub fn rebuild_all_traffic_rollups(&self) -> Result<usize, StorageError> {
        self.rebuild_traffic_rollups_since(DateTime::<Utc>::UNIX_EPOCH)
    }

    fn rebuild_traffic_rollups_since(&self, since: DateTime<Utc>) -> Result<usize, StorageError> {
        let _heap_release = crate::memory::HeapReleaseGuard;
        let c = self.conn();
        c.execute(
            "DELETE FROM hourly_rollups WHERE julianday(bucket) >= julianday(?1) AND traffic_estimation_source != ''",
            [since.to_rfc3339()],
        )
        .map_err(StorageError::from)?;
        c.execute(
            "DELETE FROM daily_rollups WHERE julianday(bucket) >= julianday(?1) AND traffic_estimation_source != ''",
            [metria_core::time::bucket_day(since, chrono_tz::Tz::UTC).to_rfc3339()],
        )
        .map_err(StorageError::from)?;
        drop(c);

        let mut rebuilt = 0usize;
        let since_text = since.to_rfc3339();
        let mut last_rowid = 0i64;
        loop {
            let events = {
                let c = self.conn();
                let mut stmt = c
                    .prepare(
                        "SELECT m.rowid, t.node_id, t.client_id, t.provider, t.model,
                                t.estimated_request_wire_bytes, t.estimated_response_wire_bytes,
                                t.estimated_total_wire_bytes, t.lower_bound_bytes, t.upper_bound_bytes,
                                t.estimation_source, t.confidence, m.started_at
                         FROM model_calls m
                         JOIN traffic_estimates t ON t.id = m.traffic_estimate_id
                         WHERE m.rowid > ?1 AND m.started_at >= ?2
                         ORDER BY m.rowid LIMIT ?3",
                    )
                    .map_err(StorageError::from)?;
                let rows = stmt
                    .query_map(
                        metria_storage::rusqlite::params![
                            last_rowid,
                            since_text,
                            ROLLUP_BATCH_SIZE
                        ],
                        |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                serde_json::json!({
                                    "node_id": r.get::<_, String>(1)?,
                                    "client_id": r.get::<_, String>(2)?,
                                    "provider": r.get::<_, Option<String>>(3)?,
                                    "model": r.get::<_, Option<String>>(4)?,
                                    "estimated_request_wire_bytes": r.get::<_, Option<i64>>(5)?,
                                    "estimated_response_wire_bytes": r.get::<_, Option<i64>>(6)?,
                                    "estimated_total_wire_bytes": r.get::<_, Option<i64>>(7)?,
                                    "lower_bound_bytes": r.get::<_, Option<i64>>(8)?,
                                    "upper_bound_bytes": r.get::<_, Option<i64>>(9)?,
                                    "estimation_source": r.get::<_, Option<String>>(10)?,
                                    "confidence": r.get::<_, Option<f64>>(11)?,
                                    "timestamp": r.get::<_, String>(12)?,
                                }),
                            ))
                        },
                    )
                    .map_err(StorageError::from)?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(StorageError::from)?
            };
            let Some(last) = events.last().map(|(rowid, _)| *rowid) else {
                break;
            };
            last_rowid = last;
            let refs: Vec<(&str, &Value)> =
                events.iter().map(|(_, event)| ("traffic", event)).collect();
            self.rollup_events(&refs)?;
            rebuilt += events.len();
        }
        Ok(rebuilt)
    }
}

/// 对账报告摘要。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    /// 已对账的 bucket 数。
    pub buckets: i64,
    /// 存在差异的 bucket 数。
    pub drift_buckets: i64,
}

/// 取下一个小时 bucket 边界（对账对比用，保持 UTC 对齐）。
fn next_bucket(bucket: &str) -> DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(bucket)
        .map(|t| t.with_timezone(&Utc) + chrono::Duration::hours(1))
        .unwrap_or_else(|_| Utc::now())
}

fn event_time(v: &Value) -> DateTime<Utc> {
    let ts = v
        .get("timestamp")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("started_at").and_then(|x| x.as_str()))
        .or_else(|| v.get("calculated_at").and_then(|x| x.as_str()))
        .unwrap_or("");
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn confidence_level(conf: Option<f64>) -> &'static str {
    match conf {
        Some(c) if c >= 0.7 => "high",
        Some(c) if c >= 0.4 => "medium",
        Some(_) => "low",
        None => "",
    }
}

trait IfEmpty {
    fn if_empty<'a>(&'a self, alt: impl FnOnce() -> &'a str) -> &'a str;
}
impl IfEmpty for str {
    fn if_empty<'a>(&'a self, alt: impl FnOnce() -> &'a str) -> &'a str {
        if self.is_empty() {
            alt()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metria_storage::DbOptions;

    fn test_db(tag: &str) -> HubDb {
        let dir =
            std::env::temp_dir().join(format!("metria-rollup-test-{}-{}", std::process::id(), tag));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = crate::config::HubConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            data_dir: dir.clone(),
            database_url: format!("sqlite://{}/hub.db", dir.display()),
            content_mode: metria_core::ContentMode::Metadata,
            timezone: chrono_tz::Tz::UTC,
            log_filter: "error".into(),
            demo: false,
            oidc: None,
        };
        let db = HubDb::open(&cfg).unwrap();
        db.apply_migrations().unwrap();
        db
    }

    fn sess_json(key: &str, ts: &str) -> serde_json::Value {
        serde_json::json!({
            "source_session_id": key, "timestamp": ts, "started_at": ts,
            "node_id": "n1", "collector_id": "c1", "client_id": "claude-code",
            "source_id": "s1", "project_id": null, "provider_normalized": "anthropic",
            "model_normalized": "claude-sonnet-4.5",
            "message_count": 3, "tool_call_count": 1, "subagent_count": 0,
            "input_tokens": 1000, "output_tokens": 200,
            "cache_read_tokens": 0, "cache_write_tokens": 0, "reasoning_tokens": 0,
            "reported_cost_micro_usd": 5000, "calculated_cost_micro_usd": 5000,
            "estimated_cost_micro_usd": null,
            "estimated_request_bytes": 1000, "estimated_response_bytes": 2000,
            "estimated_total_bytes": 3000, "estimated_lower_bound_bytes": 2000,
            "estimated_upper_bound_bytes": 4000
        })
    }

    fn call_json(ts: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": ts, "started_at": ts,
            "node_id": "n1", "collector_id": "c1", "client_id": "claude-code",
            "source_id": "s1", "provider_normalized": "anthropic",
            "model_normalized": "claude-sonnet-4.5",
        })
    }

    fn traffic_json(ts: &str) -> serde_json::Value {
        serde_json::json!({
            "id": format!("traffic-{ts}"), "timestamp": ts, "calculated_at": ts,
            "node_id": "n1", "client_id": "claude-code", "provider": "anthropic",
            "model": "claude-sonnet-4.5",
            "estimated_request_wire_bytes": 5000, "estimated_response_wire_bytes": 7000,
            "estimated_total_wire_bytes": 12000, "lower_bound_bytes": 8000,
            "upper_bound_bytes": 16000, "estimation_source": "partial_reconstruction",
            "confidence": 0.5,
        })
    }

    fn usage_json(index: usize, with_cost: bool) -> serde_json::Value {
        serde_json::json!({
            "event_id": format!("batch-usage-{index}"),
            "schema_version": 1,
            "node_id": "batch-node",
            "collector_id": "batch-collector",
            "source_id": "batch-source",
            "client_id": "codex",
            "adapter_id": "codex",
            "adapter_version": "0.1.0",
            "timestamp": "2026-08-10T08:10:30Z",
            "provider_normalized": "openai",
            "model_normalized": "batch-model",
            "usage": { "input": 10, "output": 5, "cache_read": null, "cache_write": null, "reasoning": null },
            "cost": if with_cost { serde_json::json!({ "calculated_micro_usd": 100 }) } else { serde_json::json!({}) },
            "quality": { "usage_source": "reported", "granularity": "call", "confidence": 1.0 },
        })
    }

    #[test]
    fn reconcile_reports_no_drift_after_clean_ingest() {
        let db = test_db("reconcile");
        let now = Utc::now();
        let sess = sess_json("sess-a", &(now - chrono::Duration::hours(2)).to_rfc3339());
        let call = call_json(&(now - chrono::Duration::hours(1)).to_rfc3339());
        db.upsert_session(&sess).unwrap();
        db.insert_call(&call, "n1:sess-a").unwrap();
        db.rollup_event("session", &sess).unwrap();
        db.rollup_event("call", &call).unwrap();

        let report = db.reconcile_rollups(1).unwrap();
        assert!(report.drift_buckets == 0, "干净数据不应有漂移: {report:?}");
        assert!(report.buckets >= 1, "应有 bucket 被对账");
    }

    #[test]
    fn reconcile_ignores_subagent_sessions() {
        let db = test_db("reconcile-subagent");
        let now = Utc::now();
        let root = sess_json(
            "sess-root",
            &(now - chrono::Duration::hours(3)).to_rfc3339(),
        );
        let mut child = sess_json(
            "sess-child",
            &(now - chrono::Duration::hours(2)).to_rfc3339(),
        );
        child["parent_session_id"] = serde_json::json!("n1:sess-root");

        db.upsert_session(&root).unwrap();
        db.upsert_session(&child).unwrap();
        db.rollup_event("session", &root).unwrap();
        db.rollup_event("session", &child).unwrap();

        // 增量路径与对账同为根会话口径：子会话不制造漂移，也就不会触发无效重建
        let report = db.reconcile_rollups(1).unwrap();
        assert_eq!(
            report.drift_buckets, 0,
            "存在子会话时对账不应误报漂移: {report:?}"
        );

        // 重建路径必须给出与增量路径相同的结果
        db.rebuild_rollups(1).unwrap();
        let report = db.reconcile_rollups(1).unwrap();
        assert_eq!(report.drift_buckets, 0, "重建后仍应无漂移: {report:?}");

        let sessions: i64 = db
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(session_count), 0) FROM hourly_rollups",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sessions, 1, "rollup 会话数应只含根会话，实际 {sessions}");
    }

    #[test]
    fn rebuild_rollups_rebuilds_sessions_and_calls() {
        let db = test_db("rebuild");
        let now = Utc::now();
        let sess = sess_json("sess-b", &(now - chrono::Duration::hours(2)).to_rfc3339());
        let call = call_json(&(now - chrono::Duration::hours(1)).to_rfc3339());
        db.upsert_session(&sess).unwrap();
        db.insert_call(&call, "n1:sess-b").unwrap();
        db.rollup_event("session", &sess).unwrap();
        db.rollup_event("call", &call).unwrap();

        // 人为制造漂移：删掉 rollup 行（维护重建会先删整范围再回填）
        {
            let c = db.conn();
            c.execute("DELETE FROM hourly_rollups", []).unwrap();
        }
        let rebuilt = db.rebuild_rollups(1).unwrap();
        assert!(rebuilt >= 2, "应重建 session+call: {rebuilt}");
        let report = db.reconcile_rollups(1).unwrap();
        assert!(report.drift_buckets == 0, "重建后应无漂移: {report:?}");
    }

    #[test]
    fn rebuild_traffic_rollups_restores_bytes() {
        let db = test_db("traffic");
        let now = Utc::now();
        let call_time = (now - chrono::Duration::hours(2)).to_rfc3339();
        let mut t = traffic_json(&now.to_rfc3339());
        t["model_call_id"] = serde_json::json!("traffic-call");
        t["timestamp"] = serde_json::json!(call_time.clone());
        let mut call = call_json(&call_time);
        call["id"] = serde_json::json!("traffic-call");
        call["session_id"] = serde_json::json!("traffic-session");
        call["traffic_estimate_id"] = t["id"].clone();
        call["status"] = serde_json::json!("success");
        call["call_granularity"] = serde_json::json!("call");
        db.insert_call(&call, "traffic-session").unwrap();
        db.insert_traffic(&t).unwrap();
        db.rollup_event("traffic", &t).unwrap();

        // 人为制造漂移：删掉全部 rollup 行（维护重建会先删整范围，流量行也必须回填）
        {
            let c = db.conn();
            c.execute("DELETE FROM hourly_rollups", []).unwrap();
        }
        let rebuilt = db.rebuild_traffic_rollups(1).unwrap();
        assert!(rebuilt >= 1, "应重建 traffic 事件: {rebuilt}");

        let total: i64 = db
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(estimated_total_bytes),0) FROM hourly_rollups WHERE traffic_estimation_source != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 12000, "traffic 重建后字节应恢复");
        let bucket: String = db
            .conn()
            .query_row(
                "SELECT bucket FROM hourly_rollups WHERE traffic_estimation_source != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            bucket,
            metria_core::time::bucket_hour(
                DateTime::parse_from_rfc3339(&call_time)
                    .unwrap()
                    .with_timezone(&Utc),
                chrono_tz::Tz::UTC,
            )
            .to_rfc3339(),
            "traffic 必须按调用时间而不是 calculated_at 归桶"
        );
    }

    #[test]
    fn rebuild_rollups_processes_usage_batches() {
        let db = test_db("usage-batches");
        let count = ROLLUP_BATCH_SIZE as usize + 1;
        for index in 0..count {
            assert!(db
                .insert_usage(&usage_json(index, true), "batch-node:batch-session")
                .unwrap());
        }

        db.rebuild_rollups(36_500).unwrap();
        let (rows, input, output): (i64, i64, i64) = db
            .conn()
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
                 FROM hourly_rollups WHERE usage_source != ''",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(input, count as i64 * 10);
        assert_eq!(output, count as i64 * 5);
    }

    #[test]
    fn rebuild_traffic_rollups_processes_multiple_batches() {
        let db = test_db("traffic-batches");
        let count = ROLLUP_BATCH_SIZE as usize + 1;
        let call_time = "2026-08-10T08:10:30Z";
        for index in 0..count {
            let call_id = format!("batch-traffic-call-{index}");
            let estimate_id = format!("batch-traffic-estimate-{index}");
            let mut call = call_json(call_time);
            call["id"] = serde_json::json!(call_id);
            call["traffic_estimate_id"] = serde_json::json!(estimate_id);
            db.insert_call(&call, "batch-traffic-session").unwrap();

            let mut traffic = traffic_json(call_time);
            traffic["id"] = serde_json::json!(estimate_id);
            traffic["model_call_id"] = serde_json::json!(call_id);
            traffic["timestamp"] = serde_json::json!(call_time);
            db.insert_traffic(&traffic).unwrap();
        }

        assert_eq!(db.rebuild_all_traffic_rollups().unwrap(), count);
        let (rows, total): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(estimated_total_bytes), 0)
                 FROM hourly_rollups WHERE traffic_estimation_source != ''",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(total, count as i64 * 12_000);
    }

    #[test]
    fn reprice_processes_multiple_batches() {
        let db = test_db("reprice-batches");
        let count = ROLLUP_BATCH_SIZE as usize + 1;
        db.insert_pricing_rule(&serde_json::json!({
            "model_pattern": "batch-model",
            "provider_pattern": "*",
            "input_price": 1_000_000,
            "output_price": 2_000_000,
        }))
        .unwrap();
        for index in 0..count {
            assert!(db
                .insert_usage(&usage_json(index, false), "batch-node:batch-session")
                .unwrap());
        }

        let engine = crate::catalog::pricing_engine(&db);
        assert_eq!(db.reprice_all(&engine, false).unwrap(), count as i64);
        let priced: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM usage_events WHERE calculated_cost_micro_usd IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(priced, count as i64);
    }

    #[test]
    fn db_options_defaults_apply_pragmas() {
        let dir = std::env::temp_dir().join(format!("metria-pragma-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let conn = metria_storage::open(&path, &DbOptions::default()).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        metria_storage::wal_checkpoint(&conn).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn try_rebuild_rollups_skips_while_locked() {
        let db = test_db("rebuild-lock");
        let guard = db.rebuild_guard();
        assert!(
            db.try_rebuild_rollups(1).unwrap().is_none(),
            "已有重建在执行时必须跳过，避免互相删写"
        );
        drop(guard);
        assert!(
            db.try_rebuild_rollups(1).unwrap().is_some(),
            "重建锁释放后应可执行"
        );
    }

    #[test]
    fn rebuild_rollups_restores_counts_and_tokens() {
        let db = test_db("rebuild-match");
        let now = Utc::now();
        let sess = sess_json("sess-m", &(now - chrono::Duration::hours(2)).to_rfc3339());
        let call = call_json(&(now - chrono::Duration::hours(1)).to_rfc3339());
        db.upsert_session(&sess).unwrap();
        db.insert_call(&call, "n1:sess-m").unwrap();
        db.insert_usage(&usage_json(0, true), "batch-node:batch-session")
            .unwrap();
        let call_time = (now - chrono::Duration::hours(1)).to_rfc3339();
        let mut traffic = traffic_json(&now.to_rfc3339());
        traffic["model_call_id"] = serde_json::json!("traffic-call-m");
        let mut traffic_call = call_json(&call_time);
        traffic_call["id"] = serde_json::json!("traffic-call-m");
        traffic_call["session_id"] = serde_json::json!("sess-m");
        traffic_call["traffic_estimate_id"] = traffic["id"].clone();
        traffic_call["status"] = serde_json::json!("success");
        traffic_call["call_granularity"] = serde_json::json!("call");
        db.insert_call(&traffic_call, "n1:sess-m").unwrap();
        db.insert_traffic(&traffic).unwrap();

        // 人为清空 rollup：重建必须同时恢复 session/call 计数、usage 的 Token 与流量字节。
        {
            let c = db.conn();
            c.execute("DELETE FROM hourly_rollups", []).unwrap();
            c.execute("DELETE FROM daily_rollups", []).unwrap();
        }
        db.rebuild_rollups(36_500).unwrap();

        let report = db.reconcile_rollups(1).unwrap();
        assert_eq!(report.drift_buckets, 0, "重建后不应有漂移: {report:?}");
        let (input, output): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
                 FROM hourly_rollups WHERE usage_source != ''",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((input, output), (10, 5), "usage Token 必须随重建恢复");
        let traffic: i64 = db
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(estimated_total_bytes), 0)
                 FROM hourly_rollups WHERE traffic_estimation_source != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(traffic, 12000, "流量估算字节必须随重建恢复");
    }

    fn rollup_rows(db: &HubDb, table: &str) -> Vec<String> {
        let c = db.conn();
        let mut stmt = c.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |r| {
                let mut parts = Vec::with_capacity(columns);
                for index in 0..columns {
                    parts.push(format!("{:?}", r.get_ref(index)?));
                }
                Ok(parts.join("|"))
            })
            .unwrap();
        let mut rows: Vec<String> = rows.filter_map(Result::ok).collect();
        rows.sort();
        rows
    }

    #[test]
    fn batch_rollup_matches_per_event_rollup() {
        let single = test_db("batch-rollup-single");
        let batched = test_db("batch-rollup-batched");
        let events = vec![
            ("session", sess_json("sess-a", "2026-08-10T08:00:00Z")),
            ("call", call_json("2026-08-10T08:10:00Z")),
            ("usage", usage_json(1, true)),
            ("usage", usage_json(2, true)),
        ];
        for (kind, value) in &events {
            single.rollup_event(kind, value).unwrap();
        }
        let refs: Vec<(&str, &Value)> = events.iter().map(|(kind, value)| (*kind, value)).collect();
        batched.rollup_events(&refs).unwrap();

        assert_eq!(
            rollup_rows(&single, "hourly_rollups"),
            rollup_rows(&batched, "hourly_rollups")
        );
        assert_eq!(
            rollup_rows(&single, "daily_rollups"),
            rollup_rows(&batched, "daily_rollups")
        );
    }

    #[test]
    fn batch_rollup_aggregates_same_bucket_dimensions() {
        let db = test_db("batch-rollup-agg");
        let first = call_json("2026-08-10T08:10:00Z");
        let second = call_json("2026-08-10T08:20:00Z");
        db.rollup_events(&[("call", &first), ("call", &second)])
            .unwrap();

        let (rows, calls): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(model_call_count), 0) FROM hourly_rollups",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(rows, 1, "同一桶/维度应合并为一行");
        assert_eq!(calls, 2, "两条调用应累计为 2");
    }
}
