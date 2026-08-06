//! 价格目录与规则仓储（HubDb 的 `impl` 拆分）。
//!
//! 与 `mod.rs` 同模块，可访问私有字段与辅助函数（`super::*`）。

use chrono::Utc;
use metria_storage::rusqlite::params;
use metria_storage::StorageError;
use serde_json::Value;

use super::HubDb;

impl HubDb {
    pub fn last_snapshot_etag(&self, catalog_id: &str) -> Option<String> {
        self.conn()
            .query_row(
                "SELECT etag FROM pricing_snapshots WHERE catalog_id = ?1 AND status='ok' ORDER BY fetched_at DESC LIMIT 1",
                [catalog_id],
                |r| r.get(0),
            )
            .ok()
            .flatten()
    }

    pub fn last_snapshot_hash(&self, catalog_id: &str) -> Option<String> {
        self.conn()
            .query_row(
                "SELECT content_hash FROM pricing_snapshots WHERE catalog_id = ?1 AND status='ok' ORDER BY fetched_at DESC LIMIT 1",
                [catalog_id],
                |r| r.get(0),
            )
            .ok()
            .flatten()
    }

    /// 写入新快照 + 其价格规则，停用该目录旧快照规则。
    pub fn upsert_snapshot_and_rules(
        &self,
        catalog_id: &str,
        kind: &str,
        etag: Option<String>,
        content_hash: String,
        rules: &[crate::catalog::RuleInput],
    ) -> Result<String, StorageError> {
        let c = self.conn();
        let snapshot_id = metria_core::model::Id::new().as_str().to_string();
        let now = Utc::now();
        let (source, channel) = match kind {
            "litellm" => ("litellm_catalog", "litellm"),
            "custom" => ("custom_http_catalog", "custom"),
            _ => ("openrouter_catalog", "openrouter"),
        };
        c.execute(
            "INSERT INTO pricing_snapshots (id, catalog_id, etag, fetched_at, effective_at, content_hash, record_count, status, created_at) VALUES (?1,?2,?3,?4,?4,?5,?6,'ok',?4)",
            params![
                snapshot_id,
                catalog_id,
                etag,
                now.to_rfc3339(),
                content_hash,
                rules.len() as i64,
            ],
        )
        .map_err(StorageError::from)?;
        // 停用旧快照规则
        c.execute(
            "UPDATE pricing_rules SET enabled = 0 WHERE snapshot_id IN (SELECT id FROM pricing_snapshots WHERE catalog_id = ?1)",
            [catalog_id],
        )
        .map_err(StorageError::from)?;
        {
            let mut stmt = c.prepare(
                "INSERT INTO pricing_rules (id, snapshot_id, source, channel, provider_pattern, model_pattern, client_pattern, input_price, output_price, cache_read_price, cache_write_price, reasoning_price, request_price, priority, enabled, metadata, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,'*',?7,?8,?9,?10,?11,?12,0,1,'{}',?13,?13)",
            )?;
            for r in rules {
                stmt.execute(params![
                    metria_core::model::Id::new().as_str().to_string(),
                    snapshot_id,
                    source,
                    channel,
                    r.provider,
                    r.model,
                    r.input,
                    r.output,
                    r.cache_read,
                    r.cache_write,
                    r.reasoning,
                    r.request,
                    now.to_rfc3339(),
                ])?;
            }
        }
        // 更新目录状态
        c.execute(
            "UPDATE pricing_catalogs SET last_refresh_at = ?1, last_success_at = ?1, last_error = NULL, updated_at = ?1 WHERE id = ?2",
            params![now.to_rfc3339(), catalog_id],
        )
        .map_err(StorageError::from)?;
        Ok(snapshot_id)
    }

    pub fn mark_catalog_error(&self, catalog_id: &str, err: &str) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "UPDATE pricing_catalogs SET last_refresh_at = ?1, last_error = ?2, updated_at = ?1 WHERE id = ?3",
                params![Utc::now().to_rfc3339(), err, catalog_id],
            )
            .map_err(StorageError::from)?;
        Ok(())
    }

    pub fn list_pricing_snapshots(&self) -> Vec<serde_json::Value> {
        let c = self.conn();
        let mut out = Vec::new();
        if let Ok(mut stmt) = c.prepare(
            "SELECT id, catalog_id, etag, fetched_at, content_hash, record_count, status FROM pricing_snapshots ORDER BY fetched_at DESC LIMIT 200",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "catalog_id": r.get::<_, String>(1)?,
                    "etag": r.get::<_, Option<String>>(2)?,
                    "fetched_at": r.get::<_, String>(3)?,
                    "content_hash": r.get::<_, String>(4)?,
                    "record_count": r.get::<_, i64>(5)?,
                    "status": r.get::<_, String>(6)?,
                }))
            }) {
                for row in rows.flatten() {
                    out.push(row);
                }
            }
        }
        out
    }

    /// 加载启用的全部价格规则（user + catalog）。
    pub fn load_all_rules(&self) -> Vec<metria_core::model::PricingRule> {
        let c = self.conn();
        let mut out = Vec::new();
        if let Ok(mut stmt) = c.prepare(
            "SELECT id, snapshot_id, source, channel, provider_pattern, model_pattern, client_pattern, input_price, output_price, cache_read_price, cache_write_price, reasoning_price, request_price, priority, enabled FROM pricing_rules WHERE enabled = 1",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                    r.get::<_, Option<i64>>(10)?,
                    r.get::<_, Option<i64>>(11)?,
                    r.get::<_, Option<i64>>(12)?,
                    r.get::<_, i64>(13)?,
                ))
            }) {
                for row in rows.flatten() {
                    let (id, snapshot_id, source, channel, provider, model, input, output, cr, cw, rea, request, priority) = row;
                    out.push(metria_core::model::PricingRule {
                        id: metria_core::model::Id::parse(&id).unwrap_or_default(),
                        snapshot_id: snapshot_id.and_then(|s| metria_core::model::Id::parse(&s).ok()),
                        source: match source.as_str() {
                            "user_override" => metria_core::model::PricingSource::UserOverride,
                            "openrouter_catalog" => metria_core::model::PricingSource::OpenRouterCatalog,
                            "litellm_catalog" => metria_core::model::PricingSource::LiteLlmCatalog,
                            "custom_http_catalog" => metria_core::model::PricingSource::CustomHttpCatalog,
                            _ => metria_core::model::PricingSource::UserOverride,
                        },
                        channel: match channel.as_str() {
                            "openrouter" => metria_core::model::PricingChannel::OpenRouter,
                            "litellm" => metria_core::model::PricingChannel::LiteLlm,
                            "custom" => metria_core::model::PricingChannel::Custom,
                            _ => metria_core::model::PricingChannel::VendorDirect,
                        },
                        provider_pattern: provider,
                        model_pattern: model,
                        client_pattern: "*".to_string(),
                        region_pattern: None,
                        service_tier: None,
                        currency: "usd".into(),
                        unit: "per_million_tokens".into(),
                        input_price: input,
                        output_price: output,
                        cache_read_price: cr,
                        cache_write_price: cw,
                        reasoning_price: rea,
                        request_price: request,
                        effective_from: None,
                        effective_to: None,
                        priority,
                        enabled: true,
                        metadata: serde_json::json!({}),
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                    });
                }
            }
        }
        out
    }

    /// 使用给定规则集对历史 usage 重新计价（保留旧 pricing_matches）。
    pub fn reprice_all(&self, engine: &metria_pricing::PricingEngine) -> Result<i64, StorageError> {
        let c = self.conn();
        let mut stmt = c
            .prepare(
                "SELECT event_id, model_normalized, provider_normalized, timestamp,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens
                 FROM usage_events",
            )
            .map_err(StorageError::from)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                ))
            })
            .map_err(StorageError::from)?;
        let mut repriced = 0i64;
        let mut upd = c
            .prepare(
                "UPDATE usage_events SET reported_cost_micro_usd = ?1, calculated_cost_micro_usd = ?2, estimated_cost_micro_usd = ?3, pricing_rule_id = ?4 WHERE event_id = ?5",
            )
            .map_err(StorageError::from)?;
        for row in rows.flatten() {
            let (event_id, model, provider, ts, input, output, cr, cw, rea) = row;
            let at = chrono::DateTime::parse_from_rfc3339(&ts)
                .map(|t| t.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let usage = metria_core::model::Usage {
                input,
                output,
                cache_read: cr,
                cache_write: cw,
                reasoning: rea,
            };
            let Ok(cost) = engine.compute(&usage, model.as_deref(), provider.as_deref(), at, None)
            else {
                continue;
            };
            if !cost.pricing_available {
                continue;
            }
            upd.execute(metria_storage::rusqlite::params![
                cost.reported_micro_usd,
                cost.calculated_micro_usd,
                cost.estimated_micro_usd,
                cost.rule_id,
                event_id,
            ])
            .map_err(StorageError::from)?;
            // 写入 pricing_match（保留历史，不覆盖）
            c.execute(
                "INSERT OR REPLACE INTO pricing_matches (id, usage_event_id, pricing_rule_id, pricing_snapshot_id, match_type, calculated_at, input_cost, output_cost, cache_read_cost, cache_write_cost, reasoning_cost, request_cost, total_cost) VALUES (?1,?2,?3,NULL,'reprice',?4,NULL,NULL,NULL,NULL,NULL,NULL,?5)",
                params![
                    metria_core::model::Id::new().as_str().to_string(),
                    event_id,
                    cost.rule_id,
                    Utc::now().to_rfc3339(),
                    cost.calculated_micro_usd.or(cost.estimated_micro_usd),
                ],
            )
            .map_err(StorageError::from)?;
            repriced += 1;
        }
        Ok(repriced)
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

    pub fn insert_pricing_rule(&self, v: &Value) -> Result<String, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| v.get(k).and_then(|x| x.as_i64());
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
        let client = if g("client_pattern").is_empty() {
            "*"
        } else {
            g("client_pattern")
        };
        c.execute(
            "INSERT INTO pricing_rules (id, snapshot_id, source, channel, provider_pattern, model_pattern, client_pattern, input_price, output_price, cache_read_price, cache_write_price, reasoning_price, request_price, priority, enabled, metadata, created_at, updated_at) VALUES (?1, NULL, 'user_override', 'vendor_direct', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, '{}', ?12, ?12)",
            params![
                &id,
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
        Ok(id)
    }

    /// 更新用户规则（编辑价格 / 优先级 / 生效区间 / 停用启用）。
    pub fn update_pricing_rule(&self, id: &str, v: &Value) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| v.get(k).and_then(|x| x.as_i64());
        let en = v.get("enabled").and_then(|x| x.as_bool());
        let n = c
            .execute(
                "UPDATE pricing_rules SET
                provider_pattern = COALESCE(NULLIF(?1,''), provider_pattern),
                model_pattern = COALESCE(NULLIF(?2,''), model_pattern),
                client_pattern = COALESCE(NULLIF(?3,''), client_pattern),
                input_price = COALESCE(?4, input_price),
                output_price = COALESCE(?5, output_price),
                cache_read_price = COALESCE(?6, cache_read_price),
                cache_write_price = COALESCE(?7, cache_write_price),
                reasoning_price = COALESCE(?8, reasoning_price),
                request_price = COALESCE(?9, request_price),
                priority = COALESCE(?10, priority),
                effective_from = COALESCE(NULLIF(?11,''), effective_from),
                effective_to = COALESCE(NULLIF(?12,''), effective_to),
                enabled = COALESCE(?13, enabled),
                updated_at = ?14
             WHERE id = ?15 AND source = 'user_override'",
                params![
                    g("provider_pattern"),
                    g("model_pattern"),
                    g("client_pattern"),
                    gn("input_price"),
                    gn("output_price"),
                    gn("cache_read_price"),
                    gn("cache_write_price"),
                    gn("reasoning_price"),
                    gn("request_price"),
                    gn("priority"),
                    g("effective_from"),
                    g("effective_to"),
                    en.map(|b| if b { 1 } else { 0 }),
                    Utc::now().to_rfc3339(),
                    id,
                ],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    /// 删除用户规则。
    pub fn delete_pricing_rule(&self, id: &str) -> Result<bool, StorageError> {
        let c = self.conn();
        let n = c
            .execute(
                "DELETE FROM pricing_rules WHERE id = ?1 AND source = 'user_override'",
                [id],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }
}
