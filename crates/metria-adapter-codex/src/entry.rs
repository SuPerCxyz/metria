//! Codex rollout JSONL 容错解析。

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// 顶层事件（所有事件均有 timestamp/type，payload 结构随 type 变化）。
#[derive(Debug, Deserialize)]
pub struct RawEvent {
    pub timestamp: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: Option<serde_json::Value>,
}

/// session_meta payload。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionMeta {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub cli_version: Option<String>,
    pub model_provider: Option<String>,
    pub timestamp: Option<String>,
}

/// token_count payload。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenCount {
    pub info: Option<TokenCountInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenCountInfo {
    pub last_token_usage: Option<UsageSummary>,
    pub total_token_usage: Option<UsageSummary>,
    pub model_context_window: Option<i64>,
}

/// Usage 摘要（Codex 字段）。
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct UsageSummary {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_write_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

impl UsageSummary {
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cached_input_tokens.is_none()
            && self.cache_write_input_tokens.is_none()
            && self.reasoning_output_tokens.is_none()
    }
}

/// response_item message payload。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MessagePayload {
    pub id: Option<String>,
    pub role: Option<String>,
    pub content: Option<Vec<ContentItem>>,
}

#[derive(Debug, Deserialize)]
pub struct ContentItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub text: Option<String>,
}

/// custom_tool_call / function_call payload。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCallPayload {
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
    pub input: Option<serde_json::Value>,
    pub call_id: Option<String>,
}

/// custom_tool_call_output / function_call_output payload。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCallOutputPayload {
    pub call_id: Option<String>,
    pub output: Option<serde_json::Value>,
    pub is_error: Option<bool>,
}

/// event_msg 中的 user_message payload。
#[derive(Debug, Deserialize)]
pub struct UserMessagePayload {
    pub message: Option<String>,
}

/// reasoning payload。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReasoningPayload {
    pub id: Option<String>,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub encrypted_content: Option<String>,
}

/// 解析 RFC3339 时间戳。
pub fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_session_meta() {
        let line = r#"{"timestamp":"2026-07-04T04:07:07.607Z","type":"session_meta","payload":{"session_id":"abc","cwd":"/home/x","cli_version":"0.142.5","model_provider":"custom"}}"#;
        let e: RawEvent = serde_json::from_str(line).unwrap();
        assert_eq!(e.event_type, "session_meta");
        let meta: SessionMeta = serde_json::from_value(e.payload.unwrap()).unwrap();
        assert_eq!(meta.session_id.as_deref(), Some("abc"));
        assert_eq!(meta.cli_version.as_deref(), Some("0.142.5"));
    }

    #[test]
    fn parse_token_count() {
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":21154,"cached_input_tokens":0,"cache_write_input_tokens":0,"output_tokens":370,"reasoning_output_tokens":107,"total_tokens":21524}}}}"#;
        let e: RawEvent = serde_json::from_str(line).unwrap();
        let tc: TokenCount = serde_json::from_value(e.payload.unwrap()).unwrap();
        let u = tc.info.unwrap().last_token_usage.unwrap();
        assert_eq!(u.input_tokens, Some(21154));
        assert_eq!(u.reasoning_output_tokens, Some(107));
    }

    #[test]
    fn parse_message_and_tool_call() {
        let m: RawEvent = serde_json::from_str(
            r#"{"type":"response_item","payload":{"type":"message","id":"msg1","role":"assistant","content":[{"type":"output_text","text":"hi"}]}}"#,
        )
        .unwrap();
        let mp: MessagePayload = serde_json::from_value(m.payload.unwrap()).unwrap();
        assert_eq!(mp.role.as_deref(), Some("assistant"));
        assert_eq!(mp.content.unwrap()[0].item_type, "output_text");

        let t: RawEvent = serde_json::from_str(
            r#"{"type":"response_item","payload":{"type":"custom_tool_call","id":"c1","name":"Read","input":{"path":"a"},"call_id":"call1"}}"#,
        )
        .unwrap();
        let tp: ToolCallPayload = serde_json::from_value(t.payload.unwrap()).unwrap();
        assert_eq!(tp.name.as_deref(), Some("Read"));
        assert_eq!(tp.call_id.as_deref(), Some("call1"));
    }

    #[test]
    fn unknown_events_tolerated() {
        let e: RawEvent = serde_json::from_str(
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"future_event","payload":{"whatever":1}}"#,
        )
        .unwrap();
        assert_eq!(e.event_type, "future_event");
    }
}
