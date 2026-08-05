//! `metria mcp`：只读 MCP（Model Context Protocol）stdio 服务。
//!
//! 通过 JSON-RPC 2.0 over stdio 提供只读查询工具，
//! 直接读取 Hub SQLite（不经过 HTTP，无写操作）。

use metria_hub::db::HubDb;
use serde_json::{json, Value};

/// MCP 服务器主循环（阻塞读取 stdin）。
pub fn run() -> Result<(), String> {
    let cfg = metria_hub::HubConfig::from_env().map_err(|e| e.to_string())?;
    let db = HubDb::open(&cfg).map_err(|e| e.to_string())?;
    let db = &db;

    let stdin = std::io::stdin();
    let mut line = String::new();
    loop {
        line.clear();
        let n = stdin
            .read_line(&mut line)
            .map_err(|e| format!("stdin 读取失败: {e}"))?;
        if n == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                // 非 JSON 忽略（协议外）
                let _ = e;
                continue;
            }
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        match method {
            "initialize" => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "metria-mcp", "version": "0.1.0" }
                    }
                });
                write_resp(&resp)?;
            }
            "notifications/initialized" | "initialized" => {}
            "ping" => write_resp(&json!({ "jsonrpc": "2.0", "id": id, "result": {} }))?,
            "tools/list" => {
                let tools = tools_list();
                write_resp(&json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tools } }))?;
            }
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let result = call_tool(db, name, &args);
                write_resp(&json!({ "jsonrpc": "2.0", "id": id, "result": {
                    "content": [ { "type": "text", "text": result } ]
                } }))?;
            }
            _ => {
                // 未知方法：返回错误但保持连接
                let resp = json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("未知方法 {method}") }
                });
                write_resp(&resp)?;
            }
        }
    }
    Ok(())
}

fn write_resp(v: &Value) -> Result<(), String> {
    let mut out = std::io::stdout();
    use std::io::Write;
    serde_json::to_writer(&mut out, v).map_err(|e| e.to_string())?;
    out.write_all(b"\n").map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())
}

fn tools_list() -> Vec<Value> {
    vec![
        tool(
            "overview",
            "获取指定时间范围用量/费用/流量汇总",
            &["from", "to"],
        ),
        tool("list_nodes", "列出节点", &[]),
        tool("list_models", "列出模型与用量汇总", &["from", "to"]),
        tool("list_sessions", "列出会话", &["from", "to", "limit"]),
        tool("get_session", "获取会话详情", &["session_id"]),
        tool("list_calls", "列出模型调用", &["from", "to", "limit"]),
        tool("traffic_summary", "估算流量汇总", &["from", "to"]),
    ]
}

fn tool(name: &str, description: &str, props: &[&str]) -> Value {
    let mut schema = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    for p in props {
        schema["properties"][p] = json!({ "type": "string" });
    }
    json!({ "name": name, "description": description, "inputSchema": schema })
}

fn from_to(args: &Value) -> (String, String) {
    let from = args
        .get("from")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let to = args
        .get("to")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    (from, to)
}

fn call_tool(db: &HubDb, name: &str, args: &Value) -> String {
    let (from, to) = from_to(args);
    let c = db.conn();
    let result = match name {
        "overview" => {
            let r = c
                .query_row(
                    "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0) FROM hourly_rollups WHERE (?1 = '' OR bucket >= ?1) AND (?2 = '' OR bucket < ?2)",
                    metria_storage::rusqlite::params![from, to],
                    |r| {
                        Ok(json!({
                            "input_tokens": r.get::<_, i64>(0)?,
                            "output_tokens": r.get::<_, i64>(1)?,
                            "estimated_traffic_bytes": r.get::<_, i64>(2)?,
                            "model_calls": r.get::<_, i64>(3)?,
                            "sessions": r.get::<_, i64>(4)?,
                        }))
                    },
                )
                .unwrap_or(json!({}));
            serde_json::to_string_pretty(&r).unwrap_or_default()
        }
        "list_nodes" => {
            let rows = query_all(
                &c,
                "SELECT id, name, status FROM nodes ORDER BY last_seen_at DESC",
                &[],
                |r| json!({ "id": r.get::<_, String>(0).unwrap_or_default(), "name": r.get::<_, String>(1).unwrap_or_default(), "status": r.get::<_, String>(2).unwrap_or_default() }),
            );
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        }
        "list_models" => {
            let rows = query_all(
                &c,
                "SELECT model, provider, COALESCE(SUM(input_tokens),0), COALESCE(SUM(model_call_count),0) FROM hourly_rollups WHERE (?1 = '' OR bucket >= ?1) AND (?2 = '' OR bucket < ?2) AND model != '' GROUP BY model, provider ORDER BY 4 DESC LIMIT 100",
                &[from, to],
                |r| json!({ "model": r.get::<_, String>(0).unwrap_or_default(), "provider": r.get::<_, String>(1).unwrap_or_default(), "input_tokens": r.get::<_, i64>(2).unwrap_or(0), "calls": r.get::<_, i64>(3).unwrap_or(0) }),
            );
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        }
        "list_sessions" => {
            let limit = args
                .get("limit")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(50);
            let rows = query_all(
                &c,
                "SELECT id, client_id, title, started_at, model_call_count, estimated_total_bytes FROM sessions WHERE (?1 = '' OR started_at >= ?1) AND (?2 = '' OR started_at < ?2) ORDER BY started_at DESC LIMIT ?3",
                &[from, to, limit.to_string()],
                |r| json!({ "id": r.get::<_, String>(0).unwrap_or_default(), "client": r.get::<_, String>(1).unwrap_or_default(), "title": r.get::<_, Option<String>>(2).ok().flatten(), "started_at": r.get::<_, String>(3).unwrap_or_default(), "calls": r.get::<_, i64>(4).unwrap_or(0), "estimated_traffic_bytes": r.get::<_, Option<i64>>(5).ok().flatten() }),
            );
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        }
        "get_session" => {
            let sid = args
                .get("session_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let rows = query_all(
                &c,
                "SELECT id, client_id, title, started_at, message_count, tool_call_count, model_call_count, input_tokens, output_tokens, estimated_total_bytes FROM sessions WHERE id = ?1",
                &[sid.to_string()],
                |r| json!({ "id": r.get::<_, String>(0).unwrap_or_default(), "client": r.get::<_, String>(1).unwrap_or_default(), "title": r.get::<_, Option<String>>(2).ok().flatten(), "started_at": r.get::<_, String>(3).unwrap_or_default(), "messages": r.get::<_, i64>(4).unwrap_or(0), "tools": r.get::<_, i64>(5).unwrap_or(0), "calls": r.get::<_, i64>(6).unwrap_or(0), "input_tokens": r.get::<_, Option<i64>>(7).ok().flatten(), "output_tokens": r.get::<_, Option<i64>>(8).ok().flatten(), "estimated_traffic_bytes": r.get::<_, Option<i64>>(9).ok().flatten() }),
            );
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        }
        "list_calls" => {
            let limit = args
                .get("limit")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(100);
            let rows = query_all(
                &c,
                "SELECT id, client_id, model_normalized, started_at, status, input_tokens, output_tokens, calculated_cost_micro_usd FROM model_calls WHERE (?1 = '' OR started_at >= ?1) AND (?2 = '' OR started_at < ?2) ORDER BY started_at DESC LIMIT ?3",
                &[from, to, limit.to_string()],
                |r| json!({ "id": r.get::<_, String>(0).unwrap_or_default(), "client": r.get::<_, String>(1).unwrap_or_default(), "model": r.get::<_, Option<String>>(2).ok().flatten(), "started_at": r.get::<_, String>(3).unwrap_or_default(), "status": r.get::<_, String>(4).unwrap_or_default(), "input_tokens": r.get::<_, Option<i64>>(5).ok().flatten(), "output_tokens": r.get::<_, Option<i64>>(6).ok().flatten(), "cost_micro_usd": r.get::<_, Option<i64>>(7).ok().flatten() }),
            );
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        }
        "traffic_summary" => {
            let r = c
                .query_row(
                    "SELECT COALESCE(SUM(estimated_request_bytes),0), COALESCE(SUM(estimated_response_bytes),0), COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0) FROM hourly_rollups WHERE (?1 = '' OR bucket >= ?1) AND (?2 = '' OR bucket < ?2)",
                    metria_storage::rusqlite::params![from, to],
                    |r| {
                        Ok(json!({
                            "estimated_request_bytes": r.get::<_, i64>(0)?,
                            "estimated_response_bytes": r.get::<_, i64>(1)?,
                            "estimated_total_bytes": r.get::<_, i64>(2)?,
                            "model_calls": r.get::<_, i64>(3)?,
                        }))
                    },
                )
                .unwrap_or(json!({}));
            serde_json::to_string_pretty(&r).unwrap_or_default()
        }
        _ => format!("未知工具 {name}"),
    };
    result
}

fn query_all(
    c: &metria_storage::rusqlite::Connection,
    sql: &str,
    params: &[String],
    mapper: impl Fn(&metria_storage::rusqlite::Row<'_>) -> Value,
) -> Vec<Value> {
    let mut out = Vec::new();
    if let Ok(mut stmt) = c.prepare(sql) {
        let args: Vec<metria_storage::rusqlite::types::Value> = params
            .iter()
            .map(|s| metria_storage::rusqlite::types::Value::Text(s.clone()))
            .collect();
        if let Ok(rows) = stmt.query_map(
            metria_storage::rusqlite::params_from_iter(args.iter()),
            |r| Ok(mapper(r)),
        ) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    out
}
