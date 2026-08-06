//! Hub 存储层：连接管理、repository 方法与 ingest 落库。
//!
//! 会话引用统一为规范键 `{node_id}:{source_session_id}`，保证幂等与跨表 join。
//! 所有事件 insert 方法返回 `bool`（是否新插入），用于幂等与 rollup 判定。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use metria_storage::rusqlite::{params, Connection};
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

    pub fn schema_version(&self) -> Result<i64, StorageError> {
        let c = self.conn();
        metria_storage::migrations::current_version(&c)
    }

    pub fn count(&self, table: &str) -> i64 {
        self.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0)
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
        Ok(n > 0)
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
        Ok(n > 0)
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
