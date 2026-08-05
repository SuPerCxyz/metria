//! Tool 事件与子 Agent 关系模型。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::{ContentHash, Id};

/// ToolEvent：一次 Tool 调用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolEvent {
    pub id: Id,
    pub session_id: Id,
    pub model_call_id: Option<Id>,
    pub turn_id: Option<Id>,
    pub source_tool_id: Option<String>,
    pub name: String,
    pub tool_type: String,
    pub status: String,
    pub input_content_hash: Option<ContentHash>,
    pub output_content_hash: Option<ContentHash>,
    pub input_length: i64,
    pub output_length: i64,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// SubagentRelation：父调用与子会话的关系。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentRelation {
    pub id: Id,
    pub session_id: Id,
    pub parent_model_call_id: Option<Id>,
    pub child_session_id: Id,
    /// spawned / task / continue 等
    pub relation: String,
    pub created_at: DateTime<Utc>,
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
    fn tool_event_serde() {
        let e = ToolEvent {
            id: Id::new(),
            session_id: Id::new(),
            model_call_id: None,
            turn_id: None,
            source_tool_id: Some("toolu_1".into()),
            name: "Read".into(),
            tool_type: "read_file".into(),
            status: "success".into(),
            input_content_hash: None,
            output_content_hash: None,
            input_length: 10,
            output_length: 0,
            started_at: t(),
            completed_at: None,
            duration_ms: None,
            error: None,
            created_at: t(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ToolEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Read");
    }
}
