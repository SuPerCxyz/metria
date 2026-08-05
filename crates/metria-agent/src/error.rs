//! Agent 错误类型。

/// Agent 错误。
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("配置错误: {0}")]
    Config(#[from] metria_core::error::ConfigError),
    #[error("存储错误: {0}")]
    Storage(#[from] metria_storage::StorageError),
    #[error("Adapter 错误: {0}")]
    Adapter(#[from] metria_adapter_api::AdapterError),
    #[error("HTTP 错误: {0}")]
    Http(String),
    #[error("序列化错误: {0}")]
    Serde(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("内部错误: {0}")]
    Internal(String),
}

impl From<metria_storage::rusqlite::Error> for AgentError {
    fn from(e: metria_storage::rusqlite::Error) -> Self {
        AgentError::Storage(metria_storage::StorageError::Query(e.to_string()))
    }
}

pub type Result<T> = std::result::Result<T, AgentError>;
