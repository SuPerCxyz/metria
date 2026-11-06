//! 价格目录与规则仓储（HubDb 的 `impl` 拆分）。
//!
//! 与 `mod.rs` 同模块，可访问私有字段与辅助函数（`super::*`）。

use chrono::{DateTime, Utc};
use metria_storage::rusqlite::params;
use metria_storage::StorageError;
use serde_json::Value;

use super::HubDb;

const REPRICE_BATCH_SIZE: i64 = 512;

fn parse_rule_time(value: Option<String>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc))
}

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
        // 只保留最新快照：先删除该目录的历史快照与规则（不保留旧计价规则）。
        c.execute(
            "DELETE FROM pricing_rules WHERE snapshot_id IN (SELECT id FROM pricing_snapshots WHERE catalog_id = ?1)",
            [catalog_id],
        )
        .map_err(StorageError::from)?;
        c.execute(
            "DELETE FROM pricing_snapshots WHERE catalog_id = ?1",
            [catalog_id],
        )
        .map_err(StorageError::from)?;
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
            "SELECT id, snapshot_id, source, channel, provider_pattern, model_pattern, client_pattern, input_price, output_price, cache_read_price, cache_write_price, reasoning_price, request_price, priority, enabled, effective_from, effective_to, metadata FROM pricing_rules WHERE enabled = 1",
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
                    r.get::<_, Option<String>>(15)?,
                    r.get::<_, Option<String>>(16)?,
                    r.get::<_, String>(17)?,
                ))
            }) {
                for row in rows.flatten() {
                    let (id, snapshot_id, source, channel, provider, model, input, output, cr, cw, rea, request, priority, effective_from, effective_to, metadata) = row;
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
                        effective_from: parse_rule_time(effective_from),
                        effective_to: parse_rule_time(effective_to),
                        priority,
                        enabled: true,
                        metadata: serde_json::from_str(&metadata)
                            .unwrap_or_else(|_| serde_json::json!({})),
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                    });
                }
            }
        }
        out
    }

    /// 使用给定规则集对历史 usage 重新计价（每个用量行保留最新一条 pricing_matches）。
    ///
    /// `only_unpriced = true` 时仅处理尚无费用的事件（后台周期增量），
    /// 否则全量重算。完成后把费用传播到关联的 model_calls，保持列表/详情一致。
    pub fn reprice_all(
        &self,
        engine: &metria_pricing::PricingEngine,
        only_unpriced: bool,
    ) -> Result<i64, StorageError> {
        // TEMP 表 reprice_changed 是连接级共享状态；catalog sync 与管理员重计价
        // 并发时会互相 DELETE/覆盖变化集。与全量重建共用 guard，串行化整个流程。
        let _guard = self.rebuild_guard();
        // 记录本轮实际变化的用量事件；传播与会话刷新只针对这些行。
        // 使用同一写连接上的 TEMP 表，避免把百万级 event id 堆在内存里。
        {
            let c = self.conn();
            c.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS reprice_changed (event_id TEXT PRIMARY KEY); \
                 DELETE FROM reprice_changed;",
            )
            .map_err(StorageError::from)?;
        }

        let mut repriced = 0i64;
        let mut last_event_id = String::new();
        loop {
            let batch = {
                let c = self.conn();
                let query = if only_unpriced {
                    // 走 idx_usage_unpriced(event_id) 部分索引，只访问未计价候选。
                    "SELECT event_id, model_normalized, provider_normalized, timestamp,
                            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                            reasoning_tokens, reported_cost_micro_usd, calculated_cost_micro_usd,
                            estimated_cost_micro_usd, pricing_rule_id
                     FROM usage_events
                     WHERE event_id > ?1
                       AND reported_cost_micro_usd IS NULL
                       AND calculated_cost_micro_usd IS NULL
                       AND estimated_cost_micro_usd IS NULL
                     ORDER BY event_id LIMIT ?2"
                } else {
                    "SELECT event_id, model_normalized, provider_normalized, timestamp,
                            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                            reasoning_tokens, reported_cost_micro_usd, calculated_cost_micro_usd,
                            estimated_cost_micro_usd, pricing_rule_id
                     FROM usage_events
                     WHERE event_id > ?1
                     ORDER BY event_id LIMIT ?2"
                };
                let mut stmt = c.prepare(query).map_err(StorageError::from)?;
                let rows = stmt
                    .query_map(params![last_event_id, REPRICE_BATCH_SIZE], |r| {
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
                            r.get::<_, Option<i64>>(9)?,
                            r.get::<_, Option<i64>>(10)?,
                            r.get::<_, Option<i64>>(11)?,
                            r.get::<_, Option<String>>(12)?,
                        ))
                    })
                    .map_err(StorageError::from)?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(StorageError::from)?
            };
            let Some(last) = batch.last().map(|row| row.0.clone()) else {
                break;
            };
            last_event_id = last;

            let mut updates = Vec::with_capacity(batch.len());
            for (
                event_id,
                model,
                provider,
                ts,
                input,
                output,
                cr,
                cw,
                rea,
                old_reported,
                old_calculated,
                old_estimated,
                old_rule,
            ) in batch
            {
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
                let computed = engine.compute(
                    &usage,
                    model.as_deref(),
                    provider.as_deref(),
                    at,
                    old_reported,
                );
                // 规则移除或计算溢出时保留 reported（如果存在），清空派生口径；
                // 正常命中时写入 reported/calculated/estimated 三口径。
                // 只有值实际变化才写库与传播。
                let (reported, calculated, estimated, rule_id, match_total) =
                    match computed.ok().filter(|cost| cost.pricing_available) {
                        Some(cost) => (
                            cost.reported_micro_usd,
                            cost.calculated_micro_usd,
                            cost.estimated_micro_usd,
                            cost.rule_id,
                            cost.calculated_micro_usd.or(cost.estimated_micro_usd),
                        ),
                        None => (old_reported, None, None, None, None),
                    };
                if reported == old_reported
                    && calculated == old_calculated
                    && estimated == old_estimated
                    && rule_id == old_rule
                {
                    continue;
                }
                updates.push((
                    event_id,
                    reported,
                    calculated,
                    estimated,
                    rule_id,
                    match_total,
                ));
            }

            if updates.is_empty() {
                continue;
            }
            let mut c = self.conn();
            let tx = c.transaction()?;
            {
                let update_sql = if only_unpriced {
                    "UPDATE usage_events SET reported_cost_micro_usd = ?1, \
                     calculated_cost_micro_usd = ?2, estimated_cost_micro_usd = ?3, \
                     pricing_rule_id = ?4 WHERE event_id = ?5 \
                     AND reported_cost_micro_usd IS NULL \
                     AND calculated_cost_micro_usd IS NULL \
                     AND estimated_cost_micro_usd IS NULL"
                } else {
                    "UPDATE usage_events SET reported_cost_micro_usd = ?1, \
                     calculated_cost_micro_usd = ?2, estimated_cost_micro_usd = ?3, \
                     pricing_rule_id = ?4 WHERE event_id = ?5"
                };
                let mut upd = tx.prepare(update_sql).map_err(StorageError::from)?;
                let mut match_stmt = tx
                    .prepare(
                        "INSERT INTO pricing_matches (id, usage_event_id, pricing_rule_id, \
                         pricing_snapshot_id, match_type, calculated_at, input_cost, output_cost, \
                         cache_read_cost, cache_write_cost, reasoning_cost, request_cost, total_cost) \
                         VALUES (?1,?2,?3,NULL,'reprice',?4,NULL,NULL,NULL,NULL,NULL,NULL,?5)",
                    )
                    .map_err(StorageError::from)?;
                let mut changed = tx
                    .prepare("INSERT OR IGNORE INTO reprice_changed (event_id) VALUES (?1)")
                    .map_err(StorageError::from)?;
                for (event_id, reported, calculated, estimated, rule_id, match_total) in updates {
                    let updated = upd
                        .execute(params![
                            reported, calculated, estimated, &rule_id, &event_id
                        ])
                        .map_err(StorageError::from)?;
                    if updated == 0 {
                        // 仅未计价路径会出现：并发同步已抢先写入，保持既有结果。
                        continue;
                    }
                    if match_total.is_some() {
                        // 只有实际命中价格时才写新的匹配记录；规则移除不伪造费用。
                        match_stmt
                            .execute(params![
                                metria_core::model::Id::new().as_str().to_string(),
                                &event_id,
                                &rule_id,
                                Utc::now().to_rfc3339(),
                                match_total,
                            ])
                            .map_err(StorageError::from)?;
                    }
                    changed.execute([&event_id]).map_err(StorageError::from)?;
                    repriced += 1;
                }
            }
            tx.commit()?;
        }

        // 无任何费用变化时不触碰 model_calls、不刷新会话汇总、不写新的匹配记录。
        if repriced == 0 {
            return Ok(0);
        }

        let c = self.conn();
        // 兼容旧数据：只对发生变化的 usage 补齐 model_calls.usage_event_id。
        c.execute(
            "UPDATE model_calls SET usage_event_id = (
                SELECT event_id FROM usage_events
                WHERE model_call_id = model_calls.id
                ORDER BY timestamp, event_id LIMIT 1
             )
             WHERE usage_event_id IS NULL
               AND EXISTS (
                   SELECT 1 FROM usage_events u
                   JOIN reprice_changed rc ON rc.event_id = u.event_id
                   WHERE u.model_call_id = model_calls.id
               )",
            [],
        )
        .map_err(StorageError::from)?;
        // 只把费用传播到本轮实际变化的用量事件关联行；未变化行保持原值。
        c.execute(
            "UPDATE model_calls SET
                reported_cost_micro_usd = (SELECT reported_cost_micro_usd FROM usage_events WHERE event_id = model_calls.usage_event_id),
                calculated_cost_micro_usd = (SELECT calculated_cost_micro_usd FROM usage_events WHERE event_id = model_calls.usage_event_id),
                estimated_cost_micro_usd = (SELECT estimated_cost_micro_usd FROM usage_events WHERE event_id = model_calls.usage_event_id)
             WHERE usage_event_id IN (SELECT event_id FROM reprice_changed)",
            [],
        )
        .map_err(StorageError::from)?;
        // 只为受影响会话重算汇总，避免每次同步刷新全部 1,836 个会话。
        let session_ids: Vec<String> = {
            let mut stmt = c
                .prepare(
                    "SELECT DISTINCT mc.session_id FROM model_calls mc
                     JOIN reprice_changed rc ON rc.event_id = mc.usage_event_id
                     WHERE mc.session_id != ''",
                )
                .map_err(StorageError::from)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(StorageError::from)?
                .filter_map(|row| row.ok())
                .collect::<Vec<_>>();
            rows
        };
        for session_id in session_ids {
            self.refresh_session_agg(&c, &session_id)?;
        }
        Ok(repriced)
    }

    // ---------- 价格 ----------

    pub fn list_pricing_catalogs(&self) -> Vec<serde_json::Value> {
        let c = self.conn();
        let mut out = Vec::new();
        if let Ok(mut stmt) = c.prepare(
            "SELECT id, name, kind, enabled, priority, base_url, authentication_type, last_success_at, last_error, created_at FROM pricing_catalogs ORDER BY priority",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "kind": r.get::<_, String>(2)?,
                    "enabled": r.get::<_, i64>(3)? != 0,
                    "priority": r.get::<_, i64>(4)?,
                    "base_url": r.get::<_, Option<String>>(5)?,
                    "authentication_type": r.get::<_, Option<String>>(6)?,
                    "last_success_at": r.get::<_, Option<String>>(7)?,
                    "last_error": r.get::<_, Option<String>>(8)?,
                    "created_at": r.get::<_, String>(9)?,
                }))
            }) {
                for row in rows.flatten() {
                    out.push(row);
                }
            }
        }
        out
    }

    /// 更新价格目录（计费模板链接 / 启用状态），供 Web 配置。
    pub fn update_pricing_catalog(&self, id: &str, v: &Value) -> Result<bool, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let n = c
            .execute(
                "UPDATE pricing_catalogs SET
                    base_url = COALESCE(NULLIF(?1,''), base_url),
                    authentication_type = COALESCE(NULLIF(?2,''), authentication_type),
                    enabled = COALESCE(?3, enabled),
                    updated_at = ?4
                 WHERE id = ?5",
                params![
                    g("base_url"),
                    g("authentication_type"),
                    v.get("enabled")
                        .and_then(|x| x.as_bool())
                        .map(|b| if b { 1 } else { 0 }),
                    Utc::now().to_rfc3339(),
                    id,
                ],
            )
            .map_err(StorageError::from)?;
        Ok(n > 0)
    }

    pub fn list_pricing_rules(&self) -> Vec<serde_json::Value> {
        let c = self.conn();
        let mut out = Vec::new();
        if let Ok(mut stmt) = c.prepare(
            "SELECT id, source, channel, provider_pattern, model_pattern, client_pattern, input_price, output_price, cache_read_price, cache_write_price, reasoning_price, request_price, priority, enabled, effective_from, effective_to, metadata, created_at FROM pricing_rules WHERE enabled = 1 OR source = 'user_override' ORDER BY priority DESC",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                let metadata: String = r.get(16)?;
                let metadata_value =
                    serde_json::from_str::<Value>(&metadata).unwrap_or_else(|_| serde_json::json!({}));
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
                    "price_equivalent_to": metadata_value.get("price_equivalent_to").and_then(Value::as_str),
                    "price_equivalent_missing_as_free": metadata_value
                        .get("price_equivalent_missing_as_free")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
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

    /// 返回价格关联表单的模型选项：平台实际使用过的模型与启用目录中的模型。
    pub fn pricing_model_options(&self) -> serde_json::Value {
        let c = self.conn();
        let used_models = if let Ok(mut stmt) = c.prepare(
            "SELECT model_normalized, COUNT(*)
             FROM model_calls
             WHERE model_normalized IS NOT NULL AND TRIM(model_normalized) != ''
             GROUP BY model_normalized
             ORDER BY COUNT(*) DESC, model_normalized COLLATE NOCASE",
        ) {
            stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "model": r.get::<_, String>(0)?,
                    "calls": r.get::<_, i64>(1)?,
                }))
            })
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        let catalog_models = if let Ok(mut stmt) = c.prepare(
            "SELECT model_pattern, source, provider_pattern,
                    CASE WHEN input_price IS NOT NULL OR output_price IS NOT NULL
                              OR cache_read_price IS NOT NULL OR cache_write_price IS NOT NULL
                              OR reasoning_price IS NOT NULL OR request_price IS NOT NULL
                         THEN 1 ELSE 0 END
             FROM pricing_rules
             WHERE enabled = 1
               AND source IN ('openrouter_catalog', 'litellm_catalog', 'custom_http_catalog')
               AND TRIM(model_pattern) != ''
             ORDER BY model_pattern COLLATE NOCASE, source",
        ) {
            stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "model_pattern": r.get::<_, String>(0)?,
                    "source": r.get::<_, String>(1)?,
                    "provider_pattern": r.get::<_, String>(2)?,
                    "price_available": r.get::<_, i64>(3)? != 0,
                }))
            })
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        serde_json::json!({
            "used_models": used_models,
            "catalog_models": catalog_models,
        })
    }

    pub fn insert_pricing_rule(&self, v: &Value) -> Result<String, StorageError> {
        let c = self.conn();
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let gn = |k: &str| {
            v.get(k).and_then(|x| {
                x.as_i64()
                    .or_else(|| x.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
            })
        };
        let now = Utc::now().to_rfc3339();
        let id = metria_core::model::Id::new().as_str().to_string();
        let provider = if g("provider_pattern").is_empty() {
            "*"
        } else {
            g("provider_pattern")
        };
        let model = if g("model_pattern").is_empty() {
            "*"
        } else {
            g("model_pattern")
        };
        let client = if g("client_pattern").is_empty() {
            "*"
        } else {
            g("client_pattern")
        };
        let effective_from = v
            .get("effective_from")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let effective_to = v
            .get("effective_to")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let metadata = v
            .get("price_equivalent_to")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|target| {
                serde_json::json!({
                    "price_equivalent_to": target,
                    "price_equivalent_missing_as_free": v
                        .get("price_equivalent_missing_as_free")
                        .and_then(|value| value.as_bool())
                        == Some(true),
                })
                .to_string()
            })
            .unwrap_or_else(|| "{}".to_string());
        c.execute(
            "INSERT INTO pricing_rules (id, snapshot_id, source, channel, provider_pattern, model_pattern, client_pattern, input_price, output_price, cache_read_price, cache_write_price, reasoning_price, request_price, effective_from, effective_to, priority, enabled, metadata, created_at, updated_at) VALUES (?1, NULL, 'user_override', 'vendor_direct', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1, ?14, ?15, ?15)",
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
                effective_from,
                effective_to,
                gn("priority").unwrap_or(0),
                metadata,
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
        let gn = |k: &str| {
            v.get(k).and_then(|x| {
                x.as_i64()
                    .or_else(|| x.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
            })
        };
        let en = v.get("enabled").and_then(|x| x.as_bool());
        let metadata = v
            .get("price_equivalent_to")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|target| {
                serde_json::json!({
                    "price_equivalent_to": target,
                    "price_equivalent_missing_as_free": v
                        .get("price_equivalent_missing_as_free")
                        .and_then(|value| value.as_bool())
                        == Some(true),
                })
                .to_string()
            });
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
                metadata = COALESCE(?13, metadata),
                enabled = COALESCE(?14, enabled),
                updated_at = ?15
             WHERE id = ?16 AND source = 'user_override'",
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
                    metadata,
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
