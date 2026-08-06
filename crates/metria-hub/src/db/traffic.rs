//! Traffic Profile 与重新估算仓储（HubDb 的 `impl` 拆分）。
//!
//! 与 `mod.rs` 同模块，可访问私有字段与辅助函数（`super::*`）。

use chrono::Utc;
use metria_storage::rusqlite::{params, Connection};
use metria_storage::StorageError;
use serde_json::Value;

use super::{opt, HubDb};

impl HubDb {
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
}
