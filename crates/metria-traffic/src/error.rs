//! metria-traffic 错误类型。

/// 流量估算错误。
#[derive(Debug, thiserror::Error)]
pub enum TrafficError {
    #[error("Traffic Profile 非法: {0}")]
    InvalidProfile(String),
    #[error("估算输入非法: {0}")]
    InvalidInput(String),
    #[error("金额溢出: {0}")]
    Overflow(String),
}

pub type Result<T> = std::result::Result<T, TrafficError>;
