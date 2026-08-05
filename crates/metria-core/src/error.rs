//! Metria core 基础错误类型。
//!
//! 各 crate 保留自身错误类型（见 storage/protocol/adapter-api 等），
//! 此处定义跨 crate 共享的基础错误，避免循环依赖。

use std::path::PathBuf;

/// 配置错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("缺少必需配置项 `{0}`")]
    Missing(String),
    #[error("配置项 `{name}` 非法: {message}")]
    Invalid { name: String, message: String },
    #[error("配置文件读取失败: {path:?}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("配置文件解析失败: {path:?}: {message}")]
    Parse { path: PathBuf, message: String },
}

/// 领域模型错误（值非法、归一化失败等）。
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("字段 `{field}` 数值非法: {message}")]
    InvalidNumber {
        field: &'static str,
        message: String,
    },
    #[error("标识 `{0}` 非法")]
    InvalidId(String),
    #[error("时间非法: {0}")]
    InvalidTime(String),
    #[error("归一化失败: {0}")]
    Normalize(String),
}

/// 时间与时区错误。
#[derive(Debug, thiserror::Error)]
pub enum TimeError {
    #[error("时区 `{0}` 无法解析")]
    InvalidTimezone(String),
    #[error("时间范围非法: 开始时间晚于结束时间")]
    InvalidRange,
    #[error("时间值超出范围: {0}")]
    OutOfRange(String),
}

/// 金额（微美元）错误。
#[derive(Debug, thiserror::Error)]
pub enum MoneyError {
    #[error("金额为负: {0}")]
    Negative(i64),
    #[error("金额计算溢出: {0}")]
    Overflow(String),
}

/// 脱敏 / 隐私处理错误。
#[derive(Debug, thiserror::Error)]
pub enum PrivacyError {
    #[error("无法生成哈希: {0}")]
    Hash(String),
}

/// 内容分类错误。
#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error("内容字节统计失败: {0}")]
    Bytes(String),
}
