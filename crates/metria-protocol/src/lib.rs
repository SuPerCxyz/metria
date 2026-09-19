//! metria-protocol: Agent 与 Hub 之间的线协议类型、序列化与校验。
#![warn(missing_debug_implementations, rust_2018_idioms)]

use serde::{Deserialize, Serialize};

pub mod limits {
    /// 协议 schema 版本。
    pub const SCHEMA_VERSION: u32 = 1;
    /// Collector 线协议版本：不兼容时 register 拒绝并返回 400。
    pub const PROTOCOL_VERSION: u32 = 1;
    /// 单批最大事件数。
    pub const MAX_EVENTS_PER_BATCH: usize = 256;
    /// 单批压缩后最大字节数。
    pub const MAX_COMPRESSED_BODY: usize = 256 * 1024;
    /// 单批解压后最大字节数。
    pub const MAX_UNCOMPRESSED_BODY: usize = 8 * 1024 * 1024;
    /// JSON 最大深度。
    pub const MAX_JSON_DEPTH: usize = 32;
    /// 单条事件最大字节数。
    pub const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
    /// 单次游标同步最大条目数。
    pub const MAX_CURSORS_PER_SYNC: usize = 256;
    /// 单条游标 JSON 最大字节数。
    pub const MAX_CURSOR_JSON_BYTES: usize = 64 * 1024;
}

/// Collector 注册请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub schema_version: u32,
    pub node_id: String,
    pub node_name: String,
    pub node_platform: Option<String>,
    pub node_architecture: Option<String>,
    pub node_timezone: Option<String>,
    pub agent_version: String,
    pub protocol_version: u32,
    pub container_image: Option<String>,
    pub collector_id_hint: Option<String>,
}

/// Collector 注册响应（返回已注册信息）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub node_id: String,
    pub collector_id: String,
    pub ok: bool,
    pub message: Option<String>,
}

/// 心跳请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub schema_version: u32,
    pub node_id: String,
    pub collector_id: String,
    pub spool_pending_events: i64,
    pub spool_size_bytes: i64,
    pub source_count: i64,
    pub agent_clock: chrono::DateTime<chrono::Utc>,
}

/// 心跳响应（可下发配置）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub ok: bool,
    /// 采集器配置覆盖（可选）。
    pub config: Option<CollectorConfig>,
}

/// 一条 Source 游标（Hub 存储形态；cursor_json 对 Hub 不透明）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorEntry {
    pub source_id: String,
    pub cursor_json: String,
    pub updated_at: Option<String>,
}

/// 游标推进请求（Agent 确认上传后提交；幂等 upsert）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorSyncRequest {
    pub schema_version: u32,
    pub cursors: Vec<CursorEntry>,
}

/// 游标读取/推进响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CursorSyncResponse {
    pub cursors: Vec<CursorEntry>,
}

/// 校验游标同步请求基本合法性（返回错误信息）。
pub fn validate_cursor_sync(req: &CursorSyncRequest) -> Result<(), String> {
    if req.schema_version != limits::SCHEMA_VERSION {
        return Err(format!("不支持的 schema_version: {}", req.schema_version));
    }
    if req.cursors.len() > limits::MAX_CURSORS_PER_SYNC {
        return Err(format!(
            "游标条目 {} 超过上限 {}",
            req.cursors.len(),
            limits::MAX_CURSORS_PER_SYNC
        ));
    }
    for c in &req.cursors {
        if c.source_id.trim().is_empty() {
            return Err("source_id 为空".into());
        }
        if c.cursor_json.len() > limits::MAX_CURSOR_JSON_BYTES {
            return Err(format!(
                "游标 {} 超过大小上限（{} > {} 字节）",
                c.source_id,
                c.cursor_json.len(),
                limits::MAX_CURSOR_JSON_BYTES
            ));
        }
    }
    Ok(())
}

/// Hub 下发的采集器配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectorConfig {
    pub content_mode: Option<String>,
    pub scan_interval_seconds: Option<u64>,
    pub pricing_rules_etag: Option<String>,
}

/// 上传批次。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadBatch {
    pub schema_version: u32,
    pub batch_id: String,
    pub node_id: String,
    pub collector_id: String,
    pub agent_version: String,
    pub events: Vec<BatchEvent>,
}

/// 批次内单条归一化事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEvent {
    /// 事件类型：session / call / usage / traffic / tool / subagent
    pub kind: String,
    pub event_id: String,
    pub payload: serde_json::Value,
}

/// 上传响应（部分成功语义）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponse {
    pub batch_id: String,
    pub ok: bool,
    pub accepted: Vec<String>,
    pub duplicate: Vec<String>,
    pub failed: Vec<FailedEvent>,
    pub message: Option<String>,
}

/// Pull 模式批次确认请求（Agent `/ack`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub batch_id: String,
}

/// Pull 模式批次确认响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckResponse {
    pub ok: bool,
    /// already=true 表示批次此前已确认（幂等重放）
    pub already: bool,
}

/// 失败事件明细。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedEvent {
    pub event_id: String,
    pub reason: String,
    /// true 表示可重试，false 表示应转死信
    pub retryable: bool,
}

/// 状态查询请求（GET status）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorStatusRequest {
    pub node_id: String,
    pub collector_id: String,
}

/// 状态响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorStatusResponse {
    pub ok: bool,
    pub node_id: String,
    pub collector_id: String,
    pub hub_time: chrono::DateTime<chrono::Utc>,
}

/// 通用错误响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub error: String,
    pub message: String,
}

/// 校验批次基本合法性（返回错误信息）。
pub fn validate_batch(batch: &UploadBatch) -> Result<(), String> {
    if batch.schema_version != limits::SCHEMA_VERSION {
        return Err(format!("不支持的 schema_version: {}", batch.schema_version));
    }
    if batch.batch_id.trim().is_empty() {
        return Err("batch_id 为空".into());
    }
    if batch.events.len() > limits::MAX_EVENTS_PER_BATCH {
        return Err(format!(
            "事件数 {} 超过上限 {}",
            batch.events.len(),
            limits::MAX_EVENTS_PER_BATCH
        ));
    }
    let mut total: usize = 0;
    for e in &batch.events {
        if e.kind == "call" {
            validate_call_payload(&e.payload)?;
        }
        let s = serde_json::to_string(&e.payload).map_err(|e| e.to_string())?;
        if s.len() > limits::MAX_EVENT_BYTES {
            return Err(format!(
                "单条事件 {} 超过大小上限（{} > {} 字节）",
                e.event_id,
                s.len(),
                limits::MAX_EVENT_BYTES
            ));
        }
        if json_depth(&e.payload) > limits::MAX_JSON_DEPTH {
            return Err(format!(
                "单条事件 {} 嵌套深度超过上限 {}",
                e.event_id,
                limits::MAX_JSON_DEPTH
            ));
        }
        total += s.len();
    }
    if total > limits::MAX_UNCOMPRESSED_BODY {
        return Err("解压后超过大小上限".into());
    }
    Ok(())
}

/// 校验调用观测字段的边界；旧调用没有这些字段时保持兼容。
fn validate_call_payload(payload: &serde_json::Value) -> Result<(), String> {
    const NON_NEGATIVE: &[&str] = &[
        "duration_ms",
        "first_byte_latency_ms",
        "ttft_ms",
        "generation_duration_ms",
        "output_tokens_per_second_milli",
        "inter_token_latency_avg_ms",
        "inter_token_latency_p95_ms",
        "stall_count",
        "stall_duration_ms",
        "observed_request_payload_bytes",
        "observed_response_payload_bytes",
        "observed_request_wire_bytes",
        "observed_response_wire_bytes",
    ];
    for key in NON_NEGATIVE {
        if let Some(value) = payload.get(*key).filter(|v| !v.is_null()) {
            let Some(number) = value.as_i64() else {
                return Err(format!("call.{key} 必须是整数"));
            };
            if number < 0 {
                return Err(format!("call.{key} 不得为负数"));
            }
        }
    }

    if let Some(value) = payload.get("rate_limited").filter(|v| !v.is_null()) {
        if !value.is_boolean() {
            return Err("call.rate_limited 必须是布尔值".into());
        }
    }

    let timestamps = [
        "started_at",
        "first_byte_at",
        "first_token_at",
        "last_output_at",
        "completed_at",
    ];
    let mut previous = None;
    for key in timestamps {
        let Some(raw) = payload.get(key).and_then(|v| v.as_str()) else {
            continue;
        };
        let parsed = chrono::DateTime::parse_from_rfc3339(raw)
            .map_err(|_| format!("call.{key} 必须是 RFC3339 时间"))?;
        if let Some(before) = previous {
            if parsed < before {
                return Err(format!("call.{key} 时间早于前一阶段"));
            }
        }
        previous = Some(parsed);
    }

    for key in ["observability_source", "observability_quality"] {
        if let Some(value) = payload.get(key).filter(|v| !v.is_null()) {
            let Some(text) = value.as_str() else {
                return Err(format!("call.{key} 必须是字符串"));
            };
            if text.is_empty() || text.len() > 64 {
                return Err(format!("call.{key} 长度非法"));
            }
            let allowed = match key {
                "observability_source" => [
                    "runtime_http",
                    "runtime_sdk",
                    "runtime_cli",
                    "ordinary_log",
                    "unknown",
                ]
                .as_slice(),
                "observability_quality" => ["observed", "partial", "unavailable"].as_slice(),
                _ => &[],
            };
            if !allowed.contains(&text) {
                return Err(format!("call.{key} 值不受支持: {text}"));
            }
        }
    }
    Ok(())
}

/// 计算 JSON 值嵌套深度（顶层为 1）。
pub fn json_depth(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::Array(items) => 1 + items.iter().map(json_depth).max().unwrap_or(0),
        serde_json::Value::Object(map) => 1 + map.values().map(json_depth).max().unwrap_or(0),
        _ => 1,
    }
}

/// 事件 ID 基本格式校验。
pub fn valid_event_id(id: &str) -> bool {
    id.len() >= 10 && id.len() <= 130
}

/// 事件类型白名单。
pub fn valid_kind(kind: &str) -> bool {
    matches!(
        kind,
        "session"
            | "source"
            | "source_sync"
            | "call"
            | "usage"
            | "message"
            | "traffic"
            | "tool"
            | "subagent"
            | "traffic_sample"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_detail_kind_is_allowed() {
        assert!(valid_kind("message"));
        assert!(valid_kind("tool"));
        assert!(valid_kind("subagent"));
        assert!(valid_kind("source_sync"));
        assert!(!valid_kind("unknown_kind"));
    }

    fn batch(payload: serde_json::Value) -> UploadBatch {
        UploadBatch {
            schema_version: limits::SCHEMA_VERSION,
            batch_id: "batch-observe".into(),
            node_id: "node-1".into(),
            collector_id: "collector-1".into(),
            agent_version: "test".into(),
            events: vec![BatchEvent {
                kind: "call".into(),
                event_id: "blake3:call-observe".into(),
                payload,
            }],
        }
    }

    #[test]
    fn accepts_observed_call_and_legacy_call() {
        let observed = serde_json::json!({
            "started_at": "2026-09-18T00:00:00Z",
            "first_byte_at": "2026-09-18T00:00:00.100Z",
            "first_token_at": "2026-09-18T00:00:00.200Z",
            "last_output_at": "2026-09-18T00:00:01Z",
            "completed_at": "2026-09-18T00:00:01.100Z",
            "ttft_ms": 200,
            "observed_response_wire_bytes": 1024,
            "observability_source": "runtime_http",
            "observability_quality": "observed"
        });
        assert!(validate_batch(&batch(observed)).is_ok());
        assert!(validate_batch(&batch(serde_json::json!({
            "started_at": "2026-09-18T00:00:00Z"
        })))
        .is_ok());
    }

    #[test]
    fn rejects_negative_or_out_of_order_observation() {
        let negative = serde_json::json!({"ttft_ms": -1});
        assert!(validate_batch(&batch(negative))
            .unwrap_err()
            .contains("不得为负"));
        let out_of_order = serde_json::json!({
            "started_at": "2026-09-18T00:00:02Z",
            "first_token_at": "2026-09-18T00:00:01Z"
        });
        assert!(validate_batch(&batch(out_of_order))
            .unwrap_err()
            .contains("早于"));
    }

    #[test]
    fn rejects_unknown_observability_enum() {
        assert!(validate_batch(&batch(serde_json::json!({
            "observability_source": "guessing"
        })))
        .is_err());
        assert!(validate_batch(&batch(serde_json::json!({
            "observability_quality": "estimated"
        })))
        .is_err());
    }
}
