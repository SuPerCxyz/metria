//! Project 模型：默认只保存路径哈希，禁止默认上传完整绝对路径。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::ContentHash;

/// Project：用户在某个节点上的工作项目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    /// 规范化键（如路径哈希或显式 key），用于跨节点合并
    pub canonical_key: String,
    pub display_name: Option<String>,
    pub path_hash: ContentHash,
    pub git_remote_hash: Option<ContentHash>,
    pub metadata: serde_json::Value,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
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
    fn project_never_contains_raw_path() {
        let p = Project {
            id: "p".into(),
            canonical_key: ContentHash::hash_str("/home/user/projects/nexora")
                .as_str()
                .to_string(),
            display_name: None,
            path_hash: ContentHash::hash_str("/home/user/projects/nexora"),
            git_remote_hash: None,
            metadata: serde_json::json!({}),
            first_seen_at: t(),
            last_seen_at: t(),
            created_at: t(),
            updated_at: t(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            !json.contains("nexora") || !json.contains("/home/user"),
            "禁止默认上传完整路径"
        );
    }
}
