//! metria-storage 错误类型。

/// 存储层错误。
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("SQLite 打开失败: {0}")]
    Open(String),
    #[error("SQLite 操作失败: {0}")]
    Query(String),
    #[error("迁移失败: {0}")]
    Migrate(String),
    #[error("完整性检查失败: {0}")]
    Integrity(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("模型序列化/反序列化失败: {0}")]
    Serde(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        StorageError::Query(e.to_string())
    }
}
