//! 外部价格目录同步：OpenRouter / LiteLLM / Custom HTTP。
//!
//! 约束：来源与快照必须保存；失败时继续使用最后一个有效快照；ETag 去重。

use crate::db::HubDb;
use serde_json::Value;

/// 从当前启用规则构造价格引擎；内置规则由引擎自身提供。
pub fn pricing_engine(db: &HubDb) -> metria_pricing::PricingEngine {
    let mut engine = metria_pricing::PricingEngine::new();
    for rule in db.load_all_rules() {
        engine.add_rule(rule);
    }
    engine
}

/// 在 usage 落库前即时补齐费用。无可靠价格时保持 null，不硬造 0。
pub fn price_usage_payload(
    engine: &metria_pricing::PricingEngine,
    payload: &mut Value,
) -> Result<(), String> {
    let usage_value = payload.get("usage").cloned().unwrap_or(Value::Null);
    let token = |key: &str| usage_value.get(key).and_then(Value::as_i64);
    let usage = metria_core::model::Usage {
        input: token("input"),
        output: token("output"),
        cache_read: token("cache_read"),
        cache_write: token("cache_write"),
        reasoning: token("reasoning"),
    };
    let existing = payload
        .get("cost")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if [
        "reported_micro_usd",
        "calculated_micro_usd",
        "estimated_micro_usd",
    ]
    .iter()
    .any(|key| existing.get(*key).and_then(Value::as_i64).is_some())
    {
        return Ok(());
    }
    let reported = existing.get("reported_micro_usd").and_then(Value::as_i64);
    let model = payload
        .get("model_normalized")
        .or_else(|| payload.get("model_raw"))
        .and_then(Value::as_str);
    let provider = payload
        .get("provider_normalized")
        .or_else(|| payload.get("provider_raw"))
        .and_then(Value::as_str);
    let at = payload
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|ts| ts.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    let cost = engine
        .compute(&usage, model, provider, at, reported)
        .map_err(|e| e.to_string())?;
    if !cost.pricing_available {
        return Ok(());
    }
    payload["cost"] = serde_json::json!({
        "reported_micro_usd": cost.reported_micro_usd,
        "calculated_micro_usd": cost.calculated_micro_usd,
        "estimated_micro_usd": cost.estimated_micro_usd,
        "pricing_rule_id": cost.rule_id,
        "pricing_snapshot_id": cost.snapshot_id,
    });
    Ok(())
}

/// 目录定义。
#[derive(Debug, Clone)]
pub struct CatalogDef {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub auth: Option<String>,
}

/// 同步结果。
#[derive(Debug)]
pub struct SyncResult {
    pub fetched: bool,
    pub rules: usize,
    pub etag: Option<String>,
}

/// 单条价格规则输入（价格单位：微美元 / 百万 token）。
#[derive(Debug, Clone)]
pub struct RuleInput {
    pub provider: String,
    pub model: String,
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    pub reasoning: Option<i64>,
    pub request: Option<i64>,
}

/// 执行一次目录同步。
pub fn sync_catalog(db: &HubDb, catalog: &CatalogDef) -> Result<SyncResult, String> {
    let last_etag = db.last_snapshot_etag(&catalog.id);
    let mut req = ureq::get(&catalog.url).timeout(std::time::Duration::from_secs(30));
    if let Some(etag) = &last_etag {
        req = req.set("If-None-Match", etag);
    }
    if let Some(auth) = &catalog.auth {
        if !auth.is_empty() {
            req = req.set("Authorization", &format!("Bearer {auth}"));
        }
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(304, _)) => {
            // 未修改：继续使用现有快照
            return Ok(SyncResult {
                fetched: false,
                rules: 0,
                etag: last_etag,
            });
        }
        Err(e) => return Err(format!("请求失败: {e}")),
    };
    let etag = resp.header("ETag").map(|s| s.to_string());
    let body = resp
        .into_string()
        .map_err(|e| format!("读取响应失败: {e}"))?;
    let hash = metria_core::model::ContentHash::hash_str(&body)
        .as_str()
        .to_string();
    if db.last_snapshot_hash(&catalog.id).as_deref() == Some(hash.as_str()) {
        return Ok(SyncResult {
            fetched: false,
            rules: 0,
            etag,
        });
    }

    let rules = match catalog.kind.as_str() {
        "litellm" => parse_litellm(&body)?,
        _ => parse_openrouter(&body)?,
    };

    db.upsert_snapshot_and_rules(&catalog.id, &catalog.kind, etag.clone(), hash, &rules)
        .map_err(|e| e.to_string())?;
    Ok(SyncResult {
        fetched: true,
        rules: rules.len(),
        etag,
    })
}

/// 解析 OpenRouter /models 响应。
fn parse_openrouter(body: &str) -> Result<Vec<RuleInput>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("缺少 data 数组")?;
    let mut out = Vec::new();
    for m in data {
        let Some(id) = m.get("id").and_then(|x| x.as_str()) else {
            continue;
        };
        let pricing = m.get("pricing").cloned().unwrap_or(Value::Null);
        let p = |k: &str| {
            pricing
                .get(k)
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .map(per_token_to_per_m_micro)
        };
        // id 形如 "anthropic/claude-sonnet-4"
        let (provider, model) = match id.split_once('/') {
            Some((p, m)) => (p.to_string(), m.to_string()),
            None => ("".to_string(), id.to_string()),
        };
        out.push(RuleInput {
            provider,
            // 归一化模型名，使规则与 usage 的 model_normalized 匹配
            model: metria_core::normalize::normalize_model(&model),
            input: p("prompt"),
            output: p("completion"),
            cache_read: p("input_cache_read"),
            cache_write: p("input_cache_write"),
            reasoning: p("completion"),
            request: p("request"),
        });
    }
    Ok(out)
}

/// 解析 LiteLLM model_prices_and_context_window.json。
fn parse_litellm(body: &str) -> Result<Vec<RuleInput>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let obj = v.as_object().ok_or("缺少对象")?;
    let mut out = Vec::new();
    for (model, m) in obj {
        let f = |k: &str| {
            m.get(k)
                .and_then(|x| x.as_f64())
                .map(per_token_to_per_m_micro)
        };
        // LiteLLM 第三方数据，model 无 provider 前缀
        out.push(RuleInput {
            provider: ".*".to_string(),
            model: metria_core::normalize::normalize_model(model),
            input: f("input_cost_per_token"),
            output: f("output_cost_per_token"),
            cache_read: f("cache_read_input_token_cost"),
            cache_write: f("cache_creation_input_token_cost"),
            reasoning: f("output_cost_per_token"),
            request: None,
        });
    }
    Ok(out)
}

/// per-token USD → 微美元 / 百万 token（×1e12）。
fn per_token_to_per_m_micro(per_token: f64) -> i64 {
    (per_token * 1_000_000.0 * 1_000_000.0).round() as i64
}

/// 从 DB 中加载启用的外部目录定义。
pub fn catalogs_from_db(db: &HubDb) -> Vec<CatalogDef> {
    let c = db.conn();
    let mut out = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT id, name, kind, COALESCE(base_url,''), COALESCE(authentication_type,'') FROM pricing_catalogs WHERE enabled = 1 AND kind IN ('openrouter','litellm','custom')",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        }) {
            for row in rows.flatten() {
                if row.3.is_empty() {
                    continue;
                }
                out.push(CatalogDef {
                    id: row.0,
                    name: row.1,
                    kind: row.2,
                    url: row.3,
                    auth: if row.4.is_empty() { None } else { Some(row.4) },
                });
            }
        }
    }
    out
}

/// 校验内置目录定义（用于测试/文档）。
pub fn known_catalogs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("openrouter", "https://openrouter.ai/api/v1/models"),
        ("litellm", "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"),
    ]
}

/// 用当前全部启用规则重新计价，并重建费用 rollup（返回重计价条数）。
///
/// `only_unpriced = true` 时仅处理尚无费用的事件（后台周期增量），否则全量重算。
pub fn reprice_from_rules(db: &HubDb, only_unpriced: bool) -> Result<i64, String> {
    let engine = pricing_engine(db);
    let n = db
        .reprice_all(&engine, only_unpriced)
        .map_err(|e| e.to_string())?;
    // 历史规则可能命中任意时间的事件，必须全量重建费用口径。
    if n > 0 || !only_unpriced {
        db.rebuild_all_usage_rollups().map_err(|e| e.to_string())?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_pricing_fills_known_model_and_leaves_unknown_unavailable() {
        let engine = metria_pricing::PricingEngine::new();
        let mut known = serde_json::json!({
            "timestamp": "2026-09-11T00:00:00Z",
            "provider_normalized": "openai",
            "model_normalized": "gpt-5",
            "usage": { "input": 1_000_000, "output": 100_000 }
        });
        price_usage_payload(&engine, &mut known).unwrap();
        assert!(known["cost"]["calculated_micro_usd"].as_i64().unwrap() > 0);

        let mut unknown = serde_json::json!({
            "timestamp": "2026-09-11T00:00:00Z",
            "provider_normalized": "custom",
            "model_normalized": "unknown-model",
            "usage": { "input": 1_000_000, "output": 100_000 },
            "cost": {}
        });
        price_usage_payload(&engine, &mut unknown).unwrap();
        assert!(unknown["cost"].as_object().unwrap().is_empty());
    }
}
