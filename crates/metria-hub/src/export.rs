//! 数据导出：JSON / NDJSON / CSV（sessions / calls / usage）。

use chrono::{DateTime, Utc};
use metria_storage::rusqlite::params;
use serde_json::{json, Value};

use crate::db::HubDb;

/// 导出格式。
#[derive(Debug)]
pub enum Format {
    Json,
    Ndjson,
    Csv,
}

pub fn parse_format(s: &str) -> Option<Format> {
    match s.to_ascii_lowercase().as_str() {
        "json" => Some(Format::Json),
        "ndjson" => Some(Format::Ndjson),
        "csv" => Some(Format::Csv),
        _ => None,
    }
}

/// 导出 sessions 数据。
pub fn export_sessions(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    fmt: &Format,
) -> Result<(String, String), String> {
    let c = db.conn();
    let mut stmt = c
        .prepare(
            "SELECT id, source_session_id, node_id, client_id, title, primary_model_normalized,
                    started_at, ended_at, message_count, tool_call_count, model_call_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                    estimated_total_bytes
             FROM sessions WHERE started_at >= ?1 AND started_at < ?2 ORDER BY started_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "source_session_id": r.get::<_, String>(1)?,
                "node_id": r.get::<_, String>(2)?,
                "client_id": r.get::<_, String>(3)?,
                "title": r.get::<_, Option<String>>(4)?,
                "model": r.get::<_, Option<String>>(5)?,
                "started_at": r.get::<_, String>(6)?,
                "ended_at": r.get::<_, Option<String>>(7)?,
                "message_count": r.get::<_, i64>(8)?,
                "tool_call_count": r.get::<_, i64>(9)?,
                "model_call_count": r.get::<_, i64>(10)?,
                "input_tokens": r.get::<_, Option<i64>>(11)?,
                "output_tokens": r.get::<_, Option<i64>>(12)?,
                "cache_read_tokens": r.get::<_, Option<i64>>(13)?,
                "cache_write_tokens": r.get::<_, Option<i64>>(14)?,
                "reasoning_tokens": r.get::<_, Option<i64>>(15)?,
                "reported_cost_micro_usd": r.get::<_, Option<i64>>(16)?,
                "calculated_cost_micro_usd": r.get::<_, Option<i64>>(17)?,
                "estimated_cost_micro_usd": r.get::<_, Option<i64>>(18)?,
                "estimated_total_bytes": r.get::<_, Option<i64>>(19)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    let items: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
    render(&items, fmt, "sessions")
}

/// 导出 calls 数据。
pub fn export_calls(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    fmt: &Format,
) -> Result<(String, String), String> {
    let c = db.conn();
    let mut stmt = c
        .prepare(
            "SELECT id, client_id, session_id, provider_normalized, model_normalized, started_at, status,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd
             FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 ORDER BY started_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "client_id": r.get::<_, String>(1)?,
                "session_id": r.get::<_, String>(2)?,
                "provider": r.get::<_, Option<String>>(3)?,
                "model": r.get::<_, Option<String>>(4)?,
                "started_at": r.get::<_, String>(5)?,
                "status": r.get::<_, String>(6)?,
                "input_tokens": r.get::<_, Option<i64>>(7)?,
                "output_tokens": r.get::<_, Option<i64>>(8)?,
                "cache_read_tokens": r.get::<_, Option<i64>>(9)?,
                "cache_write_tokens": r.get::<_, Option<i64>>(10)?,
                "reasoning_tokens": r.get::<_, Option<i64>>(11)?,
                "reported_cost_micro_usd": r.get::<_, Option<i64>>(12)?,
                "calculated_cost_micro_usd": r.get::<_, Option<i64>>(13)?,
                "estimated_cost_micro_usd": r.get::<_, Option<i64>>(14)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    let items: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
    render(&items, fmt, "calls")
}

fn render(items: &[Value], fmt: &Format, name: &str) -> Result<(String, String), String> {
    match fmt {
        Format::Json => {
            let body = serde_json::to_string_pretty(items).map_err(|e| e.to_string())?;
            Ok((body, format!("{name}.json")))
        }
        Format::Ndjson => {
            let mut out = String::new();
            for it in items {
                out.push_str(&serde_json::to_string(it).map_err(|e| e.to_string())?);
                out.push('\n');
            }
            Ok((out, format!("{name}.ndjson")))
        }
        Format::Csv => {
            let mut out = String::new();
            if let Some(first) = items.first() {
                let keys: Vec<&str> = first
                    .as_object()
                    .map(|m| m.keys().map(|k| k.as_str()).collect())
                    .unwrap_or_default();
                out.push_str(&keys.join(","));
                out.push('\n');
                for it in items {
                    let vals: Vec<String> = keys
                        .iter()
                        .map(|k| {
                            it.get(k)
                                .map(|v| match v {
                                    Value::String(s) => format!("\"{}\"", s.replace('"', "\"\"")),
                                    Value::Null => String::new(),
                                    other => other.to_string(),
                                })
                                .unwrap_or_default()
                        })
                        .collect();
                    out.push_str(&vals.join(","));
                    out.push('\n');
                }
            }
            Ok((out, format!("{name}.csv")))
        }
    }
}
