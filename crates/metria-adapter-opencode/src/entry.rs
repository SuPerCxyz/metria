//! OpenCode SQLite 容错解析：message.data / part.data / session 行。

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// session 行（按需字段）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionRow {
    pub id: Option<String>,
    pub project_id: Option<String>,
    pub parent_id: Option<String>,
    pub directory: Option<String>,
    pub title: Option<String>,
    pub cost: Option<f64>,
    pub tokens_input: Option<i64>,
    pub tokens_output: Option<i64>,
    pub tokens_reasoning: Option<i64>,
    pub tokens_cache_read: Option<i64>,
    pub tokens_cache_write: Option<i64>,
    pub model: Option<String>,
    pub time_created: Option<i64>,
    pub time_updated: Option<i64>,
}

/// message.data JSON。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageData {
    pub role: Option<String>,
    pub agent: Option<String>,
    pub model: Option<MessageModel>,
    pub model_id: Option<String>,
    pub provider_id: Option<String>,
    pub tokens: Option<MessageTokens>,
    pub cost: Option<f64>,
    pub finish: Option<String>,
    pub time: Option<MessageTime>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageModel {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageTokens {
    pub total: Option<i64>,
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub reasoning: Option<i64>,
    pub cache: Option<CacheTokens>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CacheTokens {
    pub read: Option<i64>,
    pub write: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageTime {
    pub created: Option<i64>,
    pub completed: Option<i64>,
}

/// part.data JSON。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PartData {
    #[serde(rename = "type")]
    pub part_type: Option<String>,
    pub text: Option<String>,
    pub tool: Option<String>,
    #[serde(rename = "callID")]
    pub call_id: Option<String>,
    pub state: Option<PartState>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PartState {
    pub status: Option<String>,
    pub input: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub time: Option<PartTime>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PartTime {
    pub start: Option<i64>,
    pub end: Option<i64>,
}

/// 毫秒时间戳 → UTC。
pub fn from_millis(ms: i64) -> Option<DateTime<Utc>> {
    chrono::DateTime::from_timestamp_millis(ms)
}

/// 会话模型 JSON 解析：`{"id":"...","providerID":"..."}`。
pub fn parse_session_model(json: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(j) = json else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(j) else {
        return (None, None);
    };
    let model = v.get("id").and_then(|m| m.as_str()).map(|s| s.to_string());
    let provider = v
        .get("providerID")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string());
    (model, provider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_data() {
        let d: MessageData = serde_json::from_str(
            r#"{"parentID":"p1","role":"assistant","agent":"build","cost":0,"tokens":{"total":35855,"input":35528,"output":94,"reasoning":233,"cache":{"write":0,"read":10}},"modelID":"deepseek-v4-flash-free","providerID":"opencode","time":{"created":1783137427715,"completed":1783137436954},"finish":"tool-calls"}"#,
        )
        .unwrap();
        assert_eq!(d.role.as_deref(), Some("assistant"));
        let t = d.tokens.unwrap();
        assert_eq!(t.input, Some(35528));
        assert_eq!(t.reasoning, Some(233));
        assert_eq!(t.cache.unwrap().read, Some(10));
    }

    #[test]
    fn parse_part_data() {
        let p: PartData = serde_json::from_str(
            r#"{"type":"tool","tool":"grep","callID":"call_1","state":{"status":"completed","input":{"pattern":"x"},"output":"none","time":{"start":1,"end":2}}}"#,
        )
        .unwrap();
        assert_eq!(p.tool.as_deref(), Some("grep"));
        assert_eq!(p.call_id.as_deref(), Some("call_1"));
        let s = p.state.unwrap();
        assert_eq!(s.status.as_deref(), Some("completed"));
    }

    #[test]
    fn millis_conversion() {
        let t = from_millis(1783137427715).unwrap();
        assert_eq!(t.timestamp_millis(), 1783137427715);
        assert!(from_millis(-1).is_some());
    }

    #[test]
    fn session_model_parse() {
        let (m, p) = parse_session_model(Some(
            r#"{"id":"opencode-go/deepseek-v4-flash","providerID":"relay-opencode-go"}"#,
        ));
        assert_eq!(m.as_deref(), Some("opencode-go/deepseek-v4-flash"));
        assert_eq!(p.as_deref(), Some("relay-opencode-go"));
    }
}
