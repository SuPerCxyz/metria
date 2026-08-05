//! Node 与 Collector 模型。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{CollectorStatus, NodeStatus};
use super::ids::Id;

/// Node：运行 Metria Agent 容器的 Linux 宿主机。
///
/// Node ID 不能使用容器 ID；身份获取优先级：
/// 显式配置 METRIA_NODE_ID > 数据卷持久化 ID > 按 Node Name 生成。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub labels: Vec<String>,
    pub platform: Option<String>,
    pub architecture: Option<String>,
    pub timezone: Option<String>,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub status: NodeStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Collector：运行在 Node 上的 Metria Agent 容器实例。
///
/// 虽然通常一个 Node 只运行一个 Collector，但数据模型不强制一对一。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collector {
    pub id: Id,
    pub node_id: String,
    pub agent_version: String,
    pub protocol_version: u32,
    pub container_image: Option<String>,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub last_upload_at: Option<DateTime<Utc>>,
    pub status: CollectorStatus,
    pub spool_pending_events: i64,
    pub spool_size_bytes: i64,
    pub clock_skew_seconds: i64,
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
    fn node_serde() {
        let n = Node {
            id: "node-01".into(),
            name: "node-01".into(),
            description: None,
            labels: vec!["env:dev".into()],
            platform: Some("linux".into()),
            architecture: Some("x86_64".into()),
            timezone: Some("Asia/Shanghai".into()),
            first_seen_at: t(),
            last_seen_at: t(),
            status: NodeStatus::Online,
            created_at: t(),
            updated_at: t(),
        };
        let json = serde_json::to_string(&n).unwrap();
        let back: Node = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "node-01");
        assert_eq!(back.status, NodeStatus::Online);
    }

    #[test]
    fn collector_serde() {
        let c = Collector {
            id: Id::new(),
            node_id: "node-01".into(),
            agent_version: "0.1.0".into(),
            protocol_version: 1,
            container_image: None,
            started_at: t(),
            last_heartbeat_at: t(),
            last_upload_at: None,
            status: CollectorStatus::Online,
            spool_pending_events: 0,
            spool_size_bytes: 0,
            clock_skew_seconds: 0,
            created_at: t(),
            updated_at: t(),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: Collector = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent_version, "0.1.0");
    }
}
