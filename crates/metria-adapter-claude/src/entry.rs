//! Claude Code JSONL 容错解析：容忍未知字段与坏记录。

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// 顶层条目（容错：未知字段忽略，缺失字段为 None）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEntry {
    #[serde(rename = "type")]
    pub entry_type: String,
    pub session_id: Option<String>,
    pub timestamp: Option<String>,
    pub cwd: Option<String>,
    pub version: Option<String>,
    pub parent_uuid: Option<String>,
    pub prompt_id: Option<String>,
    pub uuid: Option<String>,
    pub git_branch: Option<String>,
    #[serde(default)]
    pub is_sidechain: bool,
    pub message: Option<RawMessage>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub leaf_uuid: Option<String>,
    #[serde(rename = "model")]
    pub model_field: Option<String>,
}

/// 消息（role/content/model/usage/id）。字段为 snake_case。
#[derive(Debug, Deserialize)]
pub struct RawMessage {
    pub id: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub usage: Option<RawUsage>,
    pub stop_reason: Option<String>,
    pub content: Option<RawContent>,
}

/// content 可以是字符串或 block 数组。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum RawContent {
    Text(String),
    Blocks(Vec<RawBlock>),
    Null(serde_json::Value),
}

impl RawContent {
    /// 是否为 tool_result 内容（用于区分真实用户消息与工具结果）。
    pub fn is_tool_result(&self) -> bool {
        match self {
            RawContent::Blocks(blocks) => blocks
                .iter()
                .any(|b| b.block_type == "tool_result" || b.block_type == "tool_use"),
            _ => false,
        }
    }
}

/// 内容 block。字段为 snake_case。
#[derive(Debug, Deserialize)]
pub struct RawBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
    pub thinking: Option<String>,
    pub signature: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub input: Option<serde_json::Value>,
    pub tool_use_id: Option<String>,
    pub content: Option<serde_json::Value>,
    pub is_error: Option<bool>,
}

impl RawBlock {
    pub fn visible_text(&self) -> Option<String> {
        match self.block_type.as_str() {
            "text" => self.text.clone(),
            "thinking" => self.thinking.clone(),
            "tool_result" => match &self.content {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(serde_json::Value::Array(items)) => {
                    let mut out = String::new();
                    for it in items {
                        if let Some(s) = it.get("text").and_then(|t| t.as_str()) {
                            out.push_str(s);
                            out.push('\n');
                        }
                    }
                    Some(out)
                }
                _ => None,
            },
            _ => None,
        }
    }
}

/// Usage。字段为 snake_case（input_tokens 等）。
#[derive(Debug, Deserialize, Default)]
pub struct RawUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub cache_write_input_tokens: Option<i64>,
    pub server_tool_use: Option<serde_json::Value>,
}

impl RawUsage {
    /// 是否没有任何 token 信息。
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cache_creation_input_tokens.is_none()
            && self.cache_read_input_tokens.is_none()
            && self.cache_write_input_tokens.is_none()
    }
}

/// 解析 RFC3339 时间戳（带容错：失败返回 None）。
pub fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// 判断条目是否为「真实用户消息」（非 tool_result、非系统内部）。
pub fn is_real_user_prompt(entry: &RawEntry) -> bool {
    match &entry.message {
        Some(m) if m.role.as_deref() == Some("user") => match &m.content {
            Some(c) => !c.is_tool_result(),
            None => false,
        },
        _ => false,
    }
}

/// 判断条目是否为 assistant 消息。
pub fn is_assistant(entry: &RawEntry) -> bool {
    entry
        .message
        .as_ref()
        .map(|m| m.role.as_deref() == Some("assistant"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modern_entry() {
        let line = r#"{"parentUuid":"p1","isSidechain":false,"message":{"id":"msg1","role":"assistant","model":"glm-5.2","usage":{"input_tokens":100,"output_tokens":20},"content":[{"type":"text","text":"hi"}]},"type":"assistant","uuid":"u1","timestamp":"2026-08-05T14:02:11.032Z","cwd":"/home/x","sessionId":"s1","version":"1.0.60"}"#;
        let e: RawEntry = serde_json::from_str(line).unwrap();
        assert_eq!(e.entry_type, "assistant");
        assert_eq!(e.session_id.as_deref(), Some("s1"));
        let m = e.message.unwrap();
        assert_eq!(m.role.as_deref(), Some("assistant"));
        assert_eq!(m.usage.unwrap().input_tokens, Some(100));
        assert!(matches!(m.content, Some(RawContent::Blocks(_))));
    }

    #[test]
    fn parse_unknown_fields_tolerated() {
        let line = r#"{"type":"mode","mode":"normal","sessionId":"s1","futureField":123}"#;
        let e: RawEntry = serde_json::from_str(line).unwrap();
        assert_eq!(e.entry_type, "mode");
    }

    #[test]
    fn timestamp_parsed() {
        assert!(parse_timestamp("2026-08-05T14:02:11.032Z").is_some());
        assert!(parse_timestamp("2026-08-05T14:02:11+08:00").is_some());
        assert!(parse_timestamp("garbage").is_none());
    }

    #[test]
    fn tool_result_detection() {
        let e: RawEntry = serde_json::from_str(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu1","content":"ok"}]}}"#,
        )
        .unwrap();
        assert!(!is_assistant(&e));
        let m = e.message.as_ref().unwrap();
        assert!(m.content.as_ref().unwrap().is_tool_result());
        assert!(!is_real_user_prompt(&e));
    }
}
