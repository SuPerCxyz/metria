//! metria-storage: SQLite 连接、迁移框架与 Repository 抽象。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod db;
pub mod error;
pub mod migrations;
pub mod repository;

pub use db::{open, open_readonly, quick_check, wal_checkpoint, DbOptions, Synchronous};
pub use error::{Result, StorageError};
pub use migrations::{load_embedded, migrate, migrate_embedded, Migration};
pub use repository::Repository;

/// 重导出 rusqlite，避免依赖方重复引入同一版本。
pub use rusqlite;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
