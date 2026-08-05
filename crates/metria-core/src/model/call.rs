//! ModelCall：一次可识别的模型调用。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::CallGranularity;
use super::ids::Id;

/// ModelCall：一次可识别的模型调用。
///
/// 若客户端日志无法区分单次调用，应使用最小可靠粒度并设置 `call_granularity`，
/// 禁止把 Session 级统计伪装成单次调用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCall {
    pub id: Id,
    pub source_call_id: Option<String>,
    pub node_id: String,
    pub collector_id: Id,
    pub client_id: String,
    pub source_id: Id,
    pub project_id: Option<String>,
    pub session_id: Id,
    pub turn_id: Option<Id>,
    pub provider_raw: Option<String>,
    pub provider_normalized: Option<String>,
    pub model_raw: Option<String>,
    pub model_normalized: Option<String>,
    pub started_at: DateTime<Utc>,
    pub first_response_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub status_code: Option<i64>,
    pub streaming: bool,
    pub stream_completed: Option<bool>,
    pub client_aborted: bool,
    pub retry_count: i64,
    pub call_granularity: CallGranularity,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub reported_cost_micro_usd: Option<i64>,
    pub calculated_cost_micro_usd: Option<i64>,
    pub estimated_cost_micro_usd: Option<i64>,
    pub usage_event_id: Option<String>,
    pub traffic_estimate_id: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn call_serde() {
        let c = ModelCall {
            id: Id::new(),
            source_call_id: None,
            node_id: "n".into(),
            collector_id: Id::new(),
            client_id: "codex".into(),
            source_id: Id::new(),
            project_id: None,
            session_id: Id::new(),
            turn_id: None,
            provider_raw: None,
            provider_normalized: None,
            model_raw: None,
            model_normalized: None,
            started_at: t(),
            first_response_at: None,
            completed_at: None,
            duration_ms: None,
            status: "success".into(),
            status_code: Some(200),
            streaming: false,
            stream_completed: None,
            client_aborted: false,
            retry_count: 0,
            call_granularity: CallGranularity::Turn,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            reported_cost_micro_usd: None,
            calculated_cost_micro_usd: None,
            estimated_cost_micro_usd: None,
            usage_event_id: None,
            traffic_estimate_id: None,
            created_at: t(),
            updated_at: t(),
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"call_granularity\":\"turn\""));
        let back: ModelCall = serde_json::from_str(&json).unwrap();
        assert_eq!(back.call_granularity, CallGranularity::Turn);
    }
}
