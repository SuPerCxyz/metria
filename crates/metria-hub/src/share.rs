//! Share Link：生成公开只读链接，返回脱敏 DTO（不含正文与敏感信息）。

use chrono::Utc;
use metria_storage::rusqlite::params;
use metria_storage::StorageError;
use serde_json::json;

use crate::db::HubDb;

/// 创建分享链接。
pub fn create_share(db: &HubDb, kind: &str, target_id: &str) -> Result<String, StorageError> {
    let slug = format!(
        "{}-{}",
        kind,
        &metria_core::model::ContentHash::hash_str(target_id).as_str()[..10]
    );
    let c = db.conn();
    c.execute(
        "INSERT OR REPLACE INTO share_links (id, slug, kind, target_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            metria_core::model::Id::new().as_str().to_string(),
            slug,
            kind,
            target_id,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(slug)
}

/// 查询分享目标是否存在且可见。
pub fn resolve_share(db: &HubDb, slug: &str) -> Option<(String, String)> {
    let c = db.conn();
    c.query_row(
        "SELECT kind, target_id FROM share_links WHERE slug = ?1",
        [slug],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

/// 记录查看审计。
pub fn record_view(db: &HubDb, slug: &str) {
    let c = db.conn();
    let _ = c.execute(
        "INSERT INTO share_audits (id, slug, ip, viewed_at) VALUES (?1, ?2, NULL, ?3)",
        params![
            metria_core::model::Id::new().as_str().to_string(),
            slug,
            Utc::now().to_rfc3339()
        ],
    );
}

/// 构造公开 DTO（脱敏：仅聚合，不含消息正文、路径、成本明细细节）。
pub fn build_share_dto(db: &HubDb, kind: &str, target_id: &str) -> serde_json::Value {
    match kind {
        "session" => session_dto(db, target_id),
        "node" => node_dto(db, target_id),
        _ => json!({ "error": "不支持的分享类型" }),
    }
}

fn session_dto(db: &HubDb, session_key: &str) -> serde_json::Value {
    let c = db.conn();
    let summary = c
        .query_row(
            "SELECT source_session_id, client_id, title, started_at, ended_at, message_count,
                    tool_call_count, subagent_count, model_call_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    estimated_request_bytes, estimated_response_bytes, estimated_total_bytes,
                    traffic_confidence
             FROM sessions WHERE id = ?1",
            [session_key],
            |r| {
                Ok(json!({
                    "source_session_id": r.get::<_, String>(0)?,
                    "client_id": r.get::<_, String>(1)?,
                    "title": r.get::<_, Option<String>>(2)?,
                    "started_at": r.get::<_, String>(3)?,
                    "ended_at": r.get::<_, Option<String>>(4)?,
                    "message_count": r.get::<_, i64>(5)?,
                    "tool_call_count": r.get::<_, i64>(6)?,
                    "subagent_count": r.get::<_, i64>(7)?,
                    "model_call_count": r.get::<_, i64>(8)?,
                    "input_tokens": r.get::<_, Option<i64>>(9)?,
                    "output_tokens": r.get::<_, Option<i64>>(10)?,
                    "cache_read_tokens": r.get::<_, Option<i64>>(11)?,
                    "cache_write_tokens": r.get::<_, Option<i64>>(12)?,
                    "reasoning_tokens": r.get::<_, Option<i64>>(13)?,
                    "estimated_request_bytes": r.get::<_, Option<i64>>(14)?,
                    "estimated_response_bytes": r.get::<_, Option<i64>>(15)?,
                    "estimated_total_bytes": r.get::<_, Option<i64>>(16)?,
                    "traffic_confidence": r.get::<_, Option<f64>>(17)?,
                }))
            },
        )
        .unwrap_or(json!({}));
    // 每次调用估算流量（脱敏：无模型内部细节）
    let mut calls = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT model_normalized, started_at, input_tokens, output_tokens, calculated_cost_micro_usd
         FROM model_calls WHERE session_id = ?1 ORDER BY started_at LIMIT 200",
    ) {
        if let Ok(rows) = stmt.query_map([session_key], |r| {
            Ok(json!({
                "model": r.get::<_, Option<String>>(0)?,
                "started_at": r.get::<_, String>(1)?,
                "input_tokens": r.get::<_, Option<i64>>(2)?,
                "output_tokens": r.get::<_, Option<i64>>(3)?,
                "cost_micro_usd": r.get::<_, Option<i64>>(4)?,
            }))
        }) {
            for row in rows.flatten() {
                calls.push(row);
            }
        }
    }
    json!({ "kind": "session", "summary": summary, "calls": calls })
}

fn node_dto(db: &HubDb, node_id: &str) -> serde_json::Value {
    let c = db.conn();
    let mut clients = Vec::new();
    if let Ok(mut stmt) =
        c.prepare("SELECT client_id, COUNT(*) FROM sources WHERE node_id = ?1 GROUP BY client_id")
    {
        if let Ok(rows) = stmt.query_map([node_id], |r| {
            Ok(json!({ "client_id": r.get::<_, String>(0)?, "source_count": r.get::<_, i64>(1)? }))
        }) {
            for row in rows.flatten() {
                clients.push(row);
            }
        }
    }
    json!({ "kind": "node", "node_id": node_id, "clients": clients })
}

/// 删除（吊销）分享链接。
pub fn delete_share(db: &HubDb, slug: &str) -> Result<bool, StorageError> {
    let c = db.conn();
    let n = c
        .execute("DELETE FROM share_links WHERE slug = ?1", [slug])
        .map_err(StorageError::from)?;
    Ok(n > 0)
}

/// 分享查看审计（最近 200 条）。
pub fn list_share_audits(db: &HubDb) -> Vec<serde_json::Value> {
    let c = db.conn();
    let mut out = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT id, slug, ip, viewed_at FROM share_audits ORDER BY viewed_at DESC LIMIT 200",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "slug": r.get::<_, String>(1)?,
                "ip": r.get::<_, Option<String>>(2)?,
                "viewed_at": r.get::<_, String>(3)?,
            }))
        }) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    out
}
