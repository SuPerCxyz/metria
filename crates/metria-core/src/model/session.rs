//! Session、Turn、Message 模型。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{SessionStatus, UsageGranularity, UsageSource};
use super::ids::{ContentHash, Id};

/// Session：一次客户端会话。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: Id,
    pub source_session_id: String,
    pub node_id: String,
    pub collector_id: Id,
    pub source_id: Id,
    pub client_id: String,
    pub project_id: Option<String>,
    pub parent_session_id: Option<Id>,
    pub title: Option<String>,
    pub working_directory_hash: Option<ContentHash>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub provider_raw: Option<String>,
    pub provider_normalized: Option<String>,
    pub primary_model_raw: Option<String>,
    pub primary_model_normalized: Option<String>,
    pub status: SessionStatus,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub subagent_count: i64,
    pub model_call_count: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub reported_cost_micro_usd: Option<i64>,
    pub calculated_cost_micro_usd: Option<i64>,
    pub estimated_cost_micro_usd: Option<i64>,
    pub estimated_request_bytes: Option<i64>,
    pub estimated_response_bytes: Option<i64>,
    pub estimated_total_bytes: Option<i64>,
    pub traffic_confidence: Option<f32>,
    pub content_available: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Turn：一次用户请求及其响应回合。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub id: Id,
    pub session_id: Id,
    pub source_turn_id: Option<String>,
    pub sequence: i64,
    pub role: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub provider_raw: Option<String>,
    pub provider_normalized: Option<String>,
    pub model_raw: Option<String>,
    pub model_normalized: Option<String>,
    pub finish_reason: Option<String>,
    pub usage_source: UsageSource,
    pub usage_granularity: UsageGranularity,
    pub usage_confidence: Option<f32>,
    pub created_at: DateTime<Utc>,
}

/// Message：会话中的一条消息。
///
/// 正文存储由 content_mode 控制：
/// - none / metadata：`content` 为 None，但仍保存 content_hash/content_length/utf8_bytes。
/// - full：保存完整正文。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: Id,
    pub turn_id: Option<Id>,
    pub session_id: Id,
    pub source_message_id: Option<String>,
    pub sequence: i64,
    pub role: String,
    pub content_type: String,
    pub content: Option<String>,
    pub content_hash: Option<ContentHash>,
    pub content_length: i64,
    pub utf8_bytes: i64,
    pub created_at: DateTime<Utc>,
    pub redacted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn session() -> Session {
        Session {
            id: Id::new(),
            source_session_id: "src-1".into(),
            node_id: "node-01".into(),
            collector_id: Id::new(),
            source_id: Id::new(),
            client_id: "claude-code".into(),
            project_id: None,
            parent_session_id: None,
            title: None,
            working_directory_hash: None,
            started_at: t(),
            ended_at: None,
            last_activity_at: None,
            provider_raw: None,
            provider_normalized: None,
            primary_model_raw: None,
            primary_model_normalized: None,
            status: SessionStatus::Active,
            message_count: 0,
            tool_call_count: 0,
            subagent_count: 0,
            model_call_count: 0,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            reported_cost_micro_usd: None,
            calculated_cost_micro_usd: None,
            estimated_cost_micro_usd: None,
            estimated_request_bytes: None,
            estimated_response_bytes: None,
            estimated_total_bytes: None,
            traffic_confidence: None,
            content_available: false,
            created_at: t(),
            updated_at: t(),
        }
    }

    #[test]
    fn missing_tokens_serialize_as_null() {
        let json = serde_json::to_string(&session()).unwrap();
        assert!(
            json.contains("\"input_tokens\":null"),
            "缺失 Token 必须为 null 而非 0"
        );
        assert!(!json.contains("\"input_tokens\":0"));
    }

    #[test]
    fn message_metadata_without_content() {
        let m = Message {
            id: Id::new(),
            turn_id: None,
            session_id: Id::new(),
            source_message_id: Some("m1".into()),
            sequence: 1,
            role: "user".into(),
            content_type: "text".into(),
            content: None,
            content_hash: Some(ContentHash::hash_str("hello world")),
            content_length: 11,
            utf8_bytes: 11,
            created_at: t(),
            redacted: true,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"content\":null"));
        assert!(json.contains("\"content_length\":11"));
    }
}
