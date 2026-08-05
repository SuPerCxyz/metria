//! SQLite 连接打开与 PRAGMA 配置。
//!
//! 约定：WAL、foreign_keys=ON、busy_timeout、合理的 synchronous，定期 checkpoint。

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::error::{Result, StorageError};

/// SQLite 打开选项。
#[derive(Debug, Clone)]
pub struct DbOptions {
    /// busy_timeout（毫秒），默认 5000。
    pub busy_timeout_ms: u64,
    /// 是否启用 WAL 日志模式，默认 true。
    pub wal: bool,
    /// 是否启用外键约束，默认 true。
    pub foreign_keys: bool,
    /// synchronous 级别，默认 Normal。
    pub synchronous: Synchronous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Synchronous {
    Off,
    Normal,
    Full,
    Extra,
}

impl Default for DbOptions {
    fn default() -> Self {
        Self {
            busy_timeout_ms: 5000,
            wal: true,
            foreign_keys: true,
            synchronous: Synchronous::Normal,
        }
    }
}

/// 打开（或创建）一个可写数据库连接，并应用约定的 PRAGMA。
pub fn open(path: &Path, opts: &DbOptions) -> Result<Connection> {
    let conn = Connection::open(path).map_err(|e| StorageError::Open(e.to_string()))?;
    configure(&conn, opts)?;
    Ok(conn)
}

/// 以只读方式打开数据库（用于第三方数据源，如 OpenCode SQLite）。
///
/// 不修改任何 PRAGMA 持久状态；`query_only` 属于连接级设置，确保连接上无法写入。
pub fn open_readonly(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| StorageError::Open(e.to_string()))?;
    conn.busy_timeout(Duration::from_millis(2000))
        .map_err(|e| StorageError::Open(e.to_string()))?;
    // query_only 是连接级只读保障，不写回数据库文件，不改变第三方库状态。
    conn.pragma_update(None, "query_only", true)
        .map_err(|e| StorageError::Open(e.to_string()))?;
    Ok(conn)
}

/// 对连接应用约定 PRAGMA（只影响本连接与本次会话）。
pub fn configure(conn: &Connection, opts: &DbOptions) -> Result<()> {
    conn.busy_timeout(Duration::from_millis(opts.busy_timeout_ms))
        .map_err(|e| StorageError::Open(e.to_string()))?;
    if opts.wal {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| StorageError::Open(e.to_string()))?;
    }
    let fk: i64 = if opts.foreign_keys { 1 } else { 0 };
    conn.pragma_update(None, "foreign_keys", fk)
        .map_err(|e| StorageError::Open(e.to_string()))?;
    let sync = match opts.synchronous {
        Synchronous::Off => "OFF",
        Synchronous::Normal => "NORMAL",
        Synchronous::Full => "FULL",
        Synchronous::Extra => "EXTRA",
    };
    conn.pragma_update(None, "synchronous", sync)
        .map_err(|e| StorageError::Open(e.to_string()))?;
    conn.pragma_update(None, "journal_size_limit", 64 * 1024 * 1024)
        .map_err(|e| StorageError::Open(e.to_string()))?;
    Ok(())
}

/// 执行 `PRAGMA quick_check` 完整性检查。
pub fn quick_check(conn: &Connection) -> Result<()> {
    let mut stmt = conn
        .prepare("PRAGMA quick_check")
        .map_err(|e| StorageError::Integrity(e.to_string()))?;
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .map_err(|e| StorageError::Integrity(e.to_string()))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| StorageError::Integrity(e.to_string()))?;
    if rows.is_empty() || rows.iter().all(|s| s == "ok") {
        Ok(())
    } else {
        Err(StorageError::Integrity(rows.join(", ")))
    }
}

/// 执行 WAL checkpoint，回收 WAL 文件。
pub fn wal_checkpoint(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .map_err(|e| StorageError::Query(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_pragmas() {
        let dir = std::env::temp_dir().join(format!("metria-storage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let conn = open(&path, &DbOptions::default()).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn readonly_rejects_write() {
        let dir = std::env::temp_dir().join(format!("metria-storage-ro-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ro.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE t(id INTEGER)").unwrap();
        }
        let conn = open_readonly(&path).unwrap();
        let err = conn
            .execute("INSERT INTO t(id) VALUES (1)", [])
            .unwrap_err();
        assert!(
            err.to_string().contains("readonly") || err.to_string().contains("ReadOnly"),
            "expected readonly error, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
