//! CLI 数据导出：sessions / calls / usage → JSON / NDJSON / CSV。
//!
//! 直接读取 Hub SQLite（与 `backup` 相同方式），复用 `metria_hub::export`
//! 的 sessions/calls 导出逻辑；usage 事件单独查询。

use chrono::{DateTime, Utc};
use metria_hub::db::HubDb;
use metria_hub::export::{export_calls, export_sessions, Format};
use metria_storage::rusqlite::params;

/// 导出数据范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Sessions,
    Calls,
    Usage,
}

pub fn parse_kind(s: &str) -> Option<ExportKind> {
    match s.to_ascii_lowercase().as_str() {
        "sessions" => Some(ExportKind::Sessions),
        "calls" => Some(ExportKind::Calls),
        "usage" => Some(ExportKind::Usage),
        _ => None,
    }
}

/// 执行导出。
pub fn export(
    database_url: &str,
    kind: ExportKind,
    fmt: &Format,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    out: Option<&str>,
) -> Result<(), String> {
    let cfg = hub_config(database_url)?;
    let db = HubDb::open(&cfg).map_err(|e| format!("打开数据库失败: {e}"))?;
    let (body, filename) = match kind {
        ExportKind::Sessions => export_sessions(&db, from, to, fmt)?,
        ExportKind::Calls => export_calls(&db, from, to, fmt)?,
        ExportKind::Usage => export_usage(&db, from, to, fmt)?,
    };
    let out_path = out.map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::PathBuf::from(format!(
            "metria-export-{}-{}",
            match kind {
                ExportKind::Sessions => "sessions",
                ExportKind::Calls => "calls",
                ExportKind::Usage => "usage",
            },
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        ))
    });
    std::fs::write(&out_path, body).map_err(|e| format!("写入失败: {e}"))?;
    println!("导出完成: {}（{}）", out_path.display(), filename);
    Ok(())
}

/// 构造仅用于打开数据库的 HubConfig（监听地址等无关字段用默认值）。
fn hub_config(database_url: &str) -> Result<metria_hub::HubConfig, String> {
    let mut cfg = metria_hub::HubConfig::from_env().map_err(|e| e.to_string())?;
    cfg.database_url = database_url.to_string();
    Ok(cfg)
}

/// 导出 usage 事件。
fn export_usage(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    fmt: &Format,
) -> Result<(String, String), String> {
    let c = db.conn();
    let mut stmt = c
        .prepare(
            "SELECT event_id, client_id, session_id, node_id, model_normalized, timestamp,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd
             FROM usage_events WHERE timestamp >= ?1 AND timestamp < ?2 ORDER BY timestamp",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "client_id": r.get::<_, Option<String>>(1)?,
                "session_id": r.get::<_, Option<String>>(2)?,
                "node_id": r.get::<_, Option<String>>(3)?,
                "model": r.get::<_, Option<String>>(4)?,
                "occurred_at": r.get::<_, String>(5)?,
                "input_tokens": r.get::<_, Option<i64>>(6)?,
                "output_tokens": r.get::<_, Option<i64>>(7)?,
                "cache_read_tokens": r.get::<_, Option<i64>>(8)?,
                "cache_write_tokens": r.get::<_, Option<i64>>(9)?,
                "reasoning_tokens": r.get::<_, Option<i64>>(10)?,
                "reported_cost_micro_usd": r.get::<_, Option<i64>>(11)?,
                "calculated_cost_micro_usd": r.get::<_, Option<i64>>(12)?,
                "estimated_cost_micro_usd": r.get::<_, Option<i64>>(13)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    let items: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    render(&items, fmt, "usage")
}

fn render(
    items: &[serde_json::Value],
    fmt: &Format,
    name: &str,
) -> Result<(String, String), String> {
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
                            it.get(*k)
                                .and_then(|v| v.as_str())
                                .map(escape_csv)
                                .unwrap_or_else(|| {
                                    it.get(*k).map(|v| v.to_string()).unwrap_or_default()
                                })
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

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
