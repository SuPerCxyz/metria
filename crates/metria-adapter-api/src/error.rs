//! Adapter 错误类型。
//!
//! 解析器必须容忍坏记录：警告 + continue，不因单条坏记录中断。
//! 此处错误用于结构性失败（路径不可读、数据库锁、Schema Drift 等）。

use metria_core::model::ContentHash;

/// Adapter 错误。
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("路径不存在: {0}")]
    PathNotFound(String),
    #[error("路径不可读: {path}: {source}")]
    NotReadable {
        path: String,
        source: std::io::Error,
    },
    #[error("数据库被锁定或忙: {0}")]
    DbLocked(String),
    #[error("Schema 漂移: {0}")]
    SchemaDrift(String),
    #[error("记录格式异常: {0}")]
    Malformed(String),
    #[error("仅部分读取成功（恢复 {recovered} 条，跳过 {skipped} 条）")]
    PartialRead { recovered: u64, skipped: u64 },
    #[error("游标失效: {0}")]
    CursorInvalid(String),
    #[error("来源指纹不匹配: expected {expected}")]
    FingerprintMismatch { expected: ContentHash },
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("配置错误: {0}")]
    Config(String),
    #[error("其他: {0}")]
    Other(String),
}

/// 内部结构：用于在扫描中累计警告与跳过计数，最终决定返回 PartialRead 或成功。
#[derive(Debug, Default)]
pub struct ScanTolerance {
    pub recovered: u64,
    pub skipped: u64,
    pub warnings: Vec<String>,
}

impl ScanTolerance {
    /// 记录一条坏记录（计数 + 警告）。
    pub fn record(&mut self, warning: String) {
        self.skipped += 1;
        self.warnings.push(warning);
    }

    pub fn is_clean(&self) -> bool {
        self.skipped == 0
    }
}
