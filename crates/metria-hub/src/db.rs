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

    /// 校验 collector token（仅存哈希）。
    pub fn verify_collector_token(&self, token: &str) -> Option<(String, String)> {
        let c = self.conn();
        let hash = blake3_hex(token);
        c.query_row(
            "SELECT t.collector_id, c.node_id FROM collector_tokens t JOIN collectors c ON c.id = t.collector_id WHERE t.token_hash = ?1 AND t.status = 'active'",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
    }

    pub fn upsert_collector_token(
        &self,
        collector_id: &str,
        token: &str,
    ) -> Result<(), StorageError> {
        let c = self.conn();
        let hash = blake3_hex(token);
        let now = Utc::now().to_rfc3339();
        c.execute(
            "INSERT OR IGNORE INTO collector_tokens (id, collector_id, token_hash, status, created_at) VALUES (?1, ?2, ?3, 'active', ?4)",
            params![format!("tok-{}", &hash[..12]), collector_id, hash, now],
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
    ) -> Result<(), StorageError> {
        let c = self.conn();
        let ts = now.to_rfc3339();
        c.execute(
            "UPDATE collectors SET last_heartbeat_at = ?1, spool_pending_events = ?2, spool_size_bytes = ?3, status = 'online', updated_at = ?1 WHERE id = ?4",
            params![ts, pending, size, collector_id],
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

    // ---------- Traffic Profile ----------

    /// 存储自动学习样本（幂等，按 source_hash 去重）。
    pub fn insert_traffic_profile_sample(&self, v: &Value) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let n = c
            .execute(
                "INSERT OR IGNORE INTO traffic_profile_samples (id, client, client_version, provider, model, content_profile, direction, token_count, payload_bytes, bytes_per_token, reconstruction_quality, source_hash, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    g("id"),
                    g("client"),
                    opt(g("client_version")),
                    opt(g("provider")),
                    opt(g("model")),
                    g("content_profile"),
                    g("direction"),
                    v.get("token_count").and_then(|x| x.as_i64()).unwrap_or(0),
                    v.get("payload_bytes").and_then(|x| x.as_i64()).unwrap_or(0),
                    v.get("bytes_per_token").and_then(|x| x.as_f64()).unwrap_or(0.0),
                    g("reconstruction_quality"),
                    g("source_hash"),
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    /// 从样本聚合出 learned profile（按 client+provider+model+direction+content_profile 分桶）。
    /// 只聚合样本数 >= min_samples 的桶，计算 P50/P75/P90。
    pub fn aggregate_learned_profiles(&self, min_samples: i64) -> Result<i64, StorageError> {
        let c = self.conn();
        let now = Utc::now().to_rfc3339();
        // 先清除旧 learned profile（重新学习）
        c.execute("DELETE FROM traffic_profiles WHERE source = 'learned'", [])
            .map_err(StorageError::from)?;

        let mut stmt = c
            .prepare(
                "SELECT client, COALESCE(provider,''), COALESCE(model,''), direction, COUNT(*) as n
                 FROM traffic_profile_samples GROUP BY client, provider, model, direction HAVING n >= ?1",
            )
            .map_err(StorageError::from)?;
        let rows = stmt
            .query_map([min_samples], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(StorageError::from)?;
        let mut created = 0i64;
        let buckets: Vec<(String, String, String, String, i64)> =
            rows.filter_map(|r| r.ok()).collect();
        for (client, provider, model, direction, n) in buckets {
            let input_bpt = self.bpt_percentile(&c, &client, &provider, &model, &direction, 50);
            let p75 = self.bpt_percentile(&c, &client, &provider, &model, &direction, 75);
            let p90 = self.bpt_percentile(&c, &client, &provider, &model, &direction, 90);
            let confidence = if n >= 100 {
                0.8
            } else if n >= 10 {
                0.55
            } else {
                0.35
            };
            c.execute(
                "INSERT INTO traffic_profiles (
                    id, source, client_pattern, client_version_pattern, provider_pattern, model_pattern,
                    content_profile, direction, streaming,
                    input_bytes_per_token_p50, input_bytes_per_token_p75, input_bytes_per_token_p90,
                    output_bytes_per_token_p50, output_bytes_per_token_p75, output_bytes_per_token_p90,
                    fixed_request_bytes, fixed_response_bytes, http_overhead_ratio, transport_overhead_ratio,
                    cache_read_transport_factor, cache_write_transport_factor,
                    sample_count, confidence, version, enabled, created_at, updated_at
                ) VALUES (?1,'learned',?2,'*',?3,?4,'unknown',?5,NULL,?6,?7,?8,?9,?10,?11,1024,128,0.05,0.10,0.8,1.0,?12,?13,1,1,?14,?14)",
                params![
                    metria_core::model::Id::new().as_str().to_string(),
                    client,
                    provider,
                    model,
                    direction,
                    input_bpt,
                    p75,
                    p90,
                    input_bpt,
                    p75,
                    p90,
                    n,
                    confidence,
                    now,
                ],
            )
            .map_err(StorageError::from)?;
            created += 1;
        }
        Ok(created)
    }

    /// 计算某桶的 bytes-per-token 分位数。
    fn bpt_percentile(
        &self,
        conn: &Connection,
        client: &str,
        provider: &str,
        model: &str,
        direction: &str,
        percentile: i64,
    ) -> f64 {
        let Ok(mut stmt) = conn.prepare(
            "SELECT bytes_per_token FROM traffic_profile_samples
             WHERE client = ?1 AND COALESCE(provider,'') = ?2 AND COALESCE(model,'') = ?3 AND direction = ?4",
        ) else {
            return 0.0;
        };
        let Ok(rows) = stmt.query_map(params![client, provider, model, direction], |r| {
            r.get::<_, f64>(0)
        }) else {
            return 0.0;
        };
        let mut vals: Vec<f64> = rows.filter_map(|r| r.ok()).collect();
        if vals.is_empty() {
            return 0.0;
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((vals.len() as f64 * percentile as f64 / 100.0).ceil() as usize)
            .saturating_sub(1)
            .min(vals.len() - 1);
        vals[idx]
    }

    pub fn list_traffic_profiles(&self, source: Option<&str>) -> Vec<serde_json::Value> {
        let c = self.conn();
        let mut out = Vec::new();
        let _ = source;
        if let Ok(mut stmt) =
            c.prepare("SELECT * FROM traffic_profiles ORDER BY source, created_at LIMIT 500")
        {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source": r.get::<_, String>(1)?,
                    "client_pattern": r.get::<_, String>(2)?,
                    "client_version_pattern": r.get::<_, String>(3)?,
                    "provider_pattern": r.get::<_, String>(4)?,
                    "model_pattern": r.get::<_, String>(5)?,
                    "content_profile": r.get::<_, String>(6)?,
                    "direction": r.get::<_, String>(7)?,
                    "input_bytes_per_token_p50": r.get::<_, f64>(9)?,
                    "output_bytes_per_token_p50": r.get::<_, f64>(12)?,
                    "fixed_request_bytes": r.get::<_, i64>(15)?,
                    "fixed_response_bytes": r.get::<_, i64>(16)?,
                    "http_overhead_ratio": r.get::<_, f64>(17)?,
                    "transport_overhead_ratio": r.get::<_, f64>(18)?,
                    "cache_read_transport_factor": r.get::<_, f64>(19)?,
                    "cache_write_transport_factor": r.get::<_, f64>(20)?,
                    "sample_count": r.get::<_, i64>(21)?,
                    "confidence": r.get::<_, f64>(22)?,
                    "version": r.get::<_, i64>(25)?,
                    "enabled": r.get::<_, i64>(26)? != 0,
                    "created_at": r.get::<_, String>(27)?,
                }))
            }) {
                for row in rows.flatten() {
                    out.push(row);
                }
            }
        }
        out
    }

    /// 新增用户 profile。
    pub fn insert_user_profile(&self, v: &Value) -> Result<(), StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let now = Utc::now().to_rfc3339();
        let id = metria_core::model::Id::new().as_str().to_string();
        let provider = if g("provider_pattern").is_empty() {
            ".*"
        } else {
            g("provider_pattern")
        };
        let model = if g("model_pattern").is_empty() {
            ".*"
        } else {
            g("model_pattern")
        };
        let direction = if g("direction").is_empty() {
            "request"
        } else {
            g("direction")
        };
        c.execute(
            "INSERT INTO traffic_profiles (
                id, source, client_pattern, client_version_pattern, provider_pattern, model_pattern,
                content_profile, direction, streaming,
                input_bytes_per_token_p50, input_bytes_per_token_p75, input_bytes_per_token_p90,
                output_bytes_per_token_p50, output_bytes_per_token_p75, output_bytes_per_token_p90,
                fixed_request_bytes, fixed_response_bytes, http_overhead_ratio, transport_overhead_ratio,
                cache_read_transport_factor, cache_write_transport_factor,
                sample_count, confidence, version, enabled, created_at, updated_at
            ) VALUES (?1,'user',?2,'*',?3,?4,'unknown',?5,NULL,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,0,?18,1,1,?19,?19)",
            params![
                id,
                g("client_pattern"),
                provider,
                model,
                direction,
                v.get("input_bytes_per_token").and_then(|x| x.as_f64()).unwrap_or(3.6),
                v.get("input_bytes_per_token").and_then(|x| x.as_f64()).unwrap_or(3.6),
                v.get("input_bytes_per_token").and_then(|x| x.as_f64()).unwrap_or(3.6),
                v.get("output_bytes_per_token").and_then(|x| x.as_f64()).unwrap_or(4.0),
                v.get("output_bytes_per_token").and_then(|x| x.as_f64()).unwrap_or(4.0),
                v.get("output_bytes_per_token").and_then(|x| x.as_f64()).unwrap_or(4.0),
                v.get("fixed_request_bytes").and_then(|x| x.as_i64()).unwrap_or(1024),
                v.get("fixed_response_bytes").and_then(|x| x.as_i64()).unwrap_or(128),
                v.get("http_overhead_ratio").and_then(|x| x.as_f64()).unwrap_or(0.05),
                v.get("transport_overhead_ratio").and_then(|x| x.as_f64()).unwrap_or(0.10),
                v.get("cache_read_transport_factor").and_then(|x| x.as_f64()).unwrap_or(0.8),
                v.get("cache_write_transport_factor").and_then(|x| x.as_f64()).unwrap_or(1.0),
                v.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.6),
                now,
            ],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    /// 删除用户 profile。
    pub fn delete_user_profile(&self, id: &str) -> Result<(), StorageError> {
        let c = self.conn();
        c.execute(
            "DELETE FROM traffic_profiles WHERE id = ?1 AND source = 'user'",
            [id],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    /// 加载 DB 中的 user + learned profile 为领域对象。
    pub fn load_traffic_profiles_parsed(&self) -> Vec<metria_core::model::TrafficProfile> {
        self.list_traffic_profiles(None)
            .into_iter()
            .map(|v| {
                let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let source = v.get("source").and_then(|x| x.as_str()).unwrap_or("user");
                let client = v
                    .get("client_pattern")
                    .and_then(|x| x.as_str())
                    .unwrap_or("*");
                let provider = v
                    .get("provider_pattern")
                    .and_then(|x| x.as_str())
                    .unwrap_or(".*");
                let model_pat = v
                    .get("model_pattern")
                    .and_then(|x| x.as_str())
                    .unwrap_or(".*");
                let direction = v
                    .get("direction")
                    .and_then(|x| x.as_str())
                    .unwrap_or("request");
                let in_bpt = v
                    .get("input_bytes_per_token_p50")
                    .and_then(|x| x.as_f64())
                    .unwrap_or(3.6) as f32;
                let out_bpt = v
                    .get("output_bytes_per_token_p50")
                    .and_then(|x| x.as_f64())
                    .unwrap_or(4.0) as f32;
                metria_core::model::TrafficProfile {
                    id: metria_core::model::Id::parse(id).unwrap_or_default(),
                    source: match source {
                        "user" => metria_core::model::TrafficProfileSource::User,
                        "learned" => metria_core::model::TrafficProfileSource::Learned,
                        _ => metria_core::model::TrafficProfileSource::User,
                    },
                    client_pattern: client.to_string(),
                    client_version_pattern: "*".to_string(),
                    provider_pattern: provider.to_string(),
                    model_pattern: model_pat.to_string(),
                    content_profile: metria_core::model::ContentProfile::Unknown,
                    direction: if direction == "response" {
                        metria_core::model::TrafficDirection::Response
                    } else {
                        metria_core::model::TrafficDirection::Request
                    },
                    streaming: None,
                    context_transport_mode: metria_core::model::ContextTransportMode::Unknown,
                    input_bytes_per_token_p50: in_bpt,
                    input_bytes_per_token_p75: in_bpt,
                    input_bytes_per_token_p90: in_bpt,
                    output_bytes_per_token_p50: out_bpt,
                    output_bytes_per_token_p75: out_bpt,
                    output_bytes_per_token_p90: out_bpt,
                    fixed_request_bytes: v
                        .get("fixed_request_bytes")
                        .and_then(|x| x.as_i64())
                        .unwrap_or(1024),
                    fixed_response_bytes: v
                        .get("fixed_response_bytes")
                        .and_then(|x| x.as_i64())
                        .unwrap_or(128),
                    http_overhead_ratio: v
                        .get("http_overhead_ratio")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(0.05) as f32,
                    transport_overhead_ratio: v
                        .get("transport_overhead_ratio")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(0.10) as f32,
                    cache_read_transport_factor: v
                        .get("cache_read_transport_factor")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(0.8) as f32,
                    cache_write_transport_factor: v
                        .get("cache_write_transport_factor")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(1.0) as f32,
                    sample_count: v.get("sample_count").and_then(|x| x.as_u64()).unwrap_or(0),
                    confidence: v.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.5) as f32,
                    effective_from: None,
                    effective_to: None,
                    version: v.get("version").and_then(|x| x.as_i64()).unwrap_or(1),
                    enabled: v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                }
            })
            .collect()
    }

    /// 使用当前 profile 对历史调用重新估算（插入新版本，不覆盖旧版）。
    pub fn reestimate_calls(&self, model_filter: Option<&str>) -> Result<i64, StorageError> {
        // 先取 profile（内部加锁），避免持有 c 时嵌套加锁
        let profiles = self.load_traffic_profiles_parsed();
        let c = self.conn();
        let mut stmt = c.prepare(
            "SELECT id, client_id, provider_normalized, model_normalized, started_at,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens
             FROM model_calls
             WHERE (input_tokens IS NOT NULL OR output_tokens IS NOT NULL)
               AND (?1 = '' OR model_normalized = ?1)",
        ).map_err(StorageError::from)?;
        let rows = stmt
            .query_map(params![model_filter.unwrap_or("")], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                ))
            })
            .map_err(StorageError::from)?;

        let mut reestimated = 0i64;
        for row in rows.flatten() {
            let (call_id, client, provider, model, started, input, output, cr, cw, rea) = row;
            let est = metria_traffic::estimate_with_candidates(
                &metria_traffic::EstimateInput {
                    client: &client,
                    provider: provider.as_deref(),
                    model: model.as_deref(),
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_tokens: cr,
                    cache_write_tokens: cw,
                    reasoning_tokens: rea,
                    streaming: true,
                    request_text: None,
                    response_text: None,
                    request_reconstruction_quality: metria_core::model::ReconstructionQuality::None,
                    response_reconstruction_quality:
                        metria_core::model::ReconstructionQuality::None,
                    context_transport_mode: metria_core::model::ContextTransportMode::Unknown,
                    cache_transport_behavior: metria_core::model::CacheTransportBehavior::Unknown,
                },
                &profiles,
            );
            let Ok(out) = est else { continue };
            let te_id = metria_core::model::Id::new();
            // 写入新版本 traffic estimate
            let n = c
                .execute(
                    "INSERT OR IGNORE INTO traffic_estimates (
                        id, model_call_id, node_id, client_id, session_id, turn_id, provider, model,
                        request_payload_bytes, response_payload_bytes, estimated_request_http_bytes, estimated_response_http_bytes,
                        estimated_request_wire_bytes, estimated_response_wire_bytes, estimated_total_wire_bytes,
                        lower_bound_bytes, upper_bound_bytes, estimation_source, context_transport_mode, cache_transport_behavior,
                        request_reconstruction_quality, response_reconstruction_quality, profile_id, profile_version, confidence,
                        calculated_at, created_at
                    ) VALUES (?1,?2,'',?3,NULL,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'unknown','unknown','none','none',NULL,NULL,?16,?17,?18)",
                    params![
                        te_id.as_str().to_string(),
                        call_id,
                        client,
                        provider,
                        model,
                        out.request_payload_bytes,
                        out.response_payload_bytes,
                        out.estimated_request_wire_bytes,
                        out.estimated_response_wire_bytes,
                        out.estimated_request_wire_bytes,
                        out.estimated_response_wire_bytes,
                        out.estimated_total_wire_bytes,
                        out.lower_bound_bytes,
                        out.upper_bound_bytes,
                        format!("{:?}", out.estimation_source).to_ascii_lowercase(),
                        out.confidence,
                        started,
                        Utc::now().to_rfc3339(),
                    ],
                )
                .map_err(StorageError::from)?;
            if n > 0 {
                // 指向新版本估算
                c.execute(
                    "UPDATE model_calls SET traffic_estimate_id = ?1 WHERE id = ?2",
                    params![te_id.as_str().to_string(), call_id],
                )
                .map_err(StorageError::from)?;
                reestimated += 1;
            }
        }
        Ok(reestimated)
    }

    // ---------- 价格 ----------

    pub fn list_pricing_catalogs(&self) -> Vec<serde_json::Value> {
        let c = self.conn();
        let mut out = Vec::new();
        if let Ok(mut stmt) = c.prepare(
            "SELECT id, name, kind, enabled, priority, base_url, last_success_at, last_error, created_at FROM pricing_catalogs ORDER BY priority",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "kind": r.get::<_, String>(2)?,
                    "enabled": r.get::<_, i64>(3)? != 0,
                    "priority": r.get::<_, i64>(4)?,
                    "base_url": r.get::<_, Option<String>>(5)?,
                    "last_success_at": r.get::<_, Option<String>>(6)?,
                    "last_error": r.get::<_, Option<String>>(7)?,
                    "created_at": r.get::<_, String>(8)?,
                }))
            }) {
                for row in rows.flatten() {
                    out.push(row);
                }
            }
        }
        out
    }

    pub fn list_pricing_rules(&self) -> Vec<serde_json::Value> {
        let c = self.conn();
        let mut out = Vec::new();
        if let Ok(mut stmt) = c.prepare(
            "SELECT id, source, channel, provider_pattern, model_pattern, client_pattern, input_price, output_price, cache_read_price, cache_write_price, reasoning_price, request_price, priority, enabled, effective_from, effective_to, metadata, created_at FROM pricing_rules ORDER BY priority DESC",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source": r.get::<_, String>(1)?,
                    "channel": r.get::<_, String>(2)?,
                    "provider_pattern": r.get::<_, String>(3)?,
                    "model_pattern": r.get::<_, String>(4)?,
                    "client_pattern": r.get::<_, String>(5)?,
                    "input_price": r.get::<_, Option<i64>>(6)?,
                    "output_price": r.get::<_, Option<i64>>(7)?,
                    "cache_read_price": r.get::<_, Option<i64>>(8)?,
                    "cache_write_price": r.get::<_, Option<i64>>(9)?,
                    "reasoning_price": r.get::<_, Option<i64>>(10)?,
                    "request_price": r.get::<_, Option<i64>>(11)?,
                    "priority": r.get::<_, i64>(12)?,
                    "enabled": r.get::<_, i64>(13)? != 0,
                    "effective_from": r.get::<_, Option<String>>(14)?,
                    "effective_to": r.get::<_, Option<String>>(15)?,
                    "metadata": r.get::<_, String>(16)?,
                    "created_at": r.get::<_, String>(17)?,
                }))
            }) {
                for row in rows.flatten() {
                    out.push(row);
                }
            }
        }
        out
    }

    pub fn insert_pricing_rule(&self, v: &Value) -> Result<(), StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| v.get(k).and_then(|x| x.as_i64());
        let now = Utc::now().to_rfc3339();
        let provider = if g("provider_pattern").is_empty() {
            ".*"
        } else {
            g("provider_pattern")
        };
        let model = if g("model_pattern").is_empty() {
            ".*"
        } else {
            g("model_pattern")
        };
        let client = if g("client_pattern").is_empty() {
            "*"
        } else {
            g("client_pattern")
        };
        c.execute(
            "INSERT INTO pricing_rules (id, snapshot_id, source, channel, provider_pattern, model_pattern, client_pattern, input_price, output_price, cache_read_price, cache_write_price, reasoning_price, request_price, priority, enabled, metadata, created_at, updated_at) VALUES (?1, NULL, 'user_override', 'vendor_direct', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, '{}', ?12, ?12)",
            params![
                metria_core::model::Id::new().as_str().to_string(),
                provider,
                model,
                client,
                gn("input_price"),
                gn("output_price"),
                gn("cache_read_price"),
                gn("cache_write_price"),
                gn("reasoning_price"),
                gn("request_price"),
                gn("priority").unwrap_or(0),
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
