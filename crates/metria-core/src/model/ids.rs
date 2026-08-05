//! 标识类型：节点/采集器/来源/会话等 ID 使用 ULID（时间有序），事件 ID 使用 blake3 内容哈希。

use serde::{Deserialize, Serialize};

use crate::error::ModelError;

/// 通用实体 ID：ULID 字符串（26 位 Crockford Base32，时间有序）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    /// 生成新的时间有序 ID。
    pub fn new() -> Self {
        Self(ulid::Ulid::new().to_string())
    }

    /// 从字符串构造；不合法时返回错误。
    pub fn parse(s: &str) -> Result<Self, ModelError> {
        if s.is_empty() || s.len() > 128 {
            return Err(ModelError::InvalidId(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 事件 ID：`blake3:` 前缀 + blake3 内容哈希的十六进制。
///
/// 相同内容的重复记录生成相同 Event ID，用于幂等去重。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(String);

impl EventId {
    pub const PREFIX: &'static str = "blake3:";

    /// 由字节内容生成事件 ID。
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let hash = blake3::hash(bytes);
        Self(format!("{}{}", Self::PREFIX, hash.to_hex()))
    }

    /// 由字符串内容生成事件 ID。
    pub fn from_content(content: &str) -> Self {
        Self::from_bytes(content.as_bytes())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 由规范化 JSON 生成：对 serde_json 序列化结果做 blake3。
    pub fn from_json(value: &serde_json::Value) -> Self {
        Self::from_content(&value.to_string())
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 内容哈希：blake3 十六进制（无前缀），用于路径、内容去重与校验。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentHash(String);

impl ContentHash {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }

    pub fn hash_str(s: &str) -> Self {
        Self::from_bytes(s.as_bytes())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let a = Id::new();
        let b = Id::new();
        assert_ne!(a, b);
        assert_eq!(a.as_str().len(), 26);
    }

    #[test]
    fn event_id_stable_and_deterministic() {
        let a = EventId::from_content("same");
        let b = EventId::from_content("same");
        let c = EventId::from_content("different");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.as_str().starts_with("blake3:"));
    }

    #[test]
    fn content_hash_stable() {
        assert_eq!(ContentHash::hash_str("x").as_str().len(), 64);
        assert_eq!(ContentHash::hash_str("x"), ContentHash::hash_str("x"));
    }
}
