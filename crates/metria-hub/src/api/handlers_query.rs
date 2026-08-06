//! 查询 API handlers：overview / nodes / clients / models / calls / sessions / traffic / data-quality。
//!
//! 作为 `api` 模块的子模块，通过 `crate::api::*` 复用类型与工具函数。

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use metria_storage::rusqlite::{params, params_from_iter, types::Value as SqlValue};

use crate::api::{json_err, parse_range, range_args, range_filter, AppState, RangeParams};
use crate::q;

pub(crate) async fn overview(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let c = st.db.conn();
    let row = c.query_row(
        &format!(
            "SELECT
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(reasoning_tokens),0),
                COALESCE(SUM(reported_cost),0), COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_cost),0),
                COALESCE(SUM(estimated_request_bytes),0), COALESCE(SUM(estimated_response_bytes),0), COALESCE(SUM(estimated_total_bytes),0),
                COALESCE(SUM(estimated_lower_bound_bytes),0), COALESCE(SUM(estimated_upper_bound_bytes),0),
                COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0)
             FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 {filter}"
        ),
        params_from_iter(range_args(&from, &to, fargs)),
        |r| {
            Ok(serde_json::json!({
                "input_tokens": r.get::<_, i64>(0)?,
                "output_tokens": r.get::<_, i64>(1)?,
                "cache_read_tokens": r.get::<_, i64>(2)?,
                "cache_write_tokens": r.get::<_, i64>(3)?,
                "reasoning_tokens": r.get::<_, i64>(4)?,
                "reported_cost_micro_usd": r.get::<_, i64>(5)?,
                "calculated_cost_micro_usd": r.get::<_, i64>(6)?,
                "estimated_cost_micro_usd": r.get::<_, i64>(7)?,
                "estimated_request_bytes": r.get::<_, i64>(8)?,
                "estimated_response_bytes": r.get::<_, i64>(9)?,
                "estimated_total_bytes": r.get::<_, i64>(10)?,
                "traffic_lower_bound_bytes": r.get::<_, i64>(11)?,
                "traffic_upper_bound_bytes": r.get::<_, i64>(12)?,
                "model_calls": r.get::<_, i64>(13)?,
                "sessions": r.get::<_, i64>(14)?,
            }))
        },
    );
    let mut body = row.unwrap_or_else(|e| {
        tracing::error!(%e, "overview 查询失败");
        serde_json::json!({})
    });
    body["nodes"] = c
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0)
        .into();
    body["collectors"] = c
        .query_row("SELECT COUNT(*) FROM collectors", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0)
        .into();
    Json(body).into_response()
}

pub(crate) async fn usage_timeseries(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let table = match p.granularity.as_deref() {
        Some("day") => "daily_rollups",
        _ => "hourly_rollups",
    };
    let bucket_col = if table == "daily_rollups" {
        "substr(bucket,1,10)"
    } else {
        "bucket"
    };
    let (filter, fargs) = range_filter(&p);
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(&format!(
        "SELECT {bucket_col} AS b,
            COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
            COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
            COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0)
         FROM {table} WHERE bucket >= ?1 AND bucket < ?2 {filter}
         GROUP BY b ORDER BY b"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok(serde_json::json!({
                "bucket": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "cost_micro_usd": r.get::<_, i64>(3)?,
                "estimated_traffic_bytes": r.get::<_, i64>(4)?,
                "model_calls": r.get::<_, i64>(5)?,
            }))
        },)
    );
    let points: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "series": points })).into_response()
}

pub(crate) async fn usage_breakdown(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    // 支持多维度汇总（S3.3）：node/client/model/provider/project
    let (col, key) = match p.dim.as_deref() {
        Some("client") => ("client_id", "client_id"),
        Some("model") => ("model", "model"),
        Some("provider") => ("provider", "provider"),
        Some("project") => ("project_id", "project_id"),
        _ => ("node_id", "node_id"),
    };
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(&format!(
        "SELECT {col}, COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
            COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0),
            COALESCE(SUM(session_count),0)
         FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 {filter}
         GROUP BY {col} ORDER BY 5 DESC"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok(serde_json::json!({
                "dimension": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "estimated_traffic_bytes": r.get::<_, i64>(3)?,
                "model_calls": r.get::<_, i64>(4)?,
                "sessions": r.get::<_, i64>(5)?,
            }))
        },)
    );
    let items: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "by": items, "dimension": key })).into_response()
}

pub(crate) async fn list_nodes(State(st): State<AppState>) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT id, name, platform, architecture, timezone, status, first_seen_at, last_seen_at FROM nodes ORDER BY last_seen_at DESC",
    ));
    let rows = q!(stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "name": r.get::<_, String>(1)?,
            "platform": r.get::<_, Option<String>>(2)?,
            "architecture": r.get::<_, Option<String>>(3)?,
            "timezone": r.get::<_, Option<String>>(4)?,
            "status": r.get::<_, String>(5)?,
            "first_seen_at": r.get::<_, String>(6)?,
            "last_seen_at": r.get::<_, String>(7)?,
        }))
    }));
    let nodes: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "nodes": nodes })).into_response()
}

pub(crate) async fn node_detail(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let node = c
        .query_row(
            "SELECT id, name, platform, architecture, timezone, status, first_seen_at, last_seen_at FROM nodes WHERE id = ?1",
            [&id],
            |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "platform": r.get::<_, Option<String>>(2)?,
                    "architecture": r.get::<_, Option<String>>(3)?,
                    "timezone": r.get::<_, Option<String>>(4)?,
                    "status": r.get::<_, String>(5)?,
                    "first_seen_at": r.get::<_, String>(6)?,
                    "last_seen_at": r.get::<_, String>(7)?,
                }))
            },
        )
        .ok()
        .unwrap_or(serde_json::json!({}));
    let mut clients = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT DISTINCT client_id, COUNT(*) as src_count FROM sources WHERE node_id = ?1 GROUP BY client_id",
    ) {
        if let Ok(rows) = stmt.query_map([&id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
            for row in rows.flatten() {
                clients.push(serde_json::json!({ "client_id": row.0, "source_count": row.1 }));
            }
        }
    }
    // 分布统计（S3.4）：按 Model / Project 汇总本 Node 的调用与 token
    let mut by_model = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT COALESCE(model_normalized,'(unknown)'), COUNT(*), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0)
         FROM model_calls WHERE node_id = ?1 AND model_normalized IS NOT NULL GROUP BY model_normalized ORDER BY 2 DESC LIMIT 10",
    ) {
        if let Ok(rows) = stmt.query_map([&id], |r| {
            Ok(serde_json::json!({
                "model": r.get::<_, String>(0)?,
                "calls": r.get::<_, i64>(1)?,
                "input_tokens": r.get::<_, i64>(2)?,
                "output_tokens": r.get::<_, i64>(3)?,
            }))
        }) {
            by_model = rows.filter_map(|x| x.ok()).collect();
        }
    }
    let mut by_project = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT COALESCE(project_id,'(none)'), COUNT(*), COALESCE(SUM(estimated_total_bytes),0)
         FROM sessions WHERE node_id = ?1 GROUP BY project_id ORDER BY 2 DESC LIMIT 10",
    ) {
        if let Ok(rows) = stmt.query_map([&id], |r| {
            Ok(serde_json::json!({
                "project_id": r.get::<_, String>(0)?,
                "sessions": r.get::<_, i64>(1)?,
                "estimated_total_bytes": r.get::<_, i64>(2)?,
            }))
        }) {
            by_project = rows.filter_map(|x| x.ok()).collect();
        }
    }
    Json(serde_json::json!({
        "node": node,
        "clients": clients,
        "by_model": by_model,
        "by_project": by_project,
    }))
    .into_response()
}

pub(crate) async fn node_clients(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT client_id, adapter_id, adapter_version, source_path_hash, status, client_version, last_scan_at, last_error, last_event_at
         FROM sources WHERE node_id = ?1 ORDER BY client_id",
    ));
    let rows = q!(stmt.query_map([&id], |r| {
        Ok(serde_json::json!({
            "client_id": r.get::<_, String>(0)?,
            "adapter_id": r.get::<_, String>(1)?,
            "adapter_version": r.get::<_, String>(2)?,
            "source_path_hash": r.get::<_, String>(3)?,
            "status": r.get::<_, String>(4)?,
            "client_version": r.get::<_, Option<String>>(5)?,
            "last_scan_at": r.get::<_, Option<String>>(6)?,
            "last_error": r.get::<_, Option<String>>(7)?,
            "last_event_at": r.get::<_, Option<String>>(8)?,
        }))
    }));
    let sources: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "sources": sources })).into_response()
}

pub(crate) async fn node_sessions(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(50).min(500);
    let mut stmt = q!(c.prepare(
        "SELECT id, source_session_id, client_id, title, primary_model_normalized, started_at, ended_at, message_count, model_call_count, input_tokens, output_tokens, estimated_total_bytes
         FROM sessions WHERE node_id = ?1 AND started_at >= ?2 AND started_at < ?3 ORDER BY started_at DESC LIMIT ?4",
    ));
    let rows = q!(stmt.query_map(
        params![id, from.to_rfc3339(), to.to_rfc3339(), limit],
        |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "source_session_id": r.get::<_, String>(1)?,
                "client_id": r.get::<_, String>(2)?,
                "title": r.get::<_, Option<String>>(3)?,
                "model": r.get::<_, Option<String>>(4)?,
                "started_at": r.get::<_, String>(5)?,
                "ended_at": r.get::<_, Option<String>>(6)?,
                "message_count": r.get::<_, i64>(7)?,
                "model_call_count": r.get::<_, i64>(8)?,
                "input_tokens": r.get::<_, Option<i64>>(9)?,
                "output_tokens": r.get::<_, Option<i64>>(10)?,
                "estimated_total_bytes": r.get::<_, Option<i64>>(11)?,
            }))
        },
    ));
    let sessions: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    let next_cursor = sessions.last().and_then(|v| {
        let id = v.get("id")?.as_str()?;
        let ts = v.get("started_at")?.as_str()?;
        Some(crate::api::encode_cursor(ts, id))
    });
    Json(serde_json::json!({ "sessions": sessions, "next_cursor": next_cursor })).into_response()
}

pub(crate) async fn node_calls(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(50).min(500);
    let mut stmt = q!(c.prepare(
        "SELECT id, model_normalized, provider_normalized, started_at, status, input_tokens, output_tokens, cache_read_tokens, reasoning_tokens, reported_cost_micro_usd, calculated_cost_micro_usd
         FROM model_calls WHERE node_id = ?1 AND started_at >= ?2 AND started_at < ?3 ORDER BY started_at DESC LIMIT ?4",
    ));
    let rows = q!(stmt.query_map(
        params![id, from.to_rfc3339(), to.to_rfc3339(), limit],
        |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "model": r.get::<_, Option<String>>(1)?,
                "provider": r.get::<_, Option<String>>(2)?,
                "started_at": r.get::<_, String>(3)?,
                "status": r.get::<_, String>(4)?,
                "input_tokens": r.get::<_, Option<i64>>(5)?,
                "output_tokens": r.get::<_, Option<i64>>(6)?,
                "cache_read_tokens": r.get::<_, Option<i64>>(7)?,
                "reasoning_tokens": r.get::<_, Option<i64>>(8)?,
                "reported_cost_micro_usd": r.get::<_, Option<i64>>(9)?,
                "calculated_cost_micro_usd": r.get::<_, Option<i64>>(10)?,
            }))
        },
    ));
    let calls: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "calls": calls })).into_response()
}

pub(crate) async fn list_clients(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(&format!(
        "SELECT client_id,
            COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(estimated_total_bytes),0),
            COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0), COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0)
         FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 {filter} GROUP BY client_id ORDER BY 2 DESC"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok(serde_json::json!({
                "client_id": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "estimated_traffic_bytes": r.get::<_, i64>(3)?,
                "model_calls": r.get::<_, i64>(4)?,
                "sessions": r.get::<_, i64>(5)?,
                "cost_micro_usd": r.get::<_, i64>(6)?,
            }))
        },)
    );
    let clients: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "clients": clients })).into_response()
}

pub(crate) async fn client_detail(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT node_id, COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0)
         FROM hourly_rollups WHERE client_id = ?1 AND bucket >= ?2 AND bucket < ?3 GROUP BY node_id ORDER BY 2 DESC",
    ));
    let rows = q!(
        stmt.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok(serde_json::json!({
                "node_id": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "estimated_traffic_bytes": r.get::<_, i64>(3)?,
                "model_calls": r.get::<_, i64>(4)?,
                "sessions": r.get::<_, i64>(5)?,
            }))
        },)
    );
    let by_node: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();

    // 最近 sessions（Agent Tools Detail）
    let recent: Vec<serde_json::Value> = {
        let mut st = q!(c.prepare(
            "SELECT id, source_session_id, node_id, title, primary_model_normalized, started_at,
                    model_call_count, input_tokens, output_tokens, estimated_total_bytes
             FROM sessions WHERE client_id = ?1 AND started_at >= ?2 AND started_at < ?3
             ORDER BY started_at DESC LIMIT 20",
        ));
        let r = q!(
            st.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source_session_id": r.get::<_, String>(1)?,
                    "node_id": r.get::<_, String>(2)?,
                    "title": r.get::<_, Option<String>>(3)?,
                    "model": r.get::<_, Option<String>>(4)?,
                    "started_at": r.get::<_, String>(5)?,
                    "model_call_count": r.get::<_, i64>(6)?,
                    "input_tokens": r.get::<_, Option<i64>>(7)?,
                    "output_tokens": r.get::<_, Option<i64>>(8)?,
                    "estimated_total_bytes": r.get::<_, Option<i64>>(9)?,
                }))
            },)
        );
        r.filter_map(|x| x.ok()).collect()
    };

    // 汇总（三口径 cost / 流量）
    let summary = c
        .query_row(
            "SELECT COALESCE(SUM(calculated_cost_micro_usd),0), COALESCE(SUM(estimated_total_bytes),0)
             FROM hourly_rollups WHERE client_id = ?1 AND bucket >= ?2 AND bucket < ?3",
            params![id, from.to_rfc3339(), to.to_rfc3339()],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
        .unwrap_or((0, 0));

    Json(serde_json::json!({
        "client_id": id,
        "by_node": by_node,
        "recent_sessions": recent,
        "calculated_cost_micro_usd": summary.0,
        "estimated_total_bytes": summary.1,
    }))
    .into_response()
}

pub(crate) async fn client_models(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT model_normalized, provider_normalized, COUNT(*) as cnt, COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0)
         FROM model_calls WHERE client_id = ?1 AND model_normalized IS NOT NULL GROUP BY model_normalized, provider_normalized ORDER BY cnt DESC",
    ));
    let rows = q!(stmt.query_map([&id], |r| {
        Ok(serde_json::json!({
            "model": r.get::<_, String>(0)?,
            "provider": r.get::<_, Option<String>>(1)?,
            "calls": r.get::<_, i64>(2)?,
            "input_tokens": r.get::<_, i64>(3)?,
            "output_tokens": r.get::<_, i64>(4)?,
        }))
    }));
    let models: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "models": models })).into_response()
}

pub(crate) async fn list_models(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(&format!(
        "SELECT model, MAX(provider),
            COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(estimated_total_bytes),0),
            COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0), COUNT(DISTINCT client_id), COUNT(DISTINCT node_id)
         FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 AND model != '' {filter} GROUP BY model ORDER BY 6 DESC"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok(serde_json::json!({
                "model": r.get::<_, String>(0)?,
                "provider": r.get::<_, String>(1)?,
                "input_tokens": r.get::<_, i64>(2)?,
                "output_tokens": r.get::<_, i64>(3)?,
                "estimated_traffic_bytes": r.get::<_, i64>(4)?,
                "model_calls": r.get::<_, i64>(5)?,
                "sessions": r.get::<_, i64>(6)?,
                "clients": r.get::<_, i64>(7)?,
                "nodes": r.get::<_, i64>(8)?,
            }))
        },)
    );
    let models: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "models": models })).into_response()
}

pub(crate) async fn model_detail(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT model_raw, provider_raw, COUNT(*) as cnt FROM model_calls WHERE model_normalized = ?1 GROUP BY model_raw, provider_raw ORDER BY cnt DESC",
    ));
    let rows = q!(stmt.query_map([&id], |r| {
        Ok(serde_json::json!({
            "model_raw": r.get::<_, Option<String>>(0)?,
            "provider": r.get::<_, Option<String>>(1)?,
            "calls": r.get::<_, i64>(2)?,
        }))
    }));
    let raws: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "model": id, "raw_names": raws })).into_response()
}

pub(crate) async fn list_calls(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(100).min(1000);
    let tcol = crate::api::time_column(p.allocation_mode.as_deref());
    // cursor 分页：基于 (时间, id) 排序键的游标
    let (sql, args): (String, Vec<SqlValue>) = if let Some(cur) = &p.cursor {
        match crate::api::decode_cursor(cur) {
            Some((ts, id)) => (
                format!(
                    "SELECT id, client_id, session_id, provider_normalized, model_normalized, started_at, status,
                        input_tokens, output_tokens, cache_read_tokens, reasoning_tokens,
                        reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd
                     FROM model_calls WHERE {tcol} >= ?1 AND {tcol} < ?2
                       AND ({tcol} < ?3 OR ({tcol} = ?3 AND id < ?4))
                     ORDER BY {tcol} DESC, id DESC LIMIT ?5"
                ),
                vec![
                    SqlValue::Text(from.to_rfc3339()),
                    SqlValue::Text(to.to_rfc3339()),
                    SqlValue::Text(ts),
                    SqlValue::Text(id),
                    SqlValue::Integer(limit),
                ],
            ),
            None => {
                return json_err(
                    StatusCode::BAD_REQUEST,
                    "invalid_cursor",
                    "分页游标无效",
                )
            }
        }
    } else {
        (
            format!(
                "SELECT id, client_id, session_id, provider_normalized, model_normalized, started_at, status,
                    input_tokens, output_tokens, cache_read_tokens, reasoning_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd
                 FROM model_calls WHERE {tcol} >= ?1 AND {tcol} < ?2
                 ORDER BY {tcol} DESC, id DESC LIMIT ?3"
            ),
            vec![
                SqlValue::Text(from.to_rfc3339()),
                SqlValue::Text(to.to_rfc3339()),
                SqlValue::Integer(limit),
            ],
        )
    };
    let mut stmt = q!(c.prepare(&sql));
    let rows = q!(stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok(serde_json::json!({
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
            "reasoning_tokens": r.get::<_, Option<i64>>(10)?,
            "reported_cost_micro_usd": r.get::<_, Option<i64>>(11)?,
            "calculated_cost_micro_usd": r.get::<_, Option<i64>>(12)?,
            "estimated_cost_micro_usd": r.get::<_, Option<i64>>(13)?,
        }))
    },));
    let calls: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    // 生成下一页游标（取最后一条）
    let next_cursor = calls.last().and_then(|v| {
        let id = v.get("id")?.as_str()?;
        let ts = v.get("started_at")?.as_str()?;
        Some(crate::api::encode_cursor(ts, id))
    });
    Json(serde_json::json!({ "calls": calls, "next_cursor": next_cursor })).into_response()
}

pub(crate) async fn call_detail(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let call = c
        .query_row(
            "SELECT id, client_id, session_id, provider_raw, provider_normalized, model_raw, model_normalized,
                started_at, completed_at, duration_ms, status, call_granularity,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd
             FROM model_calls WHERE id = ?1",
            [&id],
            |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "client_id": r.get::<_, String>(1)?,
                    "session_id": r.get::<_, String>(2)?,
                    "provider_raw": r.get::<_, Option<String>>(3)?,
                    "provider": r.get::<_, Option<String>>(4)?,
                    "model_raw": r.get::<_, Option<String>>(5)?,
                    "model": r.get::<_, Option<String>>(6)?,
                    "started_at": r.get::<_, String>(7)?,
                    "completed_at": r.get::<_, Option<String>>(8)?,
                    "duration_ms": r.get::<_, Option<i64>>(9)?,
                    "status": r.get::<_, String>(10)?,
                    "call_granularity": r.get::<_, String>(11)?,
                    "input_tokens": r.get::<_, Option<i64>>(12)?,
                    "output_tokens": r.get::<_, Option<i64>>(13)?,
                    "cache_read_tokens": r.get::<_, Option<i64>>(14)?,
                    "cache_write_tokens": r.get::<_, Option<i64>>(15)?,
                    "reasoning_tokens": r.get::<_, Option<i64>>(16)?,
                    "reported_cost_micro_usd": r.get::<_, Option<i64>>(17)?,
                    "calculated_cost_micro_usd": r.get::<_, Option<i64>>(18)?,
                    "estimated_cost_micro_usd": r.get::<_, Option<i64>>(19)?,
                }))
            },
        )
        .ok()
        .unwrap_or(serde_json::json!({}));
    let traffic = c
        .query_row(
            "SELECT estimated_request_wire_bytes, estimated_response_wire_bytes, estimated_total_wire_bytes, lower_bound_bytes, upper_bound_bytes, estimation_source, context_transport_mode, cache_transport_behavior, confidence
             FROM traffic_estimates WHERE model_call_id = ?1",
            [&id],
            |r| {
                Ok(serde_json::json!({
                    "estimated_request_wire_bytes": r.get::<_, Option<i64>>(0)?,
                    "estimated_response_wire_bytes": r.get::<_, Option<i64>>(1)?,
                    "estimated_total_wire_bytes": r.get::<_, Option<i64>>(2)?,
                    "lower_bound_bytes": r.get::<_, Option<i64>>(3)?,
                    "upper_bound_bytes": r.get::<_, Option<i64>>(4)?,
                    "estimation_source": r.get::<_, String>(5)?,
                    "context_transport_mode": r.get::<_, String>(6)?,
                    "cache_transport_behavior": r.get::<_, String>(7)?,
                    "confidence": r.get::<_, Option<f64>>(8)?,
                }))
            },
        )
        .ok()
        .unwrap_or(serde_json::json!({}));
    Json(serde_json::json!({ "call": call, "traffic": traffic })).into_response()
}

pub(crate) async fn list_sessions(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(100).min(1000);
    let tcol = crate::api::time_column(p.allocation_mode.as_deref());
    let (sql, args): (String, Vec<SqlValue>) = if let Some(cur) = &p.cursor {
        match crate::api::decode_cursor(cur) {
            Some((ts, id)) => (
                format!(
                    "SELECT id, source_session_id, client_id, title, provider_normalized, primary_model_normalized, started_at, ended_at,
                        message_count, tool_call_count, model_call_count, input_tokens, output_tokens,
                        reported_cost_micro_usd, estimated_total_bytes
                     FROM sessions WHERE {tcol} >= ?1 AND {tcol} < ?2
                       AND ({tcol} < ?3 OR ({tcol} = ?3 AND id < ?4))
                     ORDER BY {tcol} DESC, id DESC LIMIT ?5"
                ),
                vec![
                    SqlValue::Text(from.to_rfc3339()),
                    SqlValue::Text(to.to_rfc3339()),
                    SqlValue::Text(ts),
                    SqlValue::Text(id),
                    SqlValue::Integer(limit),
                ],
            ),
            None => return json_err(StatusCode::BAD_REQUEST, "invalid_cursor", "分页游标无效"),
        }
    } else {
        (
            format!(
                "SELECT id, source_session_id, client_id, title, provider_normalized, primary_model_normalized, started_at, ended_at,
                    message_count, tool_call_count, model_call_count, input_tokens, output_tokens,
                    reported_cost_micro_usd, estimated_total_bytes
                 FROM sessions WHERE {tcol} >= ?1 AND {tcol} < ?2
                 ORDER BY {tcol} DESC, id DESC LIMIT ?3"
            ),
            vec![
                SqlValue::Text(from.to_rfc3339()),
                SqlValue::Text(to.to_rfc3339()),
                SqlValue::Integer(limit),
            ],
        )
    };
    let mut stmt = q!(c.prepare(&sql));
    let rows = q!(stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "source_session_id": r.get::<_, String>(1)?,
            "client_id": r.get::<_, String>(2)?,
            "title": r.get::<_, Option<String>>(3)?,
            "provider": r.get::<_, Option<String>>(4)?,
            "model": r.get::<_, Option<String>>(5)?,
            "started_at": r.get::<_, String>(6)?,
            "ended_at": r.get::<_, Option<String>>(7)?,
            "message_count": r.get::<_, i64>(8)?,
            "tool_call_count": r.get::<_, i64>(9)?,
            "model_call_count": r.get::<_, i64>(10)?,
            "input_tokens": r.get::<_, Option<i64>>(11)?,
            "output_tokens": r.get::<_, Option<i64>>(12)?,
            "reported_cost_micro_usd": r.get::<_, Option<i64>>(13)?,
            "estimated_total_bytes": r.get::<_, Option<i64>>(14)?,
        }))
    },));
    let sessions: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    let next_cursor = sessions.last().and_then(|v| {
        let id = v.get("id")?.as_str()?;
        let ts = v.get("started_at")?.as_str()?;
        Some(crate::api::encode_cursor(ts, id))
    });
    Json(serde_json::json!({ "sessions": sessions, "next_cursor": next_cursor })).into_response()
}

pub(crate) async fn session_detail(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let session = c
        .query_row(
            "SELECT id, source_session_id, node_id, client_id, project_id, title, provider_normalized, primary_model_normalized,
                started_at, ended_at, message_count, tool_call_count, subagent_count, model_call_count,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                estimated_request_bytes, estimated_response_bytes, estimated_total_bytes, traffic_confidence
             FROM sessions WHERE id = ?1",
            [&id],
            |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source_session_id": r.get::<_, String>(1)?,
                    "node_id": r.get::<_, String>(2)?,
                    "client_id": r.get::<_, String>(3)?,
                    "project_id": r.get::<_, Option<String>>(4)?,
                    "title": r.get::<_, Option<String>>(5)?,
                    "provider": r.get::<_, Option<String>>(6)?,
                    "model": r.get::<_, Option<String>>(7)?,
                    "started_at": r.get::<_, String>(8)?,
                    "ended_at": r.get::<_, Option<String>>(9)?,
                    "message_count": r.get::<_, i64>(10)?,
                    "tool_call_count": r.get::<_, i64>(11)?,
                    "subagent_count": r.get::<_, i64>(12)?,
                    "model_call_count": r.get::<_, i64>(13)?,
                    "input_tokens": r.get::<_, Option<i64>>(14)?,
                    "output_tokens": r.get::<_, Option<i64>>(15)?,
                    "cache_read_tokens": r.get::<_, Option<i64>>(16)?,
                    "cache_write_tokens": r.get::<_, Option<i64>>(17)?,
                    "reasoning_tokens": r.get::<_, Option<i64>>(18)?,
                    "reported_cost_micro_usd": r.get::<_, Option<i64>>(19)?,
                    "calculated_cost_micro_usd": r.get::<_, Option<i64>>(20)?,
                    "estimated_cost_micro_usd": r.get::<_, Option<i64>>(21)?,
                    "estimated_request_bytes": r.get::<_, Option<i64>>(22)?,
                    "estimated_response_bytes": r.get::<_, Option<i64>>(23)?,
                    "estimated_total_bytes": r.get::<_, Option<i64>>(24)?,
                    "traffic_confidence": r.get::<_, Option<f64>>(25)?,
                }))
            },
        )
        .ok()
        .unwrap_or(serde_json::json!({}));
    Json(serde_json::json!({ "session": session })).into_response()
}

pub(crate) async fn session_calls(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT m.id, m.model_normalized, m.provider_normalized, m.started_at, m.status, m.input_tokens, m.output_tokens, m.cache_read_tokens, m.reasoning_tokens, m.calculated_cost_micro_usd, t.estimated_total_wire_bytes
         FROM model_calls m LEFT JOIN traffic_estimates t ON t.model_call_id = m.id WHERE m.session_id = ?1 ORDER BY m.started_at",
    ));
    let rows = q!(stmt.query_map([&id], |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "model": r.get::<_, Option<String>>(1)?,
            "provider": r.get::<_, Option<String>>(2)?,
            "started_at": r.get::<_, String>(3)?,
            "status": r.get::<_, String>(4)?,
            "input_tokens": r.get::<_, Option<i64>>(5)?,
            "output_tokens": r.get::<_, Option<i64>>(6)?,
            "cache_read_tokens": r.get::<_, Option<i64>>(7)?,
            "reasoning_tokens": r.get::<_, Option<i64>>(8)?,
            "calculated_cost_micro_usd": r.get::<_, Option<i64>>(9)?,
            "estimated_total_bytes": r.get::<_, Option<i64>>(10)?,
        }))
    }));
    let calls: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "calls": calls })).into_response()
}

pub(crate) async fn session_tools(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT id, name, tool_type, status, input_length, output_length, started_at, completed_at, error FROM tool_events WHERE session_id = ?1 ORDER BY started_at",
    ));
    let rows = q!(stmt.query_map([&id], |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "name": r.get::<_, String>(1)?,
            "tool_type": r.get::<_, String>(2)?,
            "status": r.get::<_, String>(3)?,
            "input_length": r.get::<_, i64>(4)?,
            "output_length": r.get::<_, i64>(5)?,
            "started_at": r.get::<_, String>(6)?,
            "completed_at": r.get::<_, Option<String>>(7)?,
            "error": r.get::<_, Option<String>>(8)?,
        }))
    }));
    let tools: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "tools": tools })).into_response()
}

pub(crate) async fn session_subagents(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT id, child_session_id, relation, created_at
         FROM subagent_relations WHERE session_id = ?1 ORDER BY created_at",
    ));
    let rows = q!(stmt.query_map([&id], |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "child_session_id": r.get::<_, String>(1)?,
            "relation": r.get::<_, String>(2)?,
            "created_at": r.get::<_, String>(3)?,
        }))
    }));
    let rels: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();

    // 解析子会话摘要（按 id 或 source_session_id 匹配）
    let child_ids: Vec<String> = rels
        .iter()
        .filter_map(|r| {
            r.get("child_session_id")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    let children: Vec<serde_json::Value> = if child_ids.is_empty() {
        Vec::new()
    } else {
        let placeholders = child_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, source_session_id, title, primary_model_normalized, message_count, model_call_count, input_tokens, output_tokens, estimated_total_bytes
             FROM sessions WHERE id IN ({placeholders}) OR source_session_id IN ({placeholders})"
        );
        let mut params: Vec<&str> = Vec::new();
        for id in &child_ids {
            params.push(id);
        }
        for id in &child_ids {
            params.push(id);
        }
        let mut stmt = q!(c.prepare(&sql));
        let rows = q!(
            stmt.query_map(params_from_iter(params.iter().copied()), |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source_session_id": r.get::<_, String>(1)?,
                    "title": r.get::<_, Option<String>>(2)?,
                    "model": r.get::<_, Option<String>>(3)?,
                    "message_count": r.get::<_, i64>(4)?,
                    "model_call_count": r.get::<_, i64>(5)?,
                    "input_tokens": r.get::<_, Option<i64>>(6)?,
                    "output_tokens": r.get::<_, Option<i64>>(7)?,
                    "estimated_total_bytes": r.get::<_, Option<i64>>(8)?,
                }))
            })
        );
        rows.filter_map(|r| r.ok()).collect()
    };

    Json(serde_json::json!({ "relations": rels, "children": children })).into_response()
}

pub(crate) async fn session_timeline(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT id, role, content_type, content, content_length, utf8_bytes, created_at, redacted FROM messages WHERE session_id = ?1 ORDER BY sequence",
    ));
    let rows = q!(stmt.query_map([&id], |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "role": r.get::<_, String>(1)?,
            "content_type": r.get::<_, String>(2)?,
            "content": r.get::<_, Option<String>>(3)?,
            "content_length": r.get::<_, i64>(4)?,
            "utf8_bytes": r.get::<_, i64>(5)?,
            "created_at": r.get::<_, String>(6)?,
            "redacted": r.get::<_, i64>(7)?,
        }))
    }));
    let messages: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "messages": messages })).into_response()
}

pub(crate) async fn traffic_summary(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let c = st.db.conn();
    let row = c.query_row(
        &format!(
            "SELECT COALESCE(SUM(estimated_request_bytes),0), COALESCE(SUM(estimated_response_bytes),0), COALESCE(SUM(estimated_total_bytes),0),
                COALESCE(SUM(estimated_lower_bound_bytes),0), COALESCE(SUM(estimated_upper_bound_bytes),0), COALESCE(SUM(model_call_count),0)
             FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 {filter}"
        ),
        params_from_iter(range_args(&from, &to, fargs)),
        |r| {
            Ok(serde_json::json!({
                "estimated_request_bytes": r.get::<_, i64>(0)?,
                "estimated_response_bytes": r.get::<_, i64>(1)?,
                "estimated_total_bytes": r.get::<_, i64>(2)?,
                "lower_bound_bytes": r.get::<_, i64>(3)?,
                "upper_bound_bytes": r.get::<_, i64>(4)?,
                "model_calls": r.get::<_, i64>(5)?,
            }))
        },
    );
    Json(row.unwrap_or_else(|_| serde_json::json!({}))).into_response()
}

macro_rules! traffic_by_dim {
    ($name:ident, $dim:expr) => {
        pub(crate) async fn $name(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
            let (from, to) = parse_range(&p);
            let (filter, fargs) = range_filter(&p);
            let c = st.db.conn();
            let mut stmt = q!(c.prepare(&format!(
                "SELECT $dim AS d,
                    COALESCE(SUM(estimated_request_bytes),0), COALESCE(SUM(estimated_response_bytes),0), COALESCE(SUM(estimated_total_bytes),0),
                    COALESCE(SUM(estimated_lower_bound_bytes),0), COALESCE(SUM(estimated_upper_bound_bytes),0), COALESCE(SUM(model_call_count),0)
                 FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 {filter} GROUP BY d ORDER BY 4 DESC"
            )));
            let rows = q!(stmt.query_map(
                params_from_iter(range_args(&from, &to, fargs)),
                |r| {
                    Ok(serde_json::json!({
                        "dimension": r.get::<_, String>(0)?,
                        "estimated_request_bytes": r.get::<_, i64>(1)?,
                        "estimated_response_bytes": r.get::<_, i64>(2)?,
                        "estimated_total_bytes": r.get::<_, i64>(3)?,
                        "lower_bound_bytes": r.get::<_, i64>(4)?,
                        "upper_bound_bytes": r.get::<_, i64>(5)?,
                        "model_calls": r.get::<_, i64>(6)?,
                    }))
                },
            ));
            let items: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
            Json(serde_json::json!({ "items": items })).into_response()
        }
    };
}

traffic_by_dim!(traffic_by_node, "node_id");
traffic_by_dim!(traffic_by_client, "client_id");
traffic_by_dim!(traffic_by_model, "model");
traffic_by_dim!(traffic_by_provider, "provider");

pub(crate) async fn data_quality(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();

    let mut usage_dist = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT usage_source, COALESCE(SUM(input_tokens),0), COALESCE(SUM(model_call_count),0) FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 GROUP BY usage_source",
    ) {
        if let Ok(rows) = stmt.query_map(
            params![from.to_rfc3339(), to.to_rfc3339()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
        ) {
            for row in rows.flatten() {
                usage_dist.push(serde_json::json!({
                    "usage_source": row.0,
                    "tokens": row.1,
                    "calls": row.2,
                }));
            }
        }
    }

    let mut traffic_dist = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT traffic_estimation_source, COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0) FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 AND traffic_estimation_source != '' GROUP BY traffic_estimation_source",
    ) {
        if let Ok(rows) = stmt.query_map(
            params![from.to_rfc3339(), to.to_rfc3339()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
        ) {
            for row in rows.flatten() {
                traffic_dist.push(serde_json::json!({
                    "estimation_source": row.0,
                    "bytes": row.1,
                    "calls": row.2,
                }));
            }
        }
    }

    let parse_warnings = c
        .query_row("SELECT COUNT(*) FROM source_errors", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0);

    Json(serde_json::json!({
        "usage_distribution": usage_dist,
        "traffic_distribution": traffic_dist,
        "parse_warnings": parse_warnings,
    }))
    .into_response()
}
