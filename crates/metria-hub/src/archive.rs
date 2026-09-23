//! 明细归档：按保留期删除超期明细，聚合数据完整保留。
//!
//! 硬约束（spec `data-archival`）：
//! * **默认关闭**：未配置时不删除任何数据，保持「全量保留」承诺；
//! * **删除前强制备份**：备份失败即中止，一行都不删；
//! * **只删明细**：`hourly_rollups` / `daily_rollups` / `hourly_performance_rollups`
//!   完整保留，总览、趋势、用量报告与热力图在归档区间内数值不变；
//! * **分批有界**：单批固定行数、批间让出锁，归档期间读请求与健康检查照常；
//! * **可恢复**：删除以「保留期目标时间」为界，幂等，中断后下次继续，不会重复或跳过；
//! * **诚实标记**：归档水位持久化，列表接口回传 `archived_before`，界面据此显示
//!   「已归档，明细不可查」，不伪装成「无数据」。
//!
//! 已知瞬态与孤儿策略：删除按表顺序执行，窗口内跨表计数可能短暂不一致（只读、
//! 只影响在途请求的瞬时读数）；`usage_events` 按 `timestamp`、`traffic_estimates`
//! 按 `calculated_at` 删除，跨期边界可能残留少量孤儿行——方向是「少删不多删」，
//! 下一轮归档会随同批数据一起回收，不会产生错误数据。

use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

use chrono::{Duration, Utc};
use metria_storage::rusqlite::params;
use metria_storage::StorageError;
use tracing::info;

use crate::db::HubDb;

/// 归档开关；缺省即关闭。
pub const KEY_ARCHIVE_ENABLED: &str = "archive_enabled";
/// 归档保留天数。
pub const KEY_ARCHIVE_RETENTION_DAYS: &str = "archive_retention_days";
/// 归档水位：早于该时间的明细已删除。
pub const KEY_ARCHIVE_WATERMARK: &str = "archive_watermark";
pub const KEY_ARCHIVE_LAST_RUN_AT: &str = "archive_last_run_at";
pub const KEY_ARCHIVE_LAST_DELETED: &str = "archive_last_deleted";
pub const KEY_ARCHIVE_TOTAL_DELETED: &str = "archive_total_deleted";
/// 最近一次归档使用的备份文件，便于运维核对与恢复。
pub const KEY_ARCHIVE_BACKUP_PATH: &str = "archive_backup_path";
/// 归档累计释放的磁盘字节数（按页数差计量，需 auto_vacuum 生效）。
pub const KEY_ARCHIVE_FREED_BYTES: &str = "archive_freed_bytes";

/// 默认保留天数：一年。仅作为配置缺省值，实际以 settings 为准。
pub const DEFAULT_RETENTION_DAYS: i64 = 365;
/// 单批删除行数，保证单个事务有界、不长时间持锁。
const DELETE_BATCH: i64 = 50_000;
/// 批与批之间的让出时长，让上传与查询穿插执行。
const BATCH_PAUSE_MS: u64 = 20;

/// 归档策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivePolicy {
    pub enabled: bool,
    pub retention_days: i64,
}

impl Default for ArchivePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            retention_days: DEFAULT_RETENTION_DAYS,
        }
    }
}

/// 一次归档执行结果。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ArchiveReport {
    /// 是否因未启用或无需归档而跳过。
    pub skipped: bool,
    /// 本次删除的明细行数。
    pub deleted: i64,
    /// 归档后写入的水位（早于该时间的明细已删除）。
    pub watermark: Option<String>,
    /// 本次生成的备份文件路径。
    pub backup_path: Option<String>,
}

/// 读取归档策略；缺省为关闭。
pub fn policy(db: &HubDb) -> ArchivePolicy {
    let enabled = db
        .setting_get(KEY_ARCHIVE_ENABLED)
        .ok()
        .flatten()
        .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"));
    let retention_days = db
        .setting_get(KEY_ARCHIVE_RETENTION_DAYS)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|days| *days >= 1)
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    ArchivePolicy {
        enabled,
        retention_days,
    }
}

/// 保存归档策略。保留天数必须 ≥1，否则拒绝，避免误配置成「立刻删光」。
pub fn set_policy(
    db: &HubDb,
    enabled: bool,
    retention_days: i64,
) -> Result<ArchivePolicy, StorageError> {
    if retention_days < 1 {
        return Err(StorageError::Query(
            "归档保留天数必须大于等于 1".to_string(),
        ));
    }
    db.settings_set_many(&[
        (
            KEY_ARCHIVE_ENABLED,
            if enabled { "1" } else { "0" }.to_string(),
        ),
        (KEY_ARCHIVE_RETENTION_DAYS, retention_days.to_string()),
    ])?;
    Ok(ArchivePolicy {
        enabled,
        retention_days,
    })
}

/// 归档水位：早于该 RFC3339 时间的明细已被删除。
pub fn watermark(db: &HubDb) -> Option<String> {
    db.setting_get(KEY_ARCHIVE_WATERMARK).ok().flatten()
}

/// 当前最早的可查明细时间（`model_calls.started_at` 最小值）。
pub fn earliest_detail(db: &HubDb) -> Option<String> {
    let c = db.conn();
    c.query_row("SELECT MIN(started_at) FROM model_calls", [], |r| {
        r.get::<_, Option<String>>(0)
    })
    .ok()
    .flatten()
}

/// 归档状态（系统信息与设置页共用）。
pub fn status(db: &HubDb) -> serde_json::Value {
    let policy = policy(db);
    let get_str = |key: &str| db.setting_get(key).ok().flatten();
    let get_int = |key: &str| get_str(key).and_then(|v| v.trim().parse::<i64>().ok());
    serde_json::json!({
        "enabled": policy.enabled,
        "retention_days": policy.retention_days,
        "watermark": get_str(KEY_ARCHIVE_WATERMARK),
        "earliest_detail": earliest_detail(db),
        "last_run_at": get_str(KEY_ARCHIVE_LAST_RUN_AT),
        "last_deleted": get_int(KEY_ARCHIVE_LAST_DELETED),
        "total_deleted": get_int(KEY_ARCHIVE_TOTAL_DELETED),
        "backup_path": get_str(KEY_ARCHIVE_BACKUP_PATH),
        "freed_bytes": get_int(KEY_ARCHIVE_FREED_BYTES),
        "label": if policy.enabled {
            format!("已启用：保留最近 {} 天明细，删除前自动备份", policy.retention_days)
        } else {
            "全量保留（未启用自动清理）".to_string()
        },
    })
}

/// 执行一次归档：备份 → 分批删除 → 记录水位。
///
/// 任何一步失败都返回 `Err`：备份失败时一行都不会删，删除中途失败时已删部分保持
/// 幂等（下次按同一目标时间继续），并保留水位供界面诚实展示。
pub fn run(db: &HubDb) -> Result<ArchiveReport, StorageError> {
    let policy = policy(db);
    if !policy.enabled {
        return Ok(ArchiveReport {
            skipped: true,
            ..Default::default()
        });
    }
    let target = (Utc::now() - Duration::days(policy.retention_days)).to_rfc3339();
    let Some(earliest) = earliest_detail(db) else {
        return Ok(ArchiveReport {
            skipped: true,
            ..Default::default()
        });
    };
    if earliest >= target {
        // 没有超期明细：不动库，也不重复生成备份
        return Ok(ArchiveReport {
            skipped: true,
            watermark: watermark(db).or(Some(target)),
            ..Default::default()
        });
    }

    // 页数快照：删除后按页数差计量真实释放的磁盘空间
    let (pages_before, page_size) = page_usage(db);

    // 1) 强制备份：失败即中止，绝不进入删除阶段
    let backup_path = backup_snapshot(db)?;

    // 2) 分批删除超期明细（聚合表一律不动）
    let mut deleted = 0i64;
    for (table, column) in detail_tables() {
        loop {
            let batch = delete_batch(db, table, column, &target)?;
            if batch == 0 {
                break;
            }
            deleted += batch;
            // 批间让出连接锁，上传与查询才能穿插执行
            std::thread::sleep(std::time::Duration::from_millis(BATCH_PAUSE_MS));
        }
    }
    // 3) 会话：只删已无任何剩余明细引用的超期会话，避免留下指向空会话的调用
    deleted += delete_orphan_sessions(db, &target)?;

    // 4) 计价匹配：用量行归档后其匹配记录成为孤儿，一并回收
    deleted += delete_orphan_pricing_matches(db)?;

    // 4) 回收空闲页并按页数差计量释放空间（auto_vacuum=INCREMENTAL 时才有效，
    //    否则如实记 0，不虚报“已释放”）
    let freed_bytes = reclaim_and_measure(db, pages_before, page_size);
    let freed_total = db
        .setting_get(KEY_ARCHIVE_FREED_BYTES)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .saturating_add(freed_bytes);

    let now = Utc::now().to_rfc3339();
    let total = db
        .setting_get(KEY_ARCHIVE_TOTAL_DELETED)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .saturating_add(deleted);
    db.settings_set_many(&[
        (KEY_ARCHIVE_WATERMARK, target.clone()),
        (KEY_ARCHIVE_LAST_RUN_AT, now),
        (KEY_ARCHIVE_LAST_DELETED, deleted.to_string()),
        (KEY_ARCHIVE_TOTAL_DELETED, total.to_string()),
        (KEY_ARCHIVE_BACKUP_PATH, backup_path.clone()),
        (KEY_ARCHIVE_FREED_BYTES, freed_total.to_string()),
    ])?;
    info!(deleted, watermark = %target, backup = %backup_path, "明细归档完成");

    Ok(ArchiveReport {
        skipped: false,
        deleted,
        watermark: Some(target),
        backup_path: Some(backup_path),
    })
}

/// 当前库的页数与页大小。
fn page_usage(db: &HubDb) -> (i64, i64) {
    let c = db.conn();
    let pages = c
        .query_row("PRAGMA page_count", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0);
    let size = c
        .query_row("PRAGMA page_size", [], |r| r.get::<_, i64>(0))
        .unwrap_or(4096);
    (pages, size)
}

/// 回收空闲页并返回「实占空间」相对归档前减少的字节数。
///
/// `auto_vacuum=INCREMENTAL` 下删除只会把页挂到空闲链表，必须显式执行
/// `incremental_vacuum` 才会把文件尾部的空闲页归还给文件系统。
///
/// 只看 `page_count` 是否下降会严重低估：碎片化的空闲页夹在存活页之间时，
/// 文件大小几乎不变（实测删掉 178 万行只量到 4KiB），但这些页已经可以被复用。
/// 因此按「归档前实占页 - 归档后实占页」计量，其中实占页 = page_count - freelist。
fn reclaim_and_measure(db: &HubDb, pages_before: i64, page_size: i64) -> i64 {
    if db.incremental_vacuum().is_err() {
        return 0;
    }
    let (pages_after, free_after) = {
        let c = db.conn();
        let pages = c
            .query_row("PRAGMA page_count", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0);
        let free = c
            .query_row("PRAGMA freelist_count", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0);
        (pages, free)
    };
    if page_size <= 0 {
        return 0;
    }
    let live_after = (pages_after - free_after).max(0);
    (pages_before - live_after).max(0) * page_size
}

/// 归档涉及的明细表与其时间列（全部为已建索引列，避免全表扫描）。
fn detail_tables() -> &'static [(&'static str, &'static str)] {
    &[
        ("model_calls", "started_at"),
        ("usage_events", "timestamp"),
        ("messages", "created_at"),
        ("tool_events", "started_at"),
        ("traffic_estimates", "calculated_at"),
    ]
}

/// 单批删除：`rowid IN (SELECT ... LIMIT n)`，事务小、可中断、可续跑。
fn delete_batch(db: &HubDb, table: &str, column: &str, target: &str) -> Result<i64, StorageError> {
    let c = db.conn();
    let sql = format!(
        "DELETE FROM {table} WHERE rowid IN (
             SELECT rowid FROM {table} WHERE {column} < ?1 LIMIT {DELETE_BATCH}
         )"
    );
    let removed = c.execute(&sql, params![target])?;
    Ok(removed as i64)
}

/// 删除已没有任何剩余明细引用的超期会话。
fn delete_orphan_sessions(db: &HubDb, target: &str) -> Result<i64, StorageError> {
    // 分批执行：单条语句删几十万行会长时间持有连接锁，把同期 API 卡住。
    let mut total = 0i64;
    loop {
        let removed = {
            let c = db.conn();
            c.execute(
                "DELETE FROM sessions WHERE rowid IN (
                     SELECT rowid FROM sessions
                     WHERE started_at < ?1
                       AND NOT EXISTS (SELECT 1 FROM model_calls m WHERE m.session_id = sessions.id)
                       AND NOT EXISTS (SELECT 1 FROM messages ms WHERE ms.session_id = sessions.id)
                       AND NOT EXISTS (SELECT 1 FROM tool_events t WHERE t.session_id = sessions.id)
                     LIMIT ?2)",
                params![target, DELETE_BATCH],
            )?
        } as i64;
        if removed == 0 {
            break;
        }
        total += removed;
        std::thread::sleep(std::time::Duration::from_millis(BATCH_PAUSE_MS));
    }
    Ok(total)
}

/// 删除孤儿计价匹配（对应用量行已归档），保留仍在库内的每个用量行的最新一条。
fn delete_orphan_pricing_matches(db: &HubDb) -> Result<i64, StorageError> {
    // 同样分批：孤儿计价记录可能有数十万行，单条语句会霸占连接锁。
    let mut total = 0i64;
    loop {
        let removed = {
            let c = db.conn();
            c.execute(
                "DELETE FROM pricing_matches WHERE id IN (
                     SELECT p.id FROM pricing_matches p
                     WHERE NOT EXISTS (SELECT 1 FROM usage_events u WHERE u.event_id = p.usage_event_id)
                     LIMIT ?1)",
                params![DELETE_BATCH],
            )?
        } as i64;
        if removed == 0 {
            break;
        }
        total += removed;
        std::thread::sleep(std::time::Duration::from_millis(BATCH_PAUSE_MS));
    }
    Ok(total)
}

/// 归档前强制备份：`VACUUM INTO` 生成一致性快照后流式压缩到数据目录。
///
/// 压缩按块进行，不把整个库读进内存（CLI 版 backup 是整文件 `read`，在 1GB+ 的库上
/// 会造成上百 MB 的瞬时 RSS 尖峰，Hub 内不能这么做）。
fn backup_snapshot(db: &HubDb) -> Result<String, StorageError> {
    let (dir, stamp) = {
        let c = db.conn();
        let file: Option<String> = c
            .query_row("PRAGMA database_list", [], |r| {
                r.get::<_, Option<String>>(2)
            })
            .map_err(StorageError::from)?;
        let dir = file
            .and_then(|f| PathBuf::from(f).parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(std::env::temp_dir);
        let stamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
        (dir, stamp)
    };
    std::fs::create_dir_all(&dir)?;
    let snapshot = dir.join(format!("archive-snapshot-{stamp}.db"));
    let output = dir.join(format!("archive-before-{stamp}.db.zst"));

    // 用独立连接做 VACUUM INTO：快照只需读源库，不必占用 Hub 的全局连接锁。
    // 若共用全局锁，1GB+ 库的快照会把同期所有 API 请求卡住十几秒，
    // 违背 spec「归档期间服务可用」的要求。
    {
        let db_file = {
            let c = db.conn();
            c.query_row("PRAGMA database_list", [], |r| {
                r.get::<_, Option<String>>(2)
            })
            .map_err(StorageError::from)?
            .ok_or_else(|| StorageError::Query("无法定位数据库文件".into()))?
        };
        let snapshot_conn =
            metria_storage::rusqlite::Connection::open(&db_file).map_err(StorageError::from)?;
        snapshot_conn
            .busy_timeout(std::time::Duration::from_millis(5_000))
            .map_err(StorageError::from)?;
        // 文件名作为绑定参数传入，避免手工拼接 SQL
        snapshot_conn
            .execute("VACUUM INTO ?1", [&snapshot.to_string_lossy().to_string()])
            .map_err(|e| StorageError::Query(format!("归档前备份失败: {e}")))?;
    }

    let compress = || -> std::io::Result<()> {
        let input = BufReader::new(std::fs::File::open(&snapshot)?);
        let out = BufWriter::new(std::fs::File::create(&output)?);
        zstd::stream::copy_encode(input, out, 3)?;
        Ok(())
    };
    let result = compress();
    let _ = std::fs::remove_file(&snapshot);
    result?;

    Ok(output.to_string_lossy().to_string())
}
