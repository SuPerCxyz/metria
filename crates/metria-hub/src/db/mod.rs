//! Hub 存储层：连接管理、repository 方法与 ingest 落库。
//!
//! 会话引用统一为规范键 `{node_id}:{source_session_id}`，保证幂等与跨表 join。
//! 所有事件 insert 方法返回 `bool`（是否新插入），用于幂等与 rollup 判定。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use metria_storage::rusqlite::{params, Connection, OptionalExtension};
use metria_storage::StorageError;
use serde_json::Value;

use crate::config::HubConfig;

pub mod pricing;
pub mod traffic;

/// Hub 数据库。
#[derive(Debug, Clone)]
pub struct HubDb {
    conn: Arc<Mutex<Connection>>,
}

/// 可安全返回给 Web 的用户资料。
#[derive(Debug, Clone)]
pub struct UserProfile {
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_text: Option<String>,
    pub avatar_color: String,
    pub role: String,
}

impl HubDb {
    pub fn open(cfg: &HubConfig) -> Result<Self, StorageError> {
        let path = cfg
            .sqlite_path()
            .map_err(|e| StorageError::Open(e.to_string()))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(StorageError::Io)?;
            }
        }
        let conn = metria_storage::open(&path, &metria_storage::DbOptions::default())?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn apply_migrations(&self) -> Result<Vec<i64>, StorageError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| StorageError::Query("lock".into()))?;
        metria_storage::migrate_embedded(&mut conn, None)
    }

    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("hub db lock poisoned")
    }

    pub fn quick_check(&self) -> Result<(), StorageError> {
        let c = self.conn();
        metria_storage::quick_check(&c)
    }

    /// 执行 WAL checkpoint（TRUNCATE），回收 WAL 文件。
    pub fn wal_checkpoint(&self) -> Result<(), StorageError> {
        let c = self.conn();
        metria_storage::wal_checkpoint(&c)
    }

    /// 执行 incremental_vacuum，回收空闲页（§9）。阈值由 caller 决定。
    pub fn incremental_vacuum(&self) -> Result<(), StorageError> {
        let c = self.conn();
        c.execute_batch("PRAGMA incremental_vacuum")
            .map_err(StorageError::from)?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, StorageError> {
        let c = self.conn();
        metria_storage::migrations::current_version(&c)
    }

    pub fn count(&self, table: &str) -> i64 {
        self.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0)
    }

    // ---------- 用户账户 ----------

    pub fn user_password_hash(&self, username: &str) -> Result<Option<String>, StorageError> {
        let c = self.conn();
        c.query_row(
            "SELECT password_hash FROM users WHERE username = ?1",
            [username],
            |r| r.get(0),
        )
        .optional()
        .map_err(StorageError::from)
    }

    pub fn user_credentials(&self, username: &str) -> Result<Option<(String, bool)>, StorageError> {
        let c = self.conn();
        c.query_row(
            "SELECT password_hash, must_change_password FROM users WHERE username = ?1",
            [username],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)),
        )
        .optional()
        .map_err(StorageError::from)
    }

    pub fn user_profile(&self, username: &str) -> Result<Option<UserProfile>, StorageError> {
        let c = self.conn();
        c.query_row(
            "SELECT username, display_name, avatar_text, avatar_color, role FROM users WHERE username = ?1",
            [username],
            |r| {
                Ok(UserProfile {
                    username: r.get(0)?,
                    display_name: r.get(1)?,
                    avatar_text: r.get(2)?,
                    avatar_color: r.get(3)?,
                    role: r.get(4)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
    }

    pub fn update_user_profile(
        &self,
        username: &str,
        display_name: Option<&str>,
        avatar_text: Option<&str>,
        avatar_color: &str,
    ) -> Result<bool, StorageError> {
        let c = self.conn();
        let changed = c
            .execute(
                "UPDATE users SET display_name = ?1, avatar_text = ?2, avatar_color = ?3, updated_at = ?4 WHERE username = ?5",
                params![display_name, avatar_text, avatar_color, Utc::now().to_rfc3339(), username],
            )?;
        Ok(changed > 0)
    }

    pub fn update_user_password(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<(), StorageError> {
        let c = self.conn();
        let now = Utc::now().to_rfc3339();
        let changed = c.execute(
            "UPDATE users SET password_hash = ?1, must_change_password = 0, updated_at = ?2 WHERE username = ?3",
            params![password_hash, now, username],
        )?;
        if changed == 0 {
            c.execute(
                "INSERT INTO users (id, username, password_hash, must_change_password, role, created_at, updated_at) VALUES (?1, ?2, ?3, 0, 'admin', ?4, ?4)",
                params![format!("user-{username}"), username, password_hash, now],
            )?;
        }
        Ok(())
    }

    // ---------- 身份 ----------

    #[allow(clippy::too_many_arguments)]
    pub fn register_node_collector(
        &self,
        node_id: &str,
        node_name: &str,
        platform: Option<&str>,
        arch: Option<&str>,
        agent_version: &str,
        protocol_version: u32,
        now: DateTime<Utc>,
    ) -> Result<(String, bool), StorageError> {
        let c = self.conn();
        let ts = now.to_rfc3339();
        let existed: i64 = c
            .query_row("SELECT COUNT(*) FROM nodes WHERE id = ?1", [node_id], |r| {
                r.get(0)
            })
            .map_err(StorageError::from)?;
        if existed == 0 {
            c.execute(
                "INSERT INTO nodes (id, name, platform, architecture, first_seen_at, last_seen_at, status, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5,'online',?5,?5)",
                params![node_id, node_name, platform, arch, ts],
            )
            .map_err(StorageError::from)?;
        } else {
            c.execute(
                "UPDATE nodes SET last_seen_at = ?1, status = 'online', updated_at = ?1 WHERE id = ?2",
                params![ts, node_id],
            )
            .map_err(StorageError::from)?;
        }

        let collector_id = format!("collector-{node_id}");
        let cexists: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM collectors WHERE id = ?1",
                [&collector_id],
                |r| r.get(0),
            )
            .map_err(StorageError::from)?;
        if cexists == 0 {
            c.execute(
                "INSERT INTO collectors (id, node_id, agent_version, protocol_version, started_at, last_heartbeat_at, status, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5,'online',?5,?5)",
                params![collector_id, node_id, agent_version, protocol_version, ts],
            )
            .map_err(StorageError::from)?;
        } else {
            c.execute(
                "UPDATE collectors SET agent_version = ?1, last_heartbeat_at = ?2, status = 'online', updated_at = ?2 WHERE id = ?3",
                params![agent_version, ts, collector_id],
            )
            .map_err(StorageError::from)?;
        }
        Ok((collector_id, existed == 0))
    }

    /// Collector token 默认有效期（秒）：7 天。
    pub const TOKEN_TTL_SECONDS: i64 = 7 * 24 * 3600;

    /// 校验 collector token（仅存哈希，检查有效期）。
    pub fn verify_collector_token(&self, token: &str) -> Option<(String, String)> {
        let c = self.conn();
        let hash = blake3_hex(token);
        let now = Utc::now().to_rfc3339();
        c.query_row(
            "SELECT t.collector_id, c.node_id, t.expires_at FROM collector_tokens t JOIN collectors c ON c.id = t.collector_id WHERE t.token_hash = ?1 AND t.status = 'active'",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?)),
        )
        .ok()
        .filter(|(_, _, expires_at)| match expires_at {
            None => true, // 迁移前遗留 token 视为永久有效
            Some(exp) => exp.as_str() > now.as_str(),
        })
        .map(|(cid, nid, _)| (cid, nid))
    }

    pub fn upsert_collector_token(
        &self,
        collector_id: &str,
        token: &str,
    ) -> Result<(), StorageError> {
        let c = self.conn();
        let hash = blake3_hex(token);
        let now = Utc::now().to_rfc3339();
        let expires_at =
            (Utc::now() + chrono::Duration::seconds(Self::TOKEN_TTL_SECONDS)).to_rfc3339();
        c.execute(
            "INSERT INTO collector_tokens (id, collector_id, token_hash, status, created_at, expires_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET status='active', expires_at=excluded.expires_at",
            params![
                format!("tok-{}", &hash[..12]),
                collector_id,
                hash,
                now,
                expires_at
            ],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    /// 吊销 collector token（软删除：置为 revoked）。
    pub fn revoke_collector_token(&self, collector_id: &str) -> Result<usize, StorageError> {
        let c = self.conn();
        let n = c
            .execute(
                "UPDATE collector_tokens SET status = 'revoked', revoked_at = ?1
                 WHERE collector_id = ?2 AND status = 'active'",
                params![Utc::now().to_rfc3339(), collector_id],
            )
            .map_err(StorageError::from)?;
        Ok(n)
    }

    /// 为 collector 生成并注册新 token（轮换：吊销旧 token，写入新 token 哈希）。
    pub fn rotate_collector_token(
        &self,
        collector_id: &str,
        new_token: &str,
    ) -> Result<(), StorageError> {
        self.revoke_collector_token(collector_id)?;
        self.upsert_collector_token(collector_id, new_token)
    }

    /// 列出某 collector 的全部 token 记录（用于管理界面展示）。
    pub fn list_collector_tokens(&self, collector_id: &str) -> Vec<serde_json::Value> {
        let c = self.conn();
        let Ok(mut stmt) = c.prepare(
            "SELECT id, label, status, created_at, expires_at, revoked_at
             FROM collector_tokens WHERE collector_id = ?1 ORDER BY created_at DESC",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([collector_id], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "label": r.get::<_, Option<String>>(1)?,
                "status": r.get::<_, String>(2)?,
                "created_at": r.get::<_, String>(3)?,
                "expires_at": r.get::<_, Option<String>>(4)?,
                "revoked_at": r.get::<_, Option<String>>(5)?,
            }))
        }) else {
            return Vec::new();
        };
        rows.filter_map(|x| x.ok()).collect()
    }

    // ---------- 节点管理（前端创建 / 编辑 / 删除） ----------

    /// 创建节点并预建 collector 与专属 token（同一事务）。
    /// 返回 (node_id, collector_id, 明文 token)。明文 token 仅此一次返回。
    pub fn create_node(
        &self,
        name: &str,
        description: Option<&str>,
        labels: Vec<String>,
        ip: Option<&str>,
        hub_url: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(String, String, String), StorageError> {
        let node_id = format!("node-{}", metria_core::model::Id::new());
        let collector_id = format!("collector-{node_id}");
        let plain_token = format!("mct-{}", metria_core::model::Id::new());
        let c = self.conn();
        let ts = now.to_rfc3339();
        let labels_json = serde_json::to_string(&labels)
            .map_err(|e| StorageError::Serde(format!("labels 序列化失败: {e}")))?;
        let tx = c.unchecked_transaction().map_err(StorageError::from)?;
        tx.execute(
            "INSERT INTO nodes (id, name, description, labels, ip, hub_url, status, first_seen_at, last_seen_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?7, ?7, ?7)",
            params![node_id, name, description, labels_json, ip, hub_url, ts],
        )
        .map_err(StorageError::from)?;
        tx.execute(
            "INSERT INTO collectors (id, node_id, agent_version, protocol_version, started_at, last_heartbeat_at, status, created_at, updated_at)
             VALUES (?1, ?2, '', 0, ?3, ?3, 'pending', ?3, ?3)",
            params![collector_id, node_id, ts],
        )
        .map_err(StorageError::from)?;
        let hash = blake3_hex(&plain_token);
        let expires_at = (now + chrono::Duration::seconds(Self::TOKEN_TTL_SECONDS)).to_rfc3339();
        tx.execute(
            "INSERT INTO collector_tokens (id, collector_id, token_hash, status, created_at, expires_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?5)",
            params![
                format!("tok-{}", &hash[..12]),
                collector_id,
                hash,
                ts,
                expires_at
            ],
        )
        .map_err(StorageError::from)?;
        tx.commit().map_err(StorageError::from)?;
        Ok((node_id, collector_id, plain_token))
    }

    /// 更新节点信息（name/ip/description/labels/hub_url）。返回是否命中。
    #[allow(clippy::too_many_arguments)]
    pub fn update_node(
        &self,
        node_id: &str,
        name: &str,
        description: Option<&str>,
        labels: Vec<String>,
        ip: Option<&str>,
        hub_url: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<bool, StorageError> {
        let c = self.conn();
        let labels_json = serde_json::to_string(&labels)
            .map_err(|e| StorageError::Serde(format!("labels 序列化失败: {e}")))?;
        let n = c
            .execute(
                "UPDATE nodes SET name = ?1, description = ?2, labels = ?3, ip = ?4, hub_url = ?5, updated_at = ?6 WHERE id = ?7",
                params![name, description, labels_json, ip, hub_url, now.to_rfc3339(), node_id],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    /// 删除节点：级联清理 sources / collectors / collector_tokens，保留业务数据。
    /// sources 为来源发现记录（sources.node_id 有外键），Agent 重新注册后会重建；
    /// 业务数据（sessions/model_calls/usage_events）的 node_id 无外键约束，予以保留。
    /// 返回是否命中。
    pub fn delete_node(&self, node_id: &str) -> Result<bool, StorageError> {
        let c = self.conn();
        let tx = c.unchecked_transaction().map_err(StorageError::from)?;
        tx.execute(
            "DELETE FROM source_errors WHERE source_id IN (SELECT id FROM sources WHERE node_id = ?1)",
            params![node_id],
        )
        .map_err(StorageError::from)?;
        tx.execute("DELETE FROM sources WHERE node_id = ?1", params![node_id])
            .map_err(StorageError::from)?;
        tx.execute(
            "DELETE FROM collector_tokens WHERE collector_id IN (SELECT id FROM collectors WHERE node_id = ?1)",
            params![node_id],
        )
        .map_err(StorageError::from)?;
        tx.execute(
            "DELETE FROM collectors WHERE node_id = ?1",
            params![node_id],
        )
        .map_err(StorageError::from)?;
        let n = tx
            .execute("DELETE FROM nodes WHERE id = ?1", params![node_id])
            .map_err(StorageError::from)?;
        tx.commit().map_err(StorageError::from)?;
        Ok(n > 0)
    }

    /// 查询单节点基础信息。
    pub fn get_node(&self, node_id: &str) -> Option<serde_json::Value> {
        let c = self.conn();
        let r = c
            .query_row(
                "SELECT id, name, description, labels, ip, hub_url, platform, architecture, timezone, status,
                        first_seen_at, last_seen_at, created_at, updated_at
                 FROM nodes WHERE id = ?1",
                [node_id],
                |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "description": r.get::<_, Option<String>>(2)?,
                        "labels": r.get::<_, String>(3)?,
                        "ip": r.get::<_, Option<String>>(4)?,
                        "hub_url": r.get::<_, Option<String>>(5)?,
                        "platform": r.get::<_, Option<String>>(6)?,
                        "architecture": r.get::<_, Option<String>>(7)?,
                        "timezone": r.get::<_, Option<String>>(8)?,
                        "status": r.get::<_, String>(9)?,
                        "first_seen_at": r.get::<_, String>(10)?,
                        "last_seen_at": r.get::<_, String>(11)?,
                        "created_at": r.get::<_, String>(12)?,
                        "updated_at": r.get::<_, String>(13)?,
                    }))
                },
            )
            .ok()?;
        Some(r)
    }

    /// 查询节点的 collector_id（最新一个）。
    pub fn get_node_collector_id(&self, node_id: &str) -> Option<String> {
        let c = self.conn();
        c.query_row(
            "SELECT id FROM collectors WHERE node_id = ?1 ORDER BY created_at DESC LIMIT 1",
            [node_id],
            |r| r.get::<_, String>(0),
        )
        .ok()
    }

    /// 为 collector 新签发一个 active token（不吊销既有 token，保证运行中 agent 不受影响）。
    /// 返回明文 token（仅本次返回，Hub 只存哈希）。
    pub fn issue_collector_token(
        &self,
        collector_id: &str,
        now: DateTime<Utc>,
    ) -> Result<String, StorageError> {
        let plain_token = format!("mct-{}", metria_core::model::Id::new());
        let c = self.conn();
        let hash = blake3_hex(&plain_token);
        let ts = now.to_rfc3339();
        let expires_at = (now + chrono::Duration::seconds(Self::TOKEN_TTL_SECONDS)).to_rfc3339();
        c.execute(
            "INSERT INTO collector_tokens (id, collector_id, token_hash, status, created_at, expires_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?5)",
            params![
                format!("tok-{}", &hash[..12]),
                collector_id,
                hash,
                ts,
                expires_at
            ],
        )
        .map_err(StorageError::from)?;
        Ok(plain_token)
    }

    pub fn heartbeat(
        &self,
        node_id: &str,
        collector_id: &str,
        pending: i64,
        size: i64,
        now: DateTime<Utc>,
        clock_skew_seconds: i64,
    ) -> Result<(), StorageError> {
        let c = self.conn();
        let ts = now.to_rfc3339();
        c.execute(
            "UPDATE collectors SET last_heartbeat_at = ?1, spool_pending_events = ?2, spool_size_bytes = ?3, clock_skew_seconds = ?5, status = 'online', updated_at = ?1 WHERE id = ?4",
            params![ts, pending, size, collector_id, clock_skew_seconds],
        )
        .map_err(StorageError::from)?;
        c.execute(
            "UPDATE nodes SET last_seen_at = ?1, status = 'online', updated_at = ?1 WHERE id = ?2",
            params![ts, node_id],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    pub fn record_batch(
        &self,
        batch_id: &str,
        node_id: &str,
        collector_id: &str,
        count: i64,
        bytes: i64,
    ) -> Result<(), StorageError> {
        let c = self.conn();
        c.execute(
            "INSERT OR IGNORE INTO upload_batches (batch_id, node_id, collector_id, received_at, status, event_count, bytes) VALUES (?1,?2,?3,?4,'accepted',?5,?6)",
            params![batch_id, node_id, collector_id, Utc::now().to_rfc3339(), count, bytes],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    // ---------- 来源 ----------

    pub fn upsert_client(&self, canonical: &str, display: &str) -> Result<(), StorageError> {
        let c = self.conn();
        let now = Utc::now().to_rfc3339();
        c.execute(
            "INSERT OR IGNORE INTO clients (id, canonical_name, display_name, category, created_at, updated_at) VALUES (?1, ?2, ?3, NULL, ?4, ?4)",
            params![canonical, canonical, display, now],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    pub fn upsert_source(&self, v: &Value) -> Result<bool, StorageError> {
        let c = self.conn();
        let now = Utc::now().to_rfc3339();
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let client_id = get("client_id");
        if !client_id.is_empty() {
            let _ = c.execute(
                "INSERT OR IGNORE INTO clients (id, canonical_name, display_name, category, created_at, updated_at) VALUES (?1, ?2, ?2, NULL, ?3, ?3)",
                params![client_id, client_id, now],
            );
        }
        let n = c
            .execute(
                "INSERT OR IGNORE INTO sources (id, node_id, collector_id, client_id, adapter_id, adapter_version, source_fingerprint, source_path_hash, status, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'active',?9,?9)",
                params![
                    get("id"),
                    get("node_id"),
                    get("collector_id"),
                    client_id,
                    get("adapter_id"),
                    get("adapter_version"),
                    get("source_fingerprint"),
                    get("source_path_hash"),
                    now,
                ],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    pub fn upsert_project(&self, canonical_key: &str, path_hash: &str) -> Result<(), StorageError> {
        let c = self.conn();
        let now = Utc::now().to_rfc3339();
        c.execute(
            "INSERT OR IGNORE INTO projects (id, canonical_key, path_hash, metadata, first_seen_at, last_seen_at, created_at, updated_at) VALUES (?1,?2,?3,'{}',?4,?4,?4,?4)",
            params![
                format!("project-{}", &blake3_hex(canonical_key)[..16]),
                canonical_key,
                path_hash,
                now,
            ],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    // ---------- 事件落库 ----------

    pub fn session_key(node: &str, source: &str) -> String {
        format!("{node}:{source}")
    }

    pub fn upsert_session(&self, v: &Value) -> Result<bool, StorageError> {
        let c = self.conn();
        let now = Utc::now().to_rfc3339();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| v.get(k).and_then(|x| x.as_i64());
        let gf = |k: &str| v.get(k).and_then(|x| x.as_f64());
        let key = Self::session_key(g("node_id"), g("source_session_id"));
        // 同步维护 projects 维度表（project_id 存在时，使用已持有的 conn 避免重入死锁）
        let project_id = g("project_id");
        if !project_id.is_empty() {
            let _ = c.execute(
                "INSERT OR IGNORE INTO projects (id, canonical_key, path_hash, metadata, first_seen_at, last_seen_at, created_at, updated_at) VALUES (?1,?2,?3,'{}',?4,?4,?4,?4)",
                params![
                    format!("project-{}", &blake3_hex(project_id)[..16]),
                    project_id,
                    g("working_directory_hash"),
                    now,
                ],
            );
        }
        let n = c
            .execute(
                "INSERT OR IGNORE INTO sessions (
                    id, source_session_id, node_id, collector_id, source_id, client_id, project_id,
                    parent_session_id, title, working_directory_hash, started_at, ended_at, last_activity_at,
                    provider_raw, provider_normalized, primary_model_raw, primary_model_normalized, status,
                    message_count, tool_call_count, subagent_count, model_call_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                    estimated_request_bytes, estimated_response_bytes, estimated_total_bytes,
                    traffic_confidence, content_available, created_at, updated_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37)",
                params![
                    key,
                    g("source_session_id"),
                    g("node_id"),
                    g("collector_id"),
                    g("source_id"),
                    g("client_id"),
                    opt(g("project_id")),
                    opt(g("parent_session_id")),
                    opt(g("title")),
                    opt(g("working_directory_hash")),
                    g("started_at"),
                    opt(g("ended_at")),
                    opt(g("last_activity_at")),
                    opt(g("provider_raw")),
                    opt(g("provider_normalized")),
                    opt(g("primary_model_raw")),
                    opt(g("primary_model_normalized")),
                    g("status"),
                    gn("message_count").unwrap_or(0),
                    gn("tool_call_count").unwrap_or(0),
                    gn("subagent_count").unwrap_or(0),
                    gn("model_call_count").unwrap_or(0),
                    gn("input_tokens"),
                    gn("output_tokens"),
                    gn("cache_read_tokens"),
                    gn("cache_write_tokens"),
                    gn("reasoning_tokens"),
                    gn("reported_cost_micro_usd"),
                    gn("calculated_cost_micro_usd"),
                    gn("estimated_cost_micro_usd"),
                    gn("estimated_request_bytes"),
                    gn("estimated_response_bytes"),
                    gn("estimated_total_bytes"),
                    gf("traffic_confidence"),
                    bool_i(v.get("content_available")),
                    g("created_at"),
                    now,
                ],
            )
            .map_err(StorageError::from)?;
        // 已有会话标题为空时补充（如旧数据重扫后生成标题），不改变 duplicate 判定。
        let title = g("title");
        if !title.is_empty() {
            let _ = c.execute(
                "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3 AND (title IS NULL OR title = '')",
                params![title, now, key],
            );
        }
        Ok(n > 0)
    }

    pub fn insert_message(&self, v: &Value, session_key: &str) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| v.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        let now = Utc::now().to_rfc3339();
        let n = c
            .execute(
                "INSERT OR IGNORE INTO messages (id, turn_id, session_id, source_message_id, sequence, role, content_type, content, content_hash, content_length, utf8_bytes, created_at, redacted) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    g("id"),
                    opt(g("turn_id")),
                    session_key,
                    opt(g("source_message_id")),
                    gn("sequence"),
                    g("role"),
                    g("content_type"),
                    v.get("content").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    opt(g("content_hash")),
                    gn("content_length"),
                    gn("utf8_bytes"),
                    now,
                    bool_i(v.get("redacted")),
                ],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    pub fn insert_call(&self, v: &Value, session_key: &str) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| v.get(k).and_then(|x| x.as_i64());
        let now = Utc::now().to_rfc3339();
        let n = c
            .execute(
                "INSERT OR IGNORE INTO model_calls (
                    id, source_call_id, node_id, collector_id, client_id, source_id, project_id, session_id,
                    turn_id, provider_raw, provider_normalized, model_raw, model_normalized, started_at,
                    first_response_at, completed_at, duration_ms, status, status_code, streaming, stream_completed,
                    client_aborted, retry_count, call_granularity, input_tokens, output_tokens, cache_read_tokens,
                    cache_write_tokens, reasoning_tokens, reported_cost_micro_usd, calculated_cost_micro_usd,
                    estimated_cost_micro_usd, usage_event_id, traffic_estimate_id, created_at, updated_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36)",
                params![
                    g("id"),
                    opt(g("source_call_id")),
                    g("node_id"),
                    g("collector_id"),
                    g("client_id"),
                    g("source_id"),
                    opt(g("project_id")),
                    session_key,
                    opt(g("turn_id")),
                    opt(g("provider_raw")),
                    opt(g("provider_normalized")),
                    opt(g("model_raw")),
                    opt(g("model_normalized")),
                    g("started_at"),
                    opt(g("first_response_at")),
                    opt(g("completed_at")),
                    gn("duration_ms"),
                    g("status"),
                    gn("status_code"),
                    bool_i(v.get("streaming")),
                    opt_bool_i(v.get("stream_completed")),
                    bool_i(v.get("client_aborted")),
                    gn("retry_count").unwrap_or(0),
                    g("call_granularity"),
                    gn("input_tokens"),
                    gn("output_tokens"),
                    gn("cache_read_tokens"),
                    gn("cache_write_tokens"),
                    gn("reasoning_tokens"),
                    gn("reported_cost_micro_usd"),
                    gn("calculated_cost_micro_usd"),
                    gn("estimated_cost_micro_usd"),
                    opt(g("usage_event_id")),
                    opt(g("traffic_estimate_id")),
                    now,
                    now,
                ],
            )
            .map_err(StorageError::from)?;
        self.link_usage_to_call(&c, g("id"))?;
        // 即使这是上传重试中的重复 call，也重算一次；若上次 call 已提交但聚合刷新失败，
        // 本次重试仍能修复 session 摘要。
        self.refresh_session_agg(&c, session_key)?;
        Ok(n > 0)
    }

    /// 新调用落库后刷新会话聚合字段。
    ///
    /// call 事件与 session 事件可能不在同一批次到达，session 表的聚合字段必须在
    /// 新 call 入库时实时重算，避免会话列表显示空值。
    /// 复用调用方已持有的连接（避免重入 Mutex 死锁）。
    fn refresh_session_agg(&self, c: &Connection, session_key: &str) -> Result<(), StorageError> {
        if session_key.is_empty() {
            return Ok(());
        }
        let agg = c
            .query_row(
                "SELECT COUNT(*), SUM(input_tokens), SUM(output_tokens),
                        SUM(cache_read_tokens), SUM(cache_write_tokens), SUM(reasoning_tokens),
                        SUM(calculated_cost_micro_usd), SUM(estimated_cost_micro_usd),
                        SUM(reported_cost_micro_usd),
                        MAX(COALESCE(completed_at, started_at)),
                        (SELECT model_raw FROM model_calls latest
                         WHERE latest.session_id = ?1 AND latest.model_raw IS NOT NULL
                         ORDER BY latest.started_at DESC, latest.id DESC LIMIT 1),
                        (SELECT model_normalized FROM model_calls latest
                         WHERE latest.session_id = ?1 AND latest.model_normalized IS NOT NULL
                         ORDER BY latest.started_at DESC, latest.id DESC LIMIT 1)
                 FROM model_calls WHERE session_id = ?1",
                [session_key],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        r.get::<_, Option<i64>>(6)?,
                        r.get::<_, Option<i64>>(7)?,
                        r.get::<_, Option<i64>>(8)?,
                        r.get::<_, Option<String>>(9)?,
                        r.get::<_, Option<String>>(10)?,
                        r.get::<_, Option<String>>(11)?,
                    ))
                },
            )
            .map_err(StorageError::from)?;
        let (
            count,
            in_tok,
            out_tok,
            cr_tok,
            cw_tok,
            rs_tok,
            calc_cost,
            est_cost,
            rep_cost,
            last,
            model_raw,
            model_normalized,
        ) = agg;
        c.execute(
            "UPDATE sessions SET
                model_call_count = ?1,
                input_tokens = ?2, output_tokens = ?3,
                cache_read_tokens = ?4, cache_write_tokens = ?5, reasoning_tokens = ?6,
                calculated_cost_micro_usd = ?7, estimated_cost_micro_usd = ?8, reported_cost_micro_usd = ?9,
                last_activity_at = COALESCE(?10, last_activity_at),
                primary_model_raw = COALESCE(?11, primary_model_raw),
                primary_model_normalized = COALESCE(?12, primary_model_normalized),
                updated_at = ?13
             WHERE id = ?14",
            params![
                count,
                in_tok,
                out_tok,
                cr_tok,
                cw_tok,
                rs_tok,
                calc_cost,
                est_cost,
                rep_cost,
                last,
                model_raw,
                model_normalized,
                Utc::now().to_rfc3339(),
                session_key,
            ],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    /// 落库 usage_event（幂等），返回是否为新事件。
    pub fn insert_usage(&self, v: &Value, session_key: &str) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let usage = v.get("usage").unwrap_or(&Value::Null);
        let u = |k: &str| usage.get(k).and_then(|x| x.as_i64());
        let cost = v.get("cost").unwrap_or(&Value::Null);
        let cost_f = |k: &str| cost.get(k).and_then(|x| x.as_i64());
        let quality = v.get("quality").unwrap_or(&Value::Null);
        let n = c
            .execute(
                "INSERT OR IGNORE INTO usage_events (
                    event_id, schema_version, node_id, collector_id, source_id, client_id, adapter_id, adapter_version,
                    session_id, turn_id, model_call_id, timestamp, provider_raw, provider_normalized, model_raw, model_normalized,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd, pricing_rule_id, pricing_snapshot_id,
                    usage_source, usage_granularity, usage_confidence
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29)",
                params![
                    g("event_id"),
                    v.get("schema_version").and_then(|x| x.as_u64()).unwrap_or(1) as i64,
                    g("node_id"),
                    g("collector_id"),
                    g("source_id"),
                    g("client_id"),
                    g("adapter_id"),
                    g("adapter_version"),
                    session_key.to_string(),
                    opt(g("turn_id")),
                    opt(g("model_call_id")),
                    g("timestamp"),
                    opt(g("provider_raw")),
                    opt(g("provider_normalized")),
                    opt(g("model_raw")),
                    opt(g("model_normalized")),
                    u("input"),
                    u("output"),
                    u("cache_read"),
                    u("cache_write"),
                    u("reasoning"),
                    cost_f("reported_micro_usd"),
                    cost_f("calculated_micro_usd"),
                    cost_f("estimated_micro_usd"),
                    opt(cost.get("pricing_rule_id").and_then(|x| x.as_str()).unwrap_or("")),
                    opt(cost.get("pricing_snapshot_id").and_then(|x| x.as_str()).unwrap_or("")),
                    quality.get("usage_source").and_then(|x| x.as_str()).unwrap_or("reported"),
                    quality.get("granularity").and_then(|x| x.as_str()).unwrap_or("call"),
                    quality.get("confidence").and_then(|x| x.as_f64()),
                ],
            )
            .map_err(StorageError::from)?;
        let model_call_id = g("model_call_id");
        if !model_call_id.is_empty() {
            self.link_usage_to_call(&c, model_call_id)?;
            self.refresh_session_agg(&c, session_key)?;
        }
        Ok(n > 0)
    }

    /// 双向补齐 usage_event 与 model_call 的关联，兼容两类事件先后到达。
    fn link_usage_to_call(&self, c: &Connection, model_call_id: &str) -> Result<(), StorageError> {
        if model_call_id.is_empty() {
            return Ok(());
        }
        c.execute(
            "UPDATE model_calls SET
                usage_event_id = COALESCE(usage_event_id, (SELECT event_id FROM usage_events WHERE model_call_id = ?1 ORDER BY timestamp, event_id LIMIT 1)),
                reported_cost_micro_usd = COALESCE((SELECT reported_cost_micro_usd FROM usage_events WHERE model_call_id = ?1 ORDER BY timestamp, event_id LIMIT 1), reported_cost_micro_usd),
                calculated_cost_micro_usd = COALESCE((SELECT calculated_cost_micro_usd FROM usage_events WHERE model_call_id = ?1 ORDER BY timestamp, event_id LIMIT 1), calculated_cost_micro_usd),
                estimated_cost_micro_usd = COALESCE((SELECT estimated_cost_micro_usd FROM usage_events WHERE model_call_id = ?1 ORDER BY timestamp, event_id LIMIT 1), estimated_cost_micro_usd)
             WHERE id = ?1",
            [model_call_id],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    pub fn insert_traffic(&self, v: &Value) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| v.get(k).and_then(|x| x.as_i64());
        let gf = |k: &str| v.get(k).and_then(|x| x.as_f64());
        let n = c
            .execute(
                "INSERT OR IGNORE INTO traffic_estimates (
                    id, model_call_id, node_id, client_id, session_id, turn_id, provider, model,
                    request_payload_bytes, response_payload_bytes, estimated_request_http_bytes, estimated_response_http_bytes,
                    estimated_request_wire_bytes, estimated_response_wire_bytes, estimated_total_wire_bytes,
                    lower_bound_bytes, upper_bound_bytes, estimation_source, context_transport_mode, cache_transport_behavior,
                    request_reconstruction_quality, response_reconstruction_quality, profile_id, profile_version, confidence,
                    calculated_at, created_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27)",
                params![
                    g("id"),
                    g("model_call_id"),
                    g("node_id"),
                    g("client_id"),
                    opt(g("session_id")),
                    opt(g("turn_id")),
                    opt(g("provider")),
                    opt(g("model")),
                    gn("request_payload_bytes"),
                    gn("response_payload_bytes"),
                    gn("estimated_request_http_bytes"),
                    gn("estimated_response_http_bytes"),
                    gn("estimated_request_wire_bytes"),
                    gn("estimated_response_wire_bytes"),
                    gn("estimated_total_wire_bytes"),
                    gn("lower_bound_bytes"),
                    gn("upper_bound_bytes"),
                    g("estimation_source"),
                    g("context_transport_mode"),
                    g("cache_transport_behavior"),
                    g("request_reconstruction_quality"),
                    g("response_reconstruction_quality"),
                    opt(g("profile_id")),
                    gn("profile_version"),
                    gf("confidence"),
                    g("calculated_at"),
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    pub fn insert_tool(&self, v: &Value) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| v.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        let gf = |k: &str| v.get(k).and_then(|x| x.as_f64());
        let n = c
            .execute(
                "INSERT OR IGNORE INTO tool_events (id, session_id, model_call_id, turn_id, source_tool_id, name, tool_type, status, input_content_hash, output_content_hash, input_length, output_length, started_at, completed_at, duration_ms, error, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                params![
                    g("id"),
                    opt(g("session_id")),
                    opt(g("model_call_id")),
                    opt(g("turn_id")),
                    opt(g("source_tool_id")),
                    g("name"),
                    g("tool_type"),
                    g("status"),
                    opt(g("input_content_hash")),
                    opt(g("output_content_hash")),
                    gn("input_length"),
                    gn("output_length"),
                    g("started_at"),
                    opt(g("completed_at")),
                    gf("duration_ms").map(|f| f as i64),
                    opt(g("error")),
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    pub fn insert_subagent(&self, v: &Value) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let n = c
            .execute(
                "INSERT OR IGNORE INTO subagent_relations (id, session_id, parent_model_call_id, child_session_id, relation, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    g("id"),
                    opt(g("session_id")),
                    opt(g("parent_model_call_id")),
                    g("child_session_id"),
                    g("relation"),
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    /// 根据 ULID 会话 id 查找规范键（批次内映射兜底）。
    pub fn resolve_session_key_by_id(&self, session_id: &str) -> Option<String> {
        let c = self.conn();
        c.query_row(
            "SELECT id FROM sessions WHERE source_session_id = ?1 OR id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .ok()
    }

    /// 批次内映射：ULID 会话 id → 规范键。
    pub fn session_key_map(&self, v: &Value) -> HashMap<String, String> {
        let mut m = HashMap::new();
        if let Some(sessions) = v.get("sessions").and_then(|s| s.as_array()) {
            for s in sessions {
                let node = s.get("node_id").and_then(|x| x.as_str()).unwrap_or("");
                let src = s
                    .get("source_session_id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let id = s.get("id").and_then(|x| x.as_str()).unwrap_or("");
                m.insert(id.to_string(), Self::session_key(node, src));
            }
        }
        m
    }
}

fn blake3_hex(s: &str) -> String {
    metria_core::model::ContentHash::hash_str(s)
        .as_str()
        .to_string()
}

fn opt(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn bool_i(v: Option<&Value>) -> i64 {
    match v.and_then(|x| x.as_bool()) {
        Some(true) => 1,
        _ => 0,
    }
}

fn opt_bool_i(v: Option<&Value>) -> Option<i64> {
    v.and_then(|x| x.as_bool()).map(|b| if b { 1 } else { 0 })
}
