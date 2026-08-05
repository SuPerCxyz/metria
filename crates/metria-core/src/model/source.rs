//! Client 与 Source 模型，以及扫描游标与来源错误。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::SourceStatus;
use super::ids::{ContentHash, Id};

/// Client：被监控的 AI 编程 Agent 工具（如 claude-code / codex / opencode）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Client {
    pub id: String,
    pub canonical_name: String,
    pub display_name: String,
    pub category: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Source：某 Node 上某 Client 的具体本地数据源。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: Id,
    pub node_id: String,
    pub collector_id: Id,
    pub client_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub source_fingerprint: String,
    pub source_path_hash: ContentHash,
    pub client_version: Option<String>,
    pub status: SourceStatus,
    pub capabilities: Vec<String>,
    pub last_scan_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// JSONL 数据源游标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonlCursor {
    pub canonical_path_hash: ContentHash,
    pub file_identity: String,
    pub inode: i64,
    pub size: i64,
    pub mtime: i64,
    pub byte_offset: i64,
    pub last_event_hash: Option<ContentHash>,
    pub last_scan_at: Option<DateTime<Utc>>,
}

/// SQLite 数据源游标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SqliteCursor {
    pub database_fingerprint: ContentHash,
    pub schema_version: Option<String>,
    pub table_name: String,
    pub last_rowid: i64,
    pub last_updated_at: Option<DateTime<Utc>>,
    pub last_primary_key: Option<String>,
    pub last_scan_at: Option<DateTime<Utc>>,
}

/// 游标：按数据源类型区分 JSONL / SQLite。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceCursor {
    Jsonl(JsonlCursor),
    Sqlite(SqliteCursor),
}

/// 来源错误（解析告警、Schema Drift 等）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceError {
    pub id: Id,
    pub source_id: Id,
    /// 错误发生阶段：discover / scan / parse / traffic / health
    pub phase: String,
    /// 严重程度：warning / error / fatal
    pub severity: String,
    /// 错误模式（稳定字符串，便于聚合）
    pub pattern: String,
    /// 该模式下累计出现的样本数
    pub sample_count: u64,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub last_message: String,
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
    fn cursor_serde_roundtrip() {
        let j = SourceCursor::Jsonl(JsonlCursor {
            canonical_path_hash: ContentHash::hash_str("/a/b.jsonl"),
            file_identity: "inode:123".into(),
            inode: 123,
            size: 1000,
            mtime: 100,
            byte_offset: 500,
            last_event_hash: None,
            last_scan_at: None,
        });
        let json = serde_json::to_string(&j).unwrap();
        assert!(json.contains("\"kind\":\"jsonl\""));
        let back: SourceCursor = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, SourceCursor::Jsonl(_)));
    }

    #[test]
    fn source_path_hash_not_raw() {
        let n = Source {
            id: Id::new(),
            node_id: "n".into(),
            collector_id: Id::new(),
            client_id: "claude-code".into(),
            adapter_id: "claude-code".into(),
            adapter_version: "0.1.0".into(),
            source_fingerprint: "fp".into(),
            source_path_hash: ContentHash::hash_str("/home/user/secret/project"),
            client_version: None,
            status: SourceStatus::Active,
            capabilities: vec![],
            last_scan_at: None,
            last_success_at: None,
            last_event_at: None,
            last_error: None,
            created_at: t(),
            updated_at: t(),
        };
        // 路径哈希不得包含原始路径
        let json = serde_json::to_string(&n).unwrap();
        assert!(!json.contains("/home/user"));
    }
}
