//! Repository 抽象骨架。
//!
//! M1 阶段 Hub 使用 SQLite；本 trait 界定存储层与领域层边界，
//! 为未来切换 PostgreSQL 等实现预留接口，避免上层依赖具体存储实现。

use crate::error::Result;

/// 存储层通用能力。
pub trait Repository: Send + Sync {
    /// 返回是否就绪（连接可用、迁移已应用）。
    fn is_ready(&self) -> bool;

    /// 返回当前 schema 版本。
    fn schema_version(&self) -> Result<i64>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noop;

    impl Repository for Noop {
        fn is_ready(&self) -> bool {
            true
        }
        fn schema_version(&self) -> Result<i64> {
            Ok(0)
        }
    }

    #[test]
    fn repository_contract() {
        let r = Noop;
        assert!(r.is_ready());
        assert_eq!(r.schema_version().unwrap(), 0);
    }
}
