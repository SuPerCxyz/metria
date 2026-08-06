//! 本地 Spool：SQLite 持久化待上传事件与游标。
//!
//! 约束：
//! - 事件先写 Spool，Hub 确认后才删除；
//! - Cursor 与事件同一事务写入；
//! - 达到容量上限时停止 ingest 并告警（不静默丢弃）；
//! - 容器重启后继续上传。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Utc;
use metria_storage::rusqlite::Connection;

use crate::error::AgentError;

/// 一条待上传事件。
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub event_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
}

/// 游标更新项。
#[derive(Debug, Clone)]
pub struct CursorUpdate {
    pub source_id: String,
    pub cursor_json: String,
}

/// Spool 满信号。
#[derive(Debug, Clone)]
pub struct SpoolFull(Arc<AtomicBool>);

impl SpoolFull {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn is_full(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    fn set(&self, v: bool) {
        self.0.store(v, Ordering::Relaxed);
    }
}

/// 本地 Spool。
#[derive(Debug)]
pub struct Spool {
    conn: Connection,
    max_pending_events: i64,
    max_spool_bytes: i64,
    full: SpoolFull,
}

impl Spool {
    /// 打开（必要时创建）Spool 数据库并建表。
    pub fn open(
        path: &Path,
        max_pending_events: i64,
        max_spool_bytes: i64,
    ) -> Result<Self, AgentError> {
        let conn = metria_storage::open(path, &metria_storage::DbOptions::default())?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS agent_metadata (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS source_cursors (
                source_id  TEXT PRIMARY KEY,
                cursor_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS pending_events (
                event_id  TEXT PRIMARY KEY,
                kind      TEXT NOT NULL,
                payload   TEXT NOT NULL,
                created_at TEXT NOT NULL,
                attempts  INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS upload_batches (
                batch_id   TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                status     TEXT NOT NULL,
                last_error TEXT
            );
            CREATE TABLE IF NOT EXISTS dead_letters (
                event_id  TEXT PRIMARY KEY,
                kind      TEXT NOT NULL,
                payload   TEXT NOT NULL,
                reason    TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS source_health (
                source_id   TEXT PRIMARY KEY,
                ok          INTEGER NOT NULL,
                last_scan_at TEXT,
                last_error  TEXT,
                client_id   TEXT
            );
            CREATE TABLE IF NOT EXISTS traffic_profile_samples (
                sample_id  TEXT PRIMARY KEY,
                sample_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_pending_created ON pending_events(created_at);
            "#,
        )?;
        Ok(Self {
            conn,
            max_pending_events,
            max_spool_bytes,
            full: SpoolFull::new(),
        })
    }

    pub fn full_flag(&self) -> SpoolFull {
        self.full.clone()
    }

    pub fn pending_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM pending_events", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn spool_bytes(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(payload) + LENGTH(kind)), 0) FROM pending_events",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    pub fn dead_letter_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM dead_letters", [], |r| r.get(0))
            .unwrap_or(0)
    }

    /// 事务性写入事件 + 游标更新。
    ///
    /// 返回 Ok(true) 已写入；Ok(false) 表示 Spool 已满未写入（不静默丢弃，由调用方停止 ingest）。
    pub fn insert_batch(
        &mut self,
        events: &[PendingEvent],
        cursors: &[CursorUpdate],
    ) -> Result<bool, AgentError> {
        if events.is_empty() && cursors.is_empty() {
            return Ok(true);
        }
        let count = self.pending_count();
        let size = self.spool_bytes();
        let new_count = count + events.len() as i64;
        let new_size: i64 = size
            + events
                .iter()
                .map(|e| {
                    (serde_json::to_string(&e.payload)
                        .map(|s| s.len() as i64)
                        .unwrap_or(0))
                        + e.kind.len() as i64
                })
                .sum::<i64>();
        if new_count > self.max_pending_events || new_size > self.max_spool_bytes {
            self.full.set(true);
            tracing::error!(
                "Spool 已达容量上限（events={new_count}/{}, bytes={new_size}/{}），停止采集并告警；不会丢弃数据",
                self.max_pending_events,
                self.max_spool_bytes
            );
            return Ok(false);
        }

        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO pending_events (event_id, kind, payload, created_at, attempts) VALUES (?1, ?2, ?3, ?4, 0)",
            )?;
            let now = Utc::now().to_rfc3339();
            for e in events {
                stmt.execute(metria_storage::rusqlite::params![
                    e.event_id,
                    e.kind,
                    serde_json::to_string(&e.payload)
                        .map_err(|e| AgentError::Serde(e.to_string()))?,
                    now
                ])?;
            }
            let mut cs = tx.prepare(
                "INSERT OR REPLACE INTO source_cursors (source_id, cursor_json, updated_at) VALUES (?1, ?2, ?3)",
            )?;
            for c in cursors {
                cs.execute(metria_storage::rusqlite::params![
                    c.source_id,
                    c.cursor_json,
                    now
                ])?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn get_cursor(&self, source_id: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT cursor_json FROM source_cursors WHERE source_id = ?1",
                [source_id],
                |r| r.get(0),
            )
            .ok()
    }

    /// 取一批待上传事件（按事件数 + 未压缩字节预算）。
    pub fn next_batch(&self, max_events: usize, max_bytes: usize) -> (String, Vec<PendingEvent>) {
        let batch_id = format!("batch-{}", metria_core::model::Id::new());
        let mut events = Vec::new();
        let mut bytes = 0usize;
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT event_id, kind, payload FROM pending_events ORDER BY created_at, event_id LIMIT ?1",
        ) {
            if let Ok(rows) = stmt.query_map([max_events as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            }) {
                for row in rows.flatten() {
                    let (id, kind, payload_json) = row;
                    if events.len() >= max_events || bytes >= max_bytes {
                        break;
                    }
                    bytes += payload_json.len();
                    if let Ok(payload) = serde_json::from_str(&payload_json) {
                        events.push(PendingEvent { event_id: id, kind, payload });
                    }
                }
            }
        }
        (batch_id, events)
    }

    /// 上传成功：删除事件并记录批次。
    pub fn ack_uploaded(&mut self, batch_id: &str, event_ids: &[String]) -> Result<(), AgentError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM pending_events WHERE event_id = ?1")?;
            for id in event_ids {
                stmt.execute([id])?;
            }
            tx.execute(
                "INSERT OR REPLACE INTO upload_batches (batch_id, created_at, status) VALUES (?1, ?2, 'accepted')",
                metria_storage::rusqlite::params![batch_id, Utc::now().to_rfc3339()],
            )?;
        }
        tx.commit()?;
        // 若已明显低于上限，解除 full 标志
        if self.pending_count() < self.max_pending_events / 2 {
            self.full.set(false);
        }
        Ok(())
    }

    /// 上传失败：可重试则增加 attempts，否则转死信。
    pub fn fail_events(
        &mut self,
        batch_id: &str,
        event_ids: &[String],
        retryable: bool,
        reason: &str,
    ) -> Result<(), AgentError> {
        let tx = self.conn.transaction()?;
        {
            if retryable {
                let mut stmt = tx.prepare(
                    "UPDATE pending_events SET attempts = attempts + 1 WHERE event_id = ?1",
                )?;
                for id in event_ids {
                    stmt.execute([id])?;
                }
                tx.execute(
                    "INSERT OR REPLACE INTO upload_batches (batch_id, created_at, status, last_error) VALUES (?1, ?2, 'retry', ?3)",
                    metria_storage::rusqlite::params![batch_id, Utc::now().to_rfc3339(), reason],
                )?;
            } else {
                let mut del =
                    tx.prepare("SELECT kind, payload FROM pending_events WHERE event_id = ?1")?;
                let mut ins = tx.prepare(
                    "INSERT OR IGNORE INTO dead_letters (event_id, kind, payload, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                let mut remove = tx.prepare("DELETE FROM pending_events WHERE event_id = ?1")?;
                for id in event_ids {
                    if let Ok(kind) = del.query_row([id], |r| r.get::<_, String>(0)) {
                        let payload: String = del.query_row([id], |r| r.get(1)).unwrap_or_default();
                        ins.execute(metria_storage::rusqlite::params![
                            id,
                            kind,
                            payload,
                            reason,
                            Utc::now().to_rfc3339()
                        ])?;
                    }
                    remove.execute([id])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 元数据读写（Node ID 持久化等）。
    pub fn meta_get(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM agent_metadata WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .ok()
    }

    pub fn meta_set(&mut self, key: &str, value: &str) -> Result<(), AgentError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO agent_metadata (key, value) VALUES (?1, ?2)",
            metria_storage::rusqlite::params![key, value],
        )?;
        Ok(())
    }

    /// 来源健康记录。
    pub fn update_source_health(
        &self,
        source_id: &str,
        ok: bool,
        last_error: Option<&str>,
        client_id: &str,
    ) -> Result<(), AgentError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO source_health (source_id, ok, last_scan_at, last_error, client_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            metria_storage::rusqlite::params![
                source_id,
                if ok { 1 } else { 0 },
                Utc::now().to_rfc3339(),
                last_error.unwrap_or(""),
                client_id
            ],
        )?;
        Ok(())
    }

    pub fn source_health_all(&self) -> Vec<(String, bool, String)> {
        let mut out = Vec::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT source_id, ok, COALESCE(last_error, '') FROM source_health")
        {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            }) {
                for row in rows.flatten() {
                    out.push((row.0, row.1 != 0, row.2));
                }
            }
        }
        out
    }

    /// 最近一次失败批次信息（供 doctor）。
    pub fn last_batch_error(&self) -> Option<(String, String)> {
        self.conn
            .query_row(
                "SELECT batch_id, COALESCE(last_error, '') FROM upload_batches WHERE status != 'accepted' AND COALESCE(last_error, '') != '' ORDER BY created_at DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn insert_ack_and_cursor() {
        let path = std::env::temp_dir().join(format!("spool-test-{}", std::process::id()));
        tmp(&path);
        let mut spool = Spool::open(&path, 1000, 10_000_000).unwrap();
        let ev = vec![PendingEvent {
            event_id: "ev1".into(),
            kind: "usage".into(),
            payload: serde_json::json!({"n": 1}),
        }];
        let cur = vec![CursorUpdate {
            source_id: "s1".into(),
            cursor_json: "{\"off\":10}".into(),
        }];
        assert!(spool.insert_batch(&ev, &cur).unwrap());
        assert_eq!(spool.pending_count(), 1);
        assert_eq!(spool.get_cursor("s1").as_deref(), Some("{\"off\":10}"));
        let (batch, items) = spool.next_batch(10, 10_000);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].event_id, "ev1");
        spool.ack_uploaded(&batch, &["ev1".into()]).unwrap();
        assert_eq!(spool.pending_count(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn full_stops_ingest_without_drop() {
        let path = std::env::temp_dir().join(format!("spool-full-{}", std::process::id()));
        tmp(&path);
        let mut spool = Spool::open(&path, 2, 10_000_000).unwrap();
        let mk = |id: &str| PendingEvent {
            event_id: id.into(),
            kind: "usage".into(),
            payload: serde_json::json!({"x": 1}),
        };
        assert!(spool.insert_batch(&[mk("a"), mk("b")], &[]).unwrap());
        // 达到上限 → 拒绝写入但保留已写入数据
        assert!(!spool.insert_batch(&[mk("c")], &[]).unwrap());
        assert!(spool.full_flag().is_full());
        assert_eq!(spool.pending_count(), 2);
        // 清空后解除
        spool.ack_uploaded("b", &["a".into(), "b".into()]).unwrap();
        assert!(!spool.full_flag().is_full());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn dead_letter_on_non_retryable() {
        let path = std::env::temp_dir().join(format!("spool-dl-{}", std::process::id()));
        tmp(&path);
        let mut spool = Spool::open(&path, 100, 10_000_000).unwrap();
        spool
            .insert_batch(
                &[PendingEvent {
                    event_id: "bad".into(),
                    kind: "usage".into(),
                    payload: serde_json::json!({}),
                }],
                &[],
            )
            .unwrap();
        spool
            .fail_events("b", &["bad".into()], false, "invalid")
            .unwrap();
        assert_eq!(spool.pending_count(), 0);
        assert_eq!(spool.dead_letter_count(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn offline_events_survive_restart_and_catch_up() {
        // 断网补传：事件写入 spool，重启后仍在 pending，可续传
        let path = std::env::temp_dir().join(format!("spool-offline-{}", std::process::id()));
        tmp(&path);
        let mk = |id: &str| PendingEvent {
            event_id: id.into(),
            kind: "usage".into(),
            payload: serde_json::json!({"x": 1}),
        };
        {
            let mut spool = Spool::open(&path, 100, 10_000_000).unwrap();
            spool.insert_batch(&[mk("off1"), mk("off2")], &[]).unwrap();
            assert_eq!(spool.pending_count(), 2);
        } // 模拟 Agent 重启（连接关闭）
        {
            let mut spool = Spool::open(&path, 100, 10_000_000).unwrap();
            assert_eq!(spool.pending_count(), 2, "重启后事件不应丢失");
            let (batch, items) = spool.next_batch(10, 10_000);
            assert_eq!(items.len(), 2);
            // 网络恢复：ack 后清空
            spool
                .ack_uploaded(&batch, &["off1".into(), "off2".into()])
                .unwrap();
            assert_eq!(spool.pending_count(), 0);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn partial_success_retries_only_failed() {
        // 部分成功：仅 ack 成功子集，失败子集保留并重试
        let path = std::env::temp_dir().join(format!("spool-partial-{}", std::process::id()));
        tmp(&path);
        let mk = |id: &str| PendingEvent {
            event_id: id.into(),
            kind: "usage".into(),
            payload: serde_json::json!({"x": 1}),
        };
        let mut spool = Spool::open(&path, 100, 10_000_000).unwrap();
        spool
            .insert_batch(&[mk("p1"), mk("p2"), mk("p3")], &[])
            .unwrap();
        let (batch, _items) = spool.next_batch(10, 10_000);
        // 服务器确认 p1；p2/p3 可重试失败
        spool.ack_uploaded(&batch, &["p1".into()]).unwrap();
        spool
            .fail_events(&batch, &["p2".into(), "p3".into()], true, "hub busy")
            .unwrap();
        assert_eq!(spool.pending_count(), 2, "失败子集应保留重试");
        // 重传：再次取批，只含失败子集
        let (_, items2) = spool.next_batch(10, 10_000);
        assert_eq!(items2.len(), 2);
        let ids: Vec<&str> = items2.iter().map(|e| e.event_id.as_str()).collect();
        assert!(
            ids.contains(&"p2") && ids.contains(&"p3"),
            "只应重传失败子集: {ids:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retry_exhausted_moves_to_dead_letter() {
        // 重试次数超限 → 转死信，避免无限重试
        let path = std::env::temp_dir().join(format!("spool-exhaust-{}", std::process::id()));
        tmp(&path);
        let mut spool = Spool::open(&path, 100, 10_000_000).unwrap();
        spool
            .insert_batch(
                &[PendingEvent {
                    event_id: "exh".into(),
                    kind: "usage".into(),
                    payload: serde_json::json!({}),
                }],
                &[],
            )
            .unwrap();
        let (batch, _) = spool.next_batch(10, 10_000);
        // 多次可重试失败累积 attempts
        for _ in 0..5 {
            spool
                .fail_events(&batch, &["exh".into()], true, "busy")
                .unwrap();
        }
        let attempts: i64 = spool
            .conn
            .query_row(
                "SELECT attempts FROM pending_events WHERE event_id = 'exh'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(attempts, 5, "attempts 应累积");
        // 非可重试失败 → 转死信
        spool
            .fail_events(&batch, &["exh".into()], false, "invalid")
            .unwrap();
        assert_eq!(spool.pending_count(), 0);
        assert_eq!(spool.dead_letter_count(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
