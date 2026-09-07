//! 查询 API handlers：overview / nodes / clients / models / calls / sessions / traffic / data-quality。
//!
//! 作为 `api` 模块的子模块，通过 `crate::api::*` 复用类型与工具函数。

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use metria_storage::rusqlite::{params, params_from_iter, types::Value as SqlValue};

use crate::api::{
    add_exclusions, json_err, parse_range, range_args, range_filter, AppState, RangeParams,
};
use crate::q;

/// 会话闲置阈值：最后活跃超过该分钟数视为闲置。
const IDLE_SESSION_MINUTES: i64 = 30;

pub(crate) async fn overview(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let (call_filter, call_fargs) = range_filter_usage(&p);
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
    // S3.3：AgentTool(Client)/Model/Project 计数（范围过滤）
    let range_clause = "started_at >= ?1 AND started_at < ?2";
    body["agent_tools"] = c
        .query_row(
            &format!(
                "SELECT COUNT(DISTINCT client_id) FROM model_calls WHERE {range_clause} {call_filter}"
            ),
            params_from_iter(range_args(&from, &to, call_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .into();
    body["models"] = c
        .query_row(
            &format!(
                "SELECT COUNT(DISTINCT model_normalized) FROM model_calls WHERE {range_clause} AND model_normalized IS NOT NULL AND model_normalized != '' {call_filter}"
            ),
            params_from_iter(range_args(&from, &to, call_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .into();
    body["projects"] = c
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0)
        .into();
    // S3.3：Collector 在线状态列表（最近心跳 ≤ 5 分钟视为在线）
    let online_window = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
    body["collectors_online"] = c
        .query_row(
            "SELECT COUNT(*) FROM collectors WHERE last_heartbeat_at IS NOT NULL AND last_heartbeat_at >= ?1",
            [&online_window],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .into();
    // S3.9：范围内失败调用数（status 非成功）。数据源缺失时返回 0，前端诚实标注。
    body["failed_calls"] = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM model_calls WHERE {range_clause} AND status != 'success' AND status != 'ok' AND status != 'completed' {call_filter}"
            ),
            params_from_iter(range_args(&from, &to, call_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .into();
    // S3.9：范围内平均调用延迟（duration_ms 均值的毫秒数）。数据缺失时返回 null，前端诚实标注。
    body["avg_duration_ms"] = c
        .query_row(
            &format!(
                "SELECT AVG(duration_ms) FROM model_calls WHERE {range_clause} AND duration_ms IS NOT NULL {call_filter}"
            ),
            params_from_iter(range_args(&from, &to, call_fargs.clone())),
            |r| r.get::<_, Option<f64>>(0),
        )
        .unwrap_or(None)
        .map(|v| v.round() as i64)
        .map(|v| serde_json::json!(v))
        .unwrap_or(serde_json::Value::Null);
    // S3.9：范围内调用时长分位数（duration_ms 的 p50/p95/p99，毫秒）。数据缺失时返回 null。
    {
        let mut durations: Vec<i64> = Vec::new();
        if let Ok(mut stmt) = c.prepare(&format!(
            "SELECT duration_ms FROM model_calls WHERE {range_clause} AND duration_ms IS NOT NULL {call_filter}"
        )) {
            if let Ok(mut rows) = stmt.query(params_from_iter(range_args(
                &from,
                &to,
                call_fargs.clone(),
            ))) {
                while let Ok(Some(row)) = rows.next() {
                    if let Ok(v) = row.get::<_, i64>(0) {
                        durations.push(v);
                    }
                }
            }
        }
        durations.sort_unstable();
        let pct = |p: f64| -> serde_json::Value {
            if durations.is_empty() {
                return serde_json::Value::Null;
            }
            let idx = ((durations.len() as f64 - 1.0) * p).round() as usize;
            serde_json::json!(durations[idx.min(durations.len() - 1)])
        };
        body["duration_p50_ms"] = pct(0.50);
        body["duration_p95_ms"] = pct(0.95);
        body["duration_p99_ms"] = pct(0.99);
    }
    // S3.9：缓存读取节省费用 = 范围内各模型 cache_read_tokens × 匹配规则的缓存读取单价。
    // c 在此之后不再使用；先 drop 释放 Mutex 锁，load_all_rules 才能安全取锁（Mutex 不可重入）。
    std::mem::drop(c);
    let cache_savings: i64 = {
        let mut total = 0i64;
        // 收集范围内各模型的缓存读取 token（需重新拿锁，块作用域结束后释放）
        let model_cache: Vec<(String, i64)> = {
            let c2 = st.db.conn();
            let mut stmt = q!(c2.prepare(&format!(
                "SELECT model_normalized, COALESCE(SUM(cache_read_tokens),0) FROM model_calls WHERE {range_clause} AND cache_read_tokens IS NOT NULL AND cache_read_tokens > 0 {call_filter} GROUP BY model_normalized"
            )));
            let rows = q!(stmt.query_map(
                params_from_iter(range_args(&from, &to, call_fargs.clone())),
                |r| Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, i64>(1)?,
                )),
            ));
            rows.filter_map(|r| r.ok()).collect()
        };
        // 加载启用规则（此时未持有 conn 锁）
        let rules = st.db.load_all_rules();
        let mut engine = metria_pricing::PricingEngine::new();
        for r in rules {
            engine.add_rule(r);
        }
        for (model, cache_tokens) in model_cache {
            let usage = metria_core::model::Usage {
                input: None,
                output: None,
                cache_read: Some(cache_tokens),
                cache_write: None,
                reasoning: None,
            };
            if let Ok(cost) = engine.compute(&usage, Some(&model), None, from, None) {
                if let Some(calc) = cost.calculated_micro_usd {
                    total += calc;
                } else if let Some(est) = cost.estimated_micro_usd {
                    total += est;
                }
            }
        }
        total
    };
    body["cache_savings_micro_usd"] = serde_json::json!(cache_savings);
    Json(body).into_response()
}

pub(crate) async fn usage_timeseries(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let bucket_secs = bucket_granularity(&p, from, to);
    // 仅细粒度（<1h）从原始事件表分桶，其余走 rollup
    let use_raw = bucket_secs < 3600;

    // 维度列：raw 路径从 usage_events(u) 分桶，需要 u. 前缀避免与
    // LEFT JOIN 的 traffic_estimates 同名列歧义；rollup 路径列名不带前缀。
    let (dim_col, dim_group) = match p.dim.as_deref() {
        Some("client") => {
            if use_raw {
                ("u.client_id", "u.client_id")
            } else {
                ("client_id", "client_id")
            }
        }
        Some("model") => {
            if use_raw {
                ("u.model_normalized", "u.model_normalized")
            } else {
                ("model", "model")
            }
        }
        Some("provider") => {
            if use_raw {
                ("u.provider_normalized", "u.provider_normalized")
            } else {
                ("provider", "provider")
            }
        }
        Some("node") => {
            if use_raw {
                ("u.node_id", "u.node_id")
            } else {
                ("node_id", "node_id")
            }
        }
        Some("project") => {
            if use_raw {
                ("u.project_id", "u.project_id")
            } else {
                ("project_id", "project_id")
            }
        }
        _ => ("''", ""),
    };
    let dim_sql = format!(", {dim_col} AS dimension");
    let group_sql = if dim_group.is_empty() {
        "b".to_string()
    } else {
        format!("b, {dim_group}")
    };

    let (filter, fargs) = if use_raw {
        range_filter_usage(&p)
    } else {
        range_filter(&p)
    };
    // raw 路径带 LEFT JOIN，过滤条件需加 u. 前缀消除同名列歧义
    let prefixed_filter = if use_raw {
        prefix_usage_filter(&filter)
    } else {
        filter.clone()
    };

    let c = st.db.conn();
    let points: Vec<serde_json::Value> = if use_raw {
        // 原始事件细粒度分桶（usage_events + traffic_estimates 关联字节）
        // 所有列加 u. 前缀，避免与 LEFT JOIN 的 traffic_estimates 列名歧义
        let mut stmt = q!(c.prepare(&format!(
            "SELECT strftime('%Y-%m-%dT%H:%M:%S+00:00',
                        datetime((CAST(strftime('%s', u.timestamp) AS INTEGER) / {bucket_secs}) * {bucket_secs}, 'unixepoch')) AS b{dim_sql},
                COALESCE(SUM(u.input_tokens),0), COALESCE(SUM(u.output_tokens),0),
                COALESCE(SUM(u.cache_read_tokens),0), COALESCE(SUM(u.cache_write_tokens),0),
                COALESCE(SUM(COALESCE(u.reported_cost_micro_usd,0)+COALESCE(u.calculated_cost_micro_usd,0)+COALESCE(u.estimated_cost_micro_usd,0)),0),
                COALESCE(SUM(t.estimated_total_wire_bytes),0),
                COUNT(*)
             FROM usage_events u LEFT JOIN traffic_estimates t ON t.model_call_id = u.model_call_id
             WHERE u.timestamp >= ?1 AND u.timestamp < ?2 {prefixed_filter}
             GROUP BY {group_sql} ORDER BY b"
        )));
        let rows = q!(
            stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
                Ok(serde_json::json!({
                    "bucket": r.get::<_, String>(0)?,
                    "dimension": r.get::<_, Option<String>>(1)?,
                    "input_tokens": r.get::<_, i64>(2)?,
                    "output_tokens": r.get::<_, i64>(3)?,
                    "cache_read_tokens": r.get::<_, i64>(4)?,
                    "cache_write_tokens": r.get::<_, i64>(5)?,
                    "cost_micro_usd": r.get::<_, i64>(6)?,
                    "estimated_traffic_bytes": r.get::<_, i64>(7)?,
                    "model_calls": r.get::<_, i64>(8)?,
                }))
            },)
        );
        rows.filter_map(|r| r.ok()).collect()
    } else {
        let table = if bucket_secs >= 86400 {
            "daily_rollups"
        } else {
            "hourly_rollups"
        };
        let bucket_col = if bucket_secs >= 86400 {
            "substr(bucket,1,10)"
        } else {
            "bucket"
        };
        let mut stmt = q!(c.prepare(&format!(
            "SELECT {bucket_col} AS b{dim_sql},
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
                COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0)
             FROM {table} WHERE bucket >= ?1 AND bucket < ?2 {filter}
             GROUP BY {group_sql} ORDER BY b"
        )));
        let rows = q!(
            stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
                Ok(serde_json::json!({
                    "bucket": r.get::<_, String>(0)?,
                    "dimension": r.get::<_, Option<String>>(1)?,
                    "input_tokens": r.get::<_, i64>(2)?,
                    "output_tokens": r.get::<_, i64>(3)?,
                    "cache_read_tokens": r.get::<_, i64>(4)?,
                    "cache_write_tokens": r.get::<_, i64>(5)?,
                    "cost_micro_usd": r.get::<_, i64>(6)?,
                    "estimated_traffic_bytes": r.get::<_, i64>(7)?,
                    "model_calls": r.get::<_, i64>(8)?,
                }))
            },)
        );
        rows.filter_map(|r| r.ok()).collect()
    };

    let has_dim = !dim_group.is_empty();
    let filled = fill_timeseries(points, from, to, bucket_secs, has_dim);
    Json(serde_json::json!({ "series": filled })).into_response()
}

/// 自适应分桶粒度，保证短范围也有足够连续点；延迟序列与 usage_timeseries 共用。
fn bucket_granularity(p: &RangeParams, from: DateTime<Utc>, to: DateTime<Utc>) -> i64 {
    let span_hours = to.signed_duration_since(from).num_hours();
    let explicit_day = p.granularity.as_deref() == Some("day");
    if explicit_day {
        86400
    } else if span_hours <= 3 {
        5 * 60
    } else if span_hours <= 24 {
        15 * 60
    } else if span_hours <= 24 * 45 {
        3600
    } else {
        86400
    }
}

/// usage_events 表专用范围过滤（列名为 model_normalized / provider_normalized）。
fn range_filter_usage(p: &RangeParams) -> (String, Vec<SqlValue>) {
    let mut parts = Vec::new();
    let mut args: Vec<SqlValue> = Vec::new();
    if let Some(v) = &p.node_id {
        args.push(v.clone().into());
        parts.push(format!("node_id = ?{}", args.len() + 2));
    }
    if let Some(v) = &p.client_id {
        args.push(v.clone().into());
        parts.push(format!("client_id = ?{}", args.len() + 2));
    }
    if let Some(v) = &p.model {
        args.push(v.clone().into());
        parts.push(format!("model_normalized = ?{}", args.len() + 2));
    }
    add_exclusions(
        &mut parts,
        &mut args,
        "client_id",
        p.exclude_client_ids.as_deref(),
    );
    add_exclusions(
        &mut parts,
        &mut args,
        "model_normalized",
        p.exclude_models.as_deref(),
    );
    if let Some(v) = &p.provider {
        args.push(v.clone().into());
        parts.push(format!("provider_normalized = ?{}", args.len() + 2));
    }
    let cond = if parts.is_empty() {
        String::new()
    } else {
        format!(" AND {}", parts.join(" AND "))
    };
    (cond, args)
}

/// 为 raw 分桶查询（usage_events u LEFT JOIN traffic_estimates t）的过滤条件
/// 加 u. 前缀，避免两表同名列（client_id/node_id/model_normalized 等）歧义。
fn prefix_usage_filter(filter: &str) -> String {
    // 仅替换过滤条件中出现的裸列名；条件形如 " AND node_id = ?3 AND client_id = ?4"
    filter
        .replace("node_id = ", "u.node_id = ")
        .replace("client_id = ", "u.client_id = ")
        .replace("client_id NOT IN", "u.client_id NOT IN")
        .replace("model_normalized = ", "u.model_normalized = ")
        .replace("model_normalized NOT IN", "u.model_normalized NOT IN")
        .replace("provider_normalized = ", "u.provider_normalized = ")
}

/// 把 timeseries 补齐为范围内完整 bucket 序列（无数据的时段零填充），
/// 避免 x 轴在空闲时段跳档。
fn fill_timeseries(
    points: Vec<serde_json::Value>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    bucket_secs: i64,
    has_dim: bool,
) -> Vec<serde_json::Value> {
    let mut by_bucket: std::collections::HashMap<String, Vec<serde_json::Value>> =
        Default::default();
    for p in points {
        if let Some(b) = p.get("bucket").and_then(|b| b.as_str()) {
            by_bucket.entry(b.to_string()).or_default().push(p);
        }
    }
    let mut cur = floor_ts(from, bucket_secs);
    let end = floor_ts(to, bucket_secs);
    let mut out = Vec::new();
    while cur <= end {
        let key = if bucket_secs >= 86400 {
            cur.format("%Y-%m-%d").to_string()
        } else {
            cur.to_rfc3339()
        };
        if let Some(rows) = by_bucket.get(&key) {
            out.extend(rows.iter().cloned());
        } else {
            out.push(zero_point(&key, has_dim));
        }
        cur += chrono::Duration::seconds(bucket_secs);
    }
    out
}

fn floor_ts(ts: DateTime<Utc>, bucket_secs: i64) -> DateTime<Utc> {
    let secs = ts.timestamp();
    DateTime::from_timestamp(secs - secs.rem_euclid(bucket_secs), 0).unwrap_or(ts)
}

fn zero_point(bucket: &str, _has_dim: bool) -> serde_json::Value {
    serde_json::json!({
        "bucket": bucket,
        "dimension": serde_json::Value::Null,
        "input_tokens": 0,
        "output_tokens": 0,
        "cache_read_tokens": 0,
        "cache_write_tokens": 0,
        "cost_micro_usd": 0,
        "estimated_traffic_bytes": 0,
        "model_calls": 0,
    })
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
    // 排除空 dimension（如 model=''）的零值汇总行，避免排行出现空结果。
    let null_filter = match p.dim.as_deref() {
        Some("model") => " AND model IS NOT NULL AND model != ''",
        Some("client") => " AND client_id IS NOT NULL AND client_id != ''",
        Some("provider") => " AND provider IS NOT NULL AND provider != ''",
        Some("project") => " AND project_id IS NOT NULL AND project_id != ''",
        _ => " AND node_id IS NOT NULL AND node_id != ''",
    };
    let mut stmt = q!(c.prepare(&format!(
        "SELECT {col}, COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
            COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
            COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0),
            COALESCE(SUM(session_count),0), COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
            COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_cost),0), COALESCE(SUM(reported_cost),0)
         FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 {filter}{null_filter}
         GROUP BY {col} ORDER BY 9 DESC"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok(serde_json::json!({
                "dimension": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "cache_read_tokens": r.get::<_, i64>(3)?,
                "cache_write_tokens": r.get::<_, i64>(4)?,
                "estimated_traffic_bytes": r.get::<_, i64>(5)?,
                "model_calls": r.get::<_, i64>(6)?,
                "sessions": r.get::<_, i64>(7)?,
                "cost_micro_usd": r.get::<_, i64>(8)?,
                "calculated_cost_micro_usd": r.get::<_, i64>(9)?,
                "estimated_cost_micro_usd": r.get::<_, i64>(10)?,
                "reported_cost_micro_usd": r.get::<_, i64>(11)?,
            }))
        },)
    );
    let items: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "by": items, "dimension": key })).into_response()
}

pub(crate) async fn list_nodes(State(st): State<AppState>) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT n.id, n.name, n.platform, n.architecture, n.timezone, n.status, n.first_seen_at, n.last_seen_at,
            (SELECT COUNT(DISTINCT client_id) FROM sources s WHERE s.node_id = n.id),
            (SELECT COUNT(*) FROM collectors col WHERE col.node_id = n.id),
            n.description, n.labels, n.ip
         FROM nodes n ORDER BY n.last_seen_at DESC",
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
            "detected_clients": r.get::<_, i64>(8)?,
            "collector_count": r.get::<_, i64>(9)?,
            "description": r.get::<_, Option<String>>(10)?,
            "labels": r.get::<_, Option<String>>(11)?,
            "ip": r.get::<_, Option<String>>(12)?,
        }))
    }));
    let nodes: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "nodes": nodes })).into_response()
}

pub(crate) async fn node_detail(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let node = c
        .query_row(
            "SELECT id, name, platform, architecture, timezone, status, first_seen_at, last_seen_at, agent_url, last_pull_at, last_pull_error FROM nodes WHERE id = ?1",
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
                    "agent_url": r.get::<_, Option<String>>(8)?,
                    "last_pull_at": r.get::<_, Option<String>>(9)?,
                    "last_pull_error": r.get::<_, Option<String>>(10)?,
                }))
            },
        )
        .ok()
        .unwrap_or(serde_json::json!({}));
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
    // Collector 信息（S3.4）
    let collectors: Vec<serde_json::Value> = {
        let st = c.prepare(
            "SELECT id, agent_version, protocol_version, container_image, status,
                    last_heartbeat_at, last_upload_at, spool_pending_events, spool_size_bytes, clock_skew_seconds
             FROM collectors WHERE node_id = ?1 ORDER BY created_at",
        );
        let mut out = Vec::new();
        if let Ok(mut st) = st {
            if let Ok(rows) = st.query_map([&id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "agent_version": r.get::<_, String>(1)?,
                    "protocol_version": r.get::<_, i64>(2)?,
                    "container_image": r.get::<_, Option<String>>(3)?,
                    "status": r.get::<_, String>(4)?,
                    "last_heartbeat_at": r.get::<_, Option<String>>(5)?,
                    "last_upload_at": r.get::<_, Option<String>>(6)?,
                    "spool_pending_events": r.get::<_, i64>(7)?,
                    "spool_size_bytes": r.get::<_, i64>(8)?,
                    "clock_skew_seconds": r.get::<_, i64>(9)?,
                }))
            }) {
                out = rows.filter_map(|x| x.ok()).collect();
            }
        }
        out
    };

    // S3.4：按 Client 分布（范围过滤，含 usage 汇总）
    let mut clients = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT s.client_id, COUNT(DISTINCT s.id) as src_count,
                COALESCE(SUM(mc.model_call_count),0)
         FROM sources s LEFT JOIN sessions se ON se.node_id = s.node_id AND se.client_id = s.client_id
         LEFT JOIN hourly_rollups mc ON mc.node_id = s.node_id AND mc.client_id = s.client_id AND mc.bucket >= ?2 AND mc.bucket < ?3
         WHERE s.node_id = ?1 GROUP BY s.client_id",
    ) {
        if let Ok(rows) = stmt.query_map(
            params![id, from.to_rfc3339(), to.to_rfc3339()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
        ) {
            for row in rows.flatten() {
                clients.push(serde_json::json!({
                    "client_id": row.0, "source_count": row.1, "model_calls": row.2
                }));
            }
        }
    }

    // S3.4：时间范围统计（token/cost/traffic/calls/sessions/cache）
    let range_summary = c
        .query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
                    COALESCE(SUM(estimated_total_bytes),0),
                    COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0)
             FROM hourly_rollups WHERE node_id = ?1 AND bucket >= ?2 AND bucket < ?3",
            params![id, from.to_rfc3339(), to.to_rfc3339()],
            |r| {
                Ok(serde_json::json!({
                    "input_tokens": r.get::<_, i64>(0)?,
                    "output_tokens": r.get::<_, i64>(1)?,
                    "cost_micro_usd": r.get::<_, i64>(2)?,
                    "estimated_total_bytes": r.get::<_, i64>(3)?,
                    "model_calls": r.get::<_, i64>(4)?,
                    "sessions": r.get::<_, i64>(5)?,
                    "cache_read_tokens": r.get::<_, i64>(6)?,
                    "cache_write_tokens": r.get::<_, i64>(7)?,
                }))
            },
        )
        .unwrap_or(serde_json::json!({}));

    Json(serde_json::json!({
        "node": node,
        "clients": clients,
        "collectors": collectors,
        "by_model": by_model,
        "by_project": by_project,
        "range_summary": range_summary,
        "range": { "from": from.to_rfc3339(), "to": to.to_rfc3339() },
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
    let tcol = crate::api::time_column(p.allocation_mode.as_deref());
    let (sql, args): (String, Vec<SqlValue>) = if let Some(cur) = &p.cursor {
        match crate::api::decode_cursor(cur) {
            Some((ts, sid)) => (
                format!(
                    "SELECT id, source_session_id, client_id, title, primary_model_normalized, started_at, ended_at, message_count, model_call_count, input_tokens, output_tokens, estimated_total_bytes
                     FROM sessions WHERE node_id = ?1 AND {tcol} >= ?2 AND {tcol} < ?3
                       AND ({tcol} < ?4 OR ({tcol} = ?4 AND id < ?5))
                     ORDER BY {tcol} DESC, id DESC LIMIT ?6",
                ),
                vec![
                    SqlValue::Text(id.clone()),
                    SqlValue::Text(from.to_rfc3339()),
                    SqlValue::Text(to.to_rfc3339()),
                    SqlValue::Text(ts),
                    SqlValue::Text(sid),
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
                "SELECT id, source_session_id, client_id, title, primary_model_normalized, started_at, ended_at, message_count, model_call_count, input_tokens, output_tokens, estimated_total_bytes
                 FROM sessions WHERE node_id = ?1 AND {tcol} >= ?2 AND {tcol} < ?3 ORDER BY {tcol} DESC, id DESC LIMIT ?4",
            ),
            vec![
                SqlValue::Text(id),
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
            "model": r.get::<_, Option<String>>(4)?,
            "started_at": r.get::<_, String>(5)?,
            "ended_at": r.get::<_, Option<String>>(6)?,
            "message_count": r.get::<_, i64>(7)?,
            "model_call_count": r.get::<_, i64>(8)?,
            "input_tokens": r.get::<_, Option<i64>>(9)?,
            "output_tokens": r.get::<_, Option<i64>>(10)?,
            "estimated_total_bytes": r.get::<_, Option<i64>>(11)?,
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

pub(crate) async fn node_calls(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(50).min(500);
    let tcol = crate::api::time_column(p.allocation_mode.as_deref());
    let (sql, args): (String, Vec<SqlValue>) = if let Some(cur) = &p.cursor {
        match crate::api::decode_cursor(cur) {
            Some((ts, cid)) => (
                format!(
                    "SELECT id, model_normalized, provider_normalized, started_at, status, input_tokens, output_tokens, cache_read_tokens, reasoning_tokens, reported_cost_micro_usd, calculated_cost_micro_usd
                     FROM model_calls WHERE node_id = ?1 AND {tcol} >= ?2 AND {tcol} < ?3
                       AND ({tcol} < ?4 OR ({tcol} = ?4 AND id < ?5))
                     ORDER BY {tcol} DESC, id DESC LIMIT ?6",
                ),
                vec![
                    SqlValue::Text(id.clone()),
                    SqlValue::Text(from.to_rfc3339()),
                    SqlValue::Text(to.to_rfc3339()),
                    SqlValue::Text(ts),
                    SqlValue::Text(cid),
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
                "SELECT id, model_normalized, provider_normalized, started_at, status, input_tokens, output_tokens, cache_read_tokens, reasoning_tokens, reported_cost_micro_usd, calculated_cost_micro_usd
                 FROM model_calls WHERE node_id = ?1 AND {tcol} >= ?2 AND {tcol} < ?3 ORDER BY {tcol} DESC, id DESC LIMIT ?4",
            ),
            vec![
                SqlValue::Text(id),
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
    },));
    let calls: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    let next_cursor = calls.last().and_then(|v| {
        let id = v.get("id")?.as_str()?;
        let ts = v.get("started_at")?.as_str()?;
        Some(crate::api::encode_cursor(ts, id))
    });
    Json(serde_json::json!({ "calls": calls, "next_cursor": next_cursor })).into_response()
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

    // 汇总（三口径 cost / 流量 / token / 缓存命中率）
    let summary = c
        .query_row(
            "SELECT COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_total_bytes),0),
                    COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0)
             FROM hourly_rollups WHERE client_id = ?1 AND bucket >= ?2 AND bucket < ?3",
            params![id, from.to_rfc3339(), to.to_rfc3339()],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )
        .unwrap_or((0, 0, 0, 0, 0, 0));
    let (calc_cost, est_bytes, in_tok, out_tok, cr_tok, cw_tok) = summary;

    // S3.5：按 Project 分布
    let by_project: Vec<serde_json::Value> = {
        let mut st = q!(c.prepare(
            "SELECT COALESCE(project_id,'(none)'), COUNT(*), COALESCE(SUM(model_call_count),0),
                    COALESCE(SUM(input_tokens),0), COALESCE(SUM(estimated_total_bytes),0)
             FROM sessions WHERE client_id = ?1 AND started_at >= ?2 AND started_at < ?3
             GROUP BY project_id ORDER BY 3 DESC LIMIT 10",
        ));
        let r = q!(
            st.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
                Ok(serde_json::json!({
                    "project_id": r.get::<_, String>(0)?,
                    "sessions": r.get::<_, i64>(1)?,
                    "model_calls": r.get::<_, i64>(2)?,
                    "input_tokens": r.get::<_, i64>(3)?,
                    "estimated_total_bytes": r.get::<_, i64>(4)?,
                }))
            },)
        );
        r.filter_map(|x| x.ok()).collect()
    };

    // S3.5：最近 Calls
    let recent_calls: Vec<serde_json::Value> = {
        let mut st = q!(c.prepare(
            "SELECT id, session_id, provider_normalized, model_normalized, started_at, status,
                    input_tokens, output_tokens, calculated_cost_micro_usd
             FROM model_calls WHERE client_id = ?1 AND started_at >= ?2 AND started_at < ?3
             ORDER BY started_at DESC LIMIT 20",
        ));
        let r = q!(
            st.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "session_id": r.get::<_, String>(1)?,
                    "provider": r.get::<_, Option<String>>(2)?,
                    "model": r.get::<_, Option<String>>(3)?,
                    "started_at": r.get::<_, String>(4)?,
                    "status": r.get::<_, String>(5)?,
                    "input_tokens": r.get::<_, Option<i64>>(6)?,
                    "output_tokens": r.get::<_, Option<i64>>(7)?,
                    "calculated_cost_micro_usd": r.get::<_, Option<i64>>(8)?,
                }))
            },)
        );
        r.filter_map(|x| x.ok()).collect()
    };

    // S3.5：Source 健康（总数/健康/错误）+ 版本分布
    let source_health = c
        .query_row(
            "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN last_error IS NOT NULL AND last_error != '' THEN 1 ELSE 0 END),0)
             FROM sources WHERE client_id = ?1",
            [&id],
            |r| {
                Ok(serde_json::json!({
                    "total": r.get::<_, i64>(0)?,
                    "healthy": r.get::<_, i64>(1)?,
                    "with_errors": r.get::<_, i64>(2)?,
                }))
            },
        )
        .unwrap_or(serde_json::json!({}));

    let version_dist: Vec<serde_json::Value> = {
        let mut st = q!(c.prepare(
            "SELECT COALESCE(adapter_version,'(unknown)'), COUNT(*)
             FROM sources WHERE client_id = ?1 GROUP BY adapter_version ORDER BY 2 DESC",
        ));
        let r = q!(st.query_map([&id], |r| {
            Ok(serde_json::json!({
                "version": r.get::<_, String>(0)?,
                "count": r.get::<_, i64>(1)?,
            }))
        }));
        r.filter_map(|x| x.ok()).collect()
    };

    Json(serde_json::json!({
        "client_id": id,
        "by_node": by_node,
        "by_project": by_project,
        "recent_sessions": recent,
        "recent_calls": recent_calls,
        "source_health": source_health,
        "version_dist": version_dist,
        "calculated_cost_micro_usd": calc_cost,
        "estimated_total_bytes": est_bytes,
        "input_tokens": in_tok,
        "output_tokens": out_tok,
        "cache_read_tokens": cr_tok,
        "cache_write_tokens": cw_tok,
    }))
    .into_response()
}

pub(crate) async fn client_models(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let models: Vec<serde_json::Value> = {
        // 块作用域：提前释放 conn 锁，避免与 load_all_rules 重入死锁
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
        rows.filter_map(|r| r.ok()).collect()
    };
    // 附带每个模型的实际定价来源，与费用计算共用同一匹配器。
    let mut pricing = metria_pricing::PricingEngine::new();
    for rule in st.db.load_all_rules() {
        pricing.add_rule(rule);
    }
    let models: Vec<serde_json::Value> = models
        .into_iter()
        .map(|mut m| {
            let model = m.get("model").and_then(|v| v.as_str()).unwrap_or("");
            let provider = m.get("provider").and_then(|v| v.as_str());
            let src = pricing
                .pricing_source(Some(model), provider, Utc::now())
                .unwrap_or_else(|| "unavailable".into());
            m["pricing_source"] = serde_json::json!(src);
            m
        })
        .collect();
    Json(serde_json::json!({ "models": models })).into_response()
}

pub(crate) async fn list_models(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let mut models: Vec<serde_json::Value> = {
        // 块作用域：提前释放 conn 锁，避免与 load_all_rules 重入死锁
        let c = st.db.conn();
        let mut stmt = q!(c.prepare(&format!(
            "SELECT model, MAX(provider),
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(estimated_total_bytes),0),
                COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0), COUNT(DISTINCT client_id), COUNT(DISTINCT node_id),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_cost),0), COALESCE(SUM(reported_cost),0)
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
                    "cache_read_tokens": r.get::<_, i64>(9)?,
                    "calculated_cost_micro_usd": r.get::<_, i64>(10)?,
                    "estimated_cost_micro_usd": r.get::<_, i64>(11)?,
                    "reported_cost_micro_usd": r.get::<_, i64>(12)?,
                }))
            },)
        );
        let mut models: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
        // S3.6：Bytes per Input/Output Token（估算流量 ÷ tokens，tokens 为 0 时置空）
        for m in &mut models {
            let in_t = m.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let out_t = m.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let bytes = m
                .get("estimated_traffic_bytes")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let bpi = if in_t > 0 {
                serde_json::json!((bytes as f64) / (in_t as f64))
            } else {
                serde_json::Value::Null
            };
            let bpo = if out_t > 0 {
                serde_json::json!((bytes as f64) / (out_t as f64))
            } else {
                serde_json::Value::Null
            };
            m["bytes_per_input_token"] = bpi;
            m["bytes_per_output_token"] = bpo;
            // 缓存命中率：cache_read / (input + cache_read)
            let cr = m
                .get("cache_read_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let cache_hit = if in_t + cr > 0 {
                serde_json::json!((cr as f64) / ((in_t + cr) as f64))
            } else {
                serde_json::Value::Null
            };
            m["cache_hit_rate"] = cache_hit;
        }
        models
    };
    // 每个模型从 model_calls 统计平均耗时与错误率（数据存在时）
    {
        let (mc_filter, mc_fargs) = range_filter_usage(&p);
        let c = st.db.conn();
        let mut stmt = q!(c.prepare(
            &format!(
                "SELECT model_normalized,
                        AVG(duration_ms), COALESCE(SUM(CASE WHEN status != 'success' AND status != 'ok' AND status != 'completed' THEN 1 ELSE 0 END),0), COUNT(*)
                 FROM model_calls WHERE model_normalized IS NOT NULL AND model_normalized != '' AND started_at >= ?1 AND started_at < ?2 {mc_filter}
                 GROUP BY model_normalized",
            ),
        ));
        let rows = q!(
            stmt.query_map(params_from_iter(range_args(&from, &to, mc_fargs)), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<f64>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
        );
        let mut meta: std::collections::HashMap<String, (Option<f64>, i64, i64)> =
            std::collections::HashMap::new();
        for row in rows.flatten() {
            meta.insert(row.0, (row.1, row.2, row.3));
        }
        for m in &mut models {
            let model = m.get("model").and_then(|v| v.as_str()).unwrap_or("");
            if let Some((avg_dur, err_cnt, total)) = meta.get(model) {
                m["avg_duration_ms"] = avg_dur
                    .map(|d| serde_json::json!(d.round() as i64))
                    .unwrap_or(serde_json::Value::Null);
                m["error_rate"] = serde_json::json!(if *total > 0 {
                    (*err_cnt as f64) / (*total as f64)
                } else {
                    0.0
                });
            } else {
                m["avg_duration_ms"] = serde_json::Value::Null;
                m["error_rate"] = serde_json::json!(0.0);
            }
        }
    }
    // 附带每个模型的实际定价来源，与费用计算共用同一匹配器。
    let mut pricing = metria_pricing::PricingEngine::new();
    for rule in st.db.load_all_rules() {
        pricing.add_rule(rule);
    }
    let models: Vec<serde_json::Value> = models
        .into_iter()
        .map(|mut m| {
            let model = m.get("model").and_then(|v| v.as_str()).unwrap_or("");
            let provider = m.get("provider").and_then(|v| v.as_str());
            let src = pricing
                .pricing_source(Some(model), provider, Utc::now())
                .unwrap_or_else(|| "unavailable".into());
            m["pricing_source"] = serde_json::json!(src);
            m
        })
        .collect();
    Json(serde_json::json!({ "models": models })).into_response()
}

pub(crate) async fn model_detail(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    // 块作用域：提前释放 conn 锁，避免与 list_pricing_rules 重入死锁
    let (raws, summary, recent_sessions, recent_calls, series) = {
        let c = st.db.conn();
        // raw 名称 + provider 分布
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

        // 汇总（Token/Cost/Traffic/Cache Hit）
        let summary = match c.query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                    COALESCE(SUM(reasoning_tokens),0),
                    COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
                    COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0),
                    COALESCE(SUM(session_count),0)
             FROM hourly_rollups WHERE model = ?1 AND bucket >= ?2 AND bucket < ?3",
            params![id, from.to_rfc3339(), to.to_rfc3339()],
            |r| {
                Ok(serde_json::json!({
                    "input_tokens": r.get::<_, i64>(0)?,
                    "output_tokens": r.get::<_, i64>(1)?,
                    "cache_read_tokens": r.get::<_, i64>(2)?,
                    "cache_write_tokens": r.get::<_, i64>(3)?,
                    "reasoning_tokens": r.get::<_, i64>(4)?,
                    "cost_micro_usd": r.get::<_, i64>(5)?,
                    "estimated_total_bytes": r.get::<_, i64>(6)?,
                    "model_calls": r.get::<_, i64>(7)?,
                    "sessions": r.get::<_, i64>(8)?,
                }))
            },
        ) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(%e, model=%id, "model_detail summary 查询失败");
                serde_json::json!({})
            }
        };

        // 最近 sessions / calls
        let recent_sessions: Vec<serde_json::Value> = {
            let mut st = q!(c.prepare(
                "SELECT id, source_session_id, client_id, title, primary_model_normalized, started_at,
                        model_call_count, input_tokens, output_tokens, cache_read_tokens, estimated_total_bytes
                 FROM sessions WHERE primary_model_normalized = ?1 AND started_at >= ?2 AND started_at < ?3
                 ORDER BY started_at DESC LIMIT 20",
            ));
            let r = q!(
                st.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "source_session_id": r.get::<_, String>(1)?,
                        "client_id": r.get::<_, String>(2)?,
                        "title": r.get::<_, Option<String>>(3)?,
                        "model": r.get::<_, Option<String>>(4)?,
                        "started_at": r.get::<_, String>(5)?,
                        "model_call_count": r.get::<_, i64>(6)?,
                        "input_tokens": r.get::<_, Option<i64>>(7)?,
                        "output_tokens": r.get::<_, Option<i64>>(8)?,
                        "cache_read_tokens": r.get::<_, Option<i64>>(9)?,
                        "estimated_total_bytes": r.get::<_, Option<i64>>(10)?,
                    }))
                },)
            );
            r.filter_map(|x| x.ok()).collect()
        };

        // S3.6：最近 Calls
        let recent_calls: Vec<serde_json::Value> = {
            let mut st = q!(c.prepare(
                "SELECT m.id, m.session_id, m.provider_normalized, m.model_normalized, m.started_at, m.status,
                        m.input_tokens, m.output_tokens, m.calculated_cost_micro_usd, t.estimated_total_wire_bytes
                 FROM model_calls m LEFT JOIN traffic_estimates t ON t.model_call_id = m.id
                 WHERE m.model_normalized = ?1 AND m.started_at >= ?2 AND m.started_at < ?3
                 ORDER BY m.started_at DESC LIMIT 20",
            ));
            let r = q!(
                st.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "session_id": r.get::<_, String>(1)?,
                        "provider": r.get::<_, Option<String>>(2)?,
                        "model": r.get::<_, Option<String>>(3)?,
                        "started_at": r.get::<_, String>(4)?,
                        "status": r.get::<_, String>(5)?,
                        "input_tokens": r.get::<_, Option<i64>>(6)?,
                        "output_tokens": r.get::<_, Option<i64>>(7)?,
                        "calculated_cost_micro_usd": r.get::<_, Option<i64>>(8)?,
                        "estimated_total_bytes": r.get::<_, Option<i64>>(9)?,
                    }))
                },)
            );
            r.filter_map(|x| x.ok()).collect()
        };

        // S3.6：Token/Cost/Traffic 时间序列（按小时）
        let series: Vec<serde_json::Value> = {
            let mut st = q!(c.prepare(
                "SELECT bucket,
                        COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                        COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
                        COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0)
                 FROM hourly_rollups WHERE model = ?1 AND bucket >= ?2 AND bucket < ?3
                 GROUP BY bucket ORDER BY bucket",
            ));
            let r = q!(
                st.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
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
            r.filter_map(|x| x.ok()).collect()
        };

        (raws, summary, recent_sessions, recent_calls, series)
    };

    // 匹配的定价规则
    let rules: Vec<serde_json::Value> = st
        .db
        .list_pricing_rules()
        .into_iter()
        .filter(|r| {
            let pat = r
                .get("model_pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            metria_pricing::model_matches_pattern(pat, &id)
        })
        .collect();
    let mut pricing = metria_pricing::PricingEngine::new();
    for rule in st.db.load_all_rules() {
        pricing.add_rule(rule);
    }
    let pricing_source = pricing
        .pricing_source(Some(&id), None, Utc::now())
        .unwrap_or_else(|| "unavailable".into());

    Json(serde_json::json!({
        "model": id,
        "pricing_source": pricing_source,
        "raw_names": raws,
        "summary": summary,
        "pricing_rules": rules,
        "recent_sessions": recent_sessions,
        "recent_calls": recent_calls,
        "series": series,
    }))
    .into_response()
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
                COALESCE(reported_cost_micro_usd, (SELECT u.reported_cost_micro_usd FROM usage_events u WHERE u.model_call_id = model_calls.id OR u.event_id = model_calls.usage_event_id LIMIT 1)),
                COALESCE(calculated_cost_micro_usd, (SELECT u.calculated_cost_micro_usd FROM usage_events u WHERE u.model_call_id = model_calls.id OR u.event_id = model_calls.usage_event_id LIMIT 1)),
                COALESCE(estimated_cost_micro_usd, (SELECT u.estimated_cost_micro_usd FROM usage_events u WHERE u.model_call_id = model_calls.id OR u.event_id = model_calls.usage_event_id LIMIT 1))
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
            "SELECT estimated_request_wire_bytes, estimated_response_wire_bytes, estimated_total_wire_bytes, lower_bound_bytes, upper_bound_bytes, estimation_source, context_transport_mode, cache_transport_behavior, confidence,
                    request_reconstruction_quality, response_reconstruction_quality, profile_id, profile_version
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
                    "request_reconstruction_quality": r.get::<_, Option<String>>(9)?,
                    "response_reconstruction_quality": r.get::<_, Option<String>>(10)?,
                    "profile_id": r.get::<_, Option<String>>(11)?,
                    "profile_version": r.get::<_, Option<i64>>(12)?,
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
    // 会话在所选时间范围内仍活跃（last_activity_at 落在范围内）也展示，
    // 而非仅按 started_at 过滤，避免"还在用"的会话从列表消失。
    let range_overlap = format!(
        "(({tcol} >= ?1 AND {tcol} < ?2) OR (last_activity_at >= ?1 AND last_activity_at < ?2))"
    );
    let (sql, args): (String, Vec<SqlValue>) = if let Some(cur) = &p.cursor {
        match crate::api::decode_cursor(cur) {
            Some((ts, id)) => (
                format!(
                    "SELECT id, source_session_id, client_id, node_id, title, provider_normalized, primary_model_normalized, started_at, ended_at, last_activity_at,
                        message_count, tool_call_count, model_call_count, input_tokens, output_tokens, cache_read_tokens,
                        reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                        estimated_total_bytes, status,
                        CASE WHEN COALESCE(last_activity_at, ended_at) IS NOT NULL AND started_at IS NOT NULL
                             THEN CAST((julianday(COALESCE(last_activity_at, ended_at)) - julianday(started_at)) * 86400000 AS INTEGER) END AS duration_ms
                     FROM sessions WHERE {range_overlap}
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
                "SELECT id, source_session_id, client_id, node_id, title, provider_normalized, primary_model_normalized, started_at, ended_at, last_activity_at,
                    message_count, tool_call_count, model_call_count, input_tokens, output_tokens, cache_read_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                    estimated_total_bytes, status,
                    CASE WHEN COALESCE(last_activity_at, ended_at) IS NOT NULL AND started_at IS NOT NULL
                         THEN CAST((julianday(COALESCE(last_activity_at, ended_at)) - julianday(started_at)) * 86400000 AS INTEGER) END AS duration_ms
                 FROM sessions WHERE {range_overlap}
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
            "node_id": r.get::<_, String>(3)?,
            "title": r.get::<_, Option<String>>(4)?,
            "provider": r.get::<_, Option<String>>(5)?,
            "model": r.get::<_, Option<String>>(6)?,
            "started_at": r.get::<_, String>(7)?,
            "ended_at": r.get::<_, Option<String>>(8)?,
            "last_activity_at": r.get::<_, Option<String>>(9)?,
            "message_count": r.get::<_, i64>(10)?,
            "tool_call_count": r.get::<_, i64>(11)?,
            "model_call_count": r.get::<_, i64>(12)?,
            "input_tokens": r.get::<_, Option<i64>>(13)?,
            "output_tokens": r.get::<_, Option<i64>>(14)?,
            "cache_read_tokens": r.get::<_, Option<i64>>(15)?,
            "reported_cost_micro_usd": r.get::<_, Option<i64>>(16)?,
            "calculated_cost_micro_usd": r.get::<_, Option<i64>>(17)?,
            "estimated_cost_micro_usd": r.get::<_, Option<i64>>(18)?,
            "estimated_total_bytes": r.get::<_, Option<i64>>(19)?,
            "status": r.get::<_, String>(20)?,
            "duration_ms": r.get::<_, Option<i64>>(21)?,
        }))
    },));
    let sessions: Vec<serde_json::Value> = rows
        .filter_map(|r| r.ok())
        .map(|mut v| {
            // 状态依据最后活跃时间判定，而非信任 adapter 标记：
            //   - last_activity_at 距今 ≤ IDLE 阈值 → active（活跃）
            //   - 距今 > 阈值 → idle（闲置）
            //   - ended_at 明确存在且无后续活动 → ended（已结束）
            let now = Utc::now();
            let idle_cutoff = now - chrono::Duration::minutes(IDLE_SESSION_MINUTES);
            let la = v
                .get("last_activity_at")
                .and_then(|x| x.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.with_timezone(&Utc));
            let ended_at = v
                .get("ended_at")
                .and_then(|x| x.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.with_timezone(&Utc));
            let status = match (la, ended_at) {
                (Some(la), _) if la >= idle_cutoff => "active",
                (Some(la), Some(ended)) if la <= ended => "ended",
                _ => "idle",
            };
            v["status"] = serde_json::json!(status);
            v
        })
        .collect();
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
                started_at, ended_at, last_activity_at, status,
                message_count, tool_call_count, subagent_count, model_call_count,
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
                    "last_activity_at": r.get::<_, Option<String>>(10)?,
                    "status": r.get::<_, String>(11)?,
                    "message_count": r.get::<_, i64>(12)?,
                    "tool_call_count": r.get::<_, i64>(13)?,
                    "subagent_count": r.get::<_, i64>(14)?,
                    "model_call_count": r.get::<_, i64>(15)?,
                    "input_tokens": r.get::<_, Option<i64>>(16)?,
                    "output_tokens": r.get::<_, Option<i64>>(17)?,
                    "cache_read_tokens": r.get::<_, Option<i64>>(18)?,
                    "cache_write_tokens": r.get::<_, Option<i64>>(19)?,
                    "reasoning_tokens": r.get::<_, Option<i64>>(20)?,
                    "reported_cost_micro_usd": r.get::<_, Option<i64>>(21)?,
                    "calculated_cost_micro_usd": r.get::<_, Option<i64>>(22)?,
                    "estimated_cost_micro_usd": r.get::<_, Option<i64>>(23)?,
                    "estimated_request_bytes": r.get::<_, Option<i64>>(24)?,
                    "estimated_response_bytes": r.get::<_, Option<i64>>(25)?,
                    "estimated_total_bytes": r.get::<_, Option<i64>>(26)?,
                    "traffic_confidence": r.get::<_, Option<f64>>(27)?,
                    "startup_command": serde_json::Value::Null,
                }))
            },
        )
        .ok()
        .unwrap_or(serde_json::json!({}));
    // 状态依据最后活跃时间判定，与列表一致。
    let mut session = session;
    if !session.is_null() {
        let now = Utc::now();
        let idle_cutoff = now - chrono::Duration::minutes(IDLE_SESSION_MINUTES);
        let la = session
            .get("last_activity_at")
            .and_then(|x| x.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc));
        let ended_at = session
            .get("ended_at")
            .and_then(|x| x.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc));
        let status = match (la, ended_at) {
            (Some(la), _) if la >= idle_cutoff => "active",
            (Some(la), Some(ended)) if la <= ended => "ended",
            _ => "idle",
        };
        session["status"] = serde_json::json!(status);
    }
    Json(serde_json::json!({ "session": session })).into_response()
}

pub(crate) async fn session_calls(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT m.id, m.model_normalized, m.provider_normalized, m.started_at, m.status, m.input_tokens, m.output_tokens, m.cache_read_tokens, m.reasoning_tokens,
                COALESCE(m.reported_cost_micro_usd, (SELECT u.reported_cost_micro_usd FROM usage_events u WHERE u.model_call_id = m.id OR u.event_id = m.usage_event_id LIMIT 1)),
                COALESCE(m.calculated_cost_micro_usd, (SELECT u.calculated_cost_micro_usd FROM usage_events u WHERE u.model_call_id = m.id OR u.event_id = m.usage_event_id LIMIT 1)),
                COALESCE(m.estimated_cost_micro_usd, (SELECT u.estimated_cost_micro_usd FROM usage_events u WHERE u.model_call_id = m.id OR u.event_id = m.usage_event_id LIMIT 1)),
                t.estimated_total_wire_bytes, m.duration_ms
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
            "reported_cost_micro_usd": r.get::<_, Option<i64>>(9)?,
            "calculated_cost_micro_usd": r.get::<_, Option<i64>>(10)?,
            "estimated_cost_micro_usd": r.get::<_, Option<i64>>(11)?,
            "estimated_total_bytes": r.get::<_, Option<i64>>(12)?,
            "duration_ms": r.get::<_, Option<i64>>(13)?,
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

    // 来源错误明细（S3.8）
    let source_errors: Vec<serde_json::Value> = {
        let st = c.prepare(
            "SELECT id, source_id, phase, severity, pattern, sample_count, first_seen_at, last_seen_at, last_message
             FROM source_errors ORDER BY last_seen_at DESC LIMIT 100",
        );
        let mut out = Vec::new();
        if let Ok(mut st) = st {
            if let Ok(rows) = st.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source_id": r.get::<_, String>(1)?,
                    "phase": r.get::<_, String>(2)?,
                    "severity": r.get::<_, String>(3)?,
                    "pattern": r.get::<_, String>(4)?,
                    "sample_count": r.get::<_, i64>(5)?,
                    "first_seen_at": r.get::<_, String>(6)?,
                    "last_seen_at": r.get::<_, String>(7)?,
                    "last_message": r.get::<_, String>(8)?,
                }))
            }) {
                out = rows.filter_map(|x| x.ok()).collect();
            }
        }
        out
    };

    // 来源扫描状态（S3.8）：总数/健康/最近扫描
    let source_scan = c
        .query_row(
            "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END),0),
                COALESCE(MAX(last_scan_at),''),
                COALESCE(SUM(CASE WHEN last_error IS NOT NULL AND last_error != '' THEN 1 ELSE 0 END),0)
             FROM sources",
            [],
            |r| {
                Ok(serde_json::json!({
                    "total": r.get::<_, i64>(0)?,
                    "healthy": r.get::<_, i64>(1)?,
                    "last_scan_at": r.get::<_, String>(2)?,
                    "with_errors": r.get::<_, i64>(3)?,
                }))
            },
        )
        .unwrap_or(serde_json::json!({}));

    // 时钟偏移告警（S3.8）：|skew| > 60s 视为异常
    let clock_skew_warns: Vec<serde_json::Value> = {
        let st = c.prepare(
            "SELECT id, node_id, clock_skew_seconds, last_heartbeat_at
             FROM collectors WHERE ABS(clock_skew_seconds) > 60 ORDER BY ABS(clock_skew_seconds) DESC LIMIT 50",
        );
        let mut out = Vec::new();
        if let Ok(mut st) = st {
            if let Ok(rows) = st.query_map([], |r| {
                Ok(serde_json::json!({
                    "collector_id": r.get::<_, String>(0)?,
                    "node_id": r.get::<_, String>(1)?,
                    "clock_skew_seconds": r.get::<_, i64>(2)?,
                    "last_heartbeat_at": r.get::<_, String>(3)?,
                }))
            }) {
                out = rows.filter_map(|x| x.ok()).collect();
            }
        }
        out
    };

    // S3.8：估算置信度占比（traffic_estimates.confidence 分级）
    let confidence_dist: Vec<serde_json::Value> = {
        let st = c.prepare(
            "SELECT CASE
                        WHEN confidence >= 0.8 THEN 'high'
                        WHEN confidence >= 0.5 THEN 'medium'
                        WHEN confidence IS NOT NULL THEN 'low'
                        ELSE 'unknown' END AS level,
                    COUNT(*)
             FROM traffic_estimates GROUP BY level",
        );
        let mut out = Vec::new();
        if let Ok(mut st) = st {
            if let Ok(rows) = st.query_map([], |r| {
                Ok(serde_json::json!({
                    "level": r.get::<_, String>(0)?,
                    "count": r.get::<_, i64>(1)?,
                }))
            }) {
                out = rows.filter_map(|x| x.ok()).collect();
            }
        }
        out
    };

    // S3.8：Source cursor 状态（来自 sources 表：scan/cursor 健康）
    let cursor_status: Vec<serde_json::Value> = {
        let st = c.prepare(
            "SELECT id, client_id, adapter_id, status, last_scan_at, last_error
             FROM sources ORDER BY last_scan_at DESC LIMIT 100",
        );
        let mut out = Vec::new();
        if let Ok(mut st) = st {
            if let Ok(rows) = st.query_map([], |r| {
                Ok(serde_json::json!({
                    "source_id": r.get::<_, String>(0)?,
                    "client_id": r.get::<_, String>(1)?,
                    "adapter_id": r.get::<_, String>(2)?,
                    "status": r.get::<_, String>(3)?,
                    "last_scan_at": r.get::<_, Option<String>>(4)?,
                    "last_error": r.get::<_, Option<String>>(5)?,
                }))
            }) {
                out = rows.filter_map(|x| x.ok()).collect();
            }
        }
        out
    };

    // S3.8：告警（spool 满/死信 → Hub 侧以严重 source_errors 与解析错误为代表）
    let alerts: Vec<serde_json::Value> = {
        let st = c.prepare(
            "SELECT id, source_id, phase, severity, pattern, sample_count, last_seen_at
             FROM source_errors WHERE severity IN ('error','critical') ORDER BY last_seen_at DESC LIMIT 50",
        );
        let mut out = Vec::new();
        if let Ok(mut st) = st {
            if let Ok(rows) = st.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source_id": r.get::<_, String>(1)?,
                    "phase": r.get::<_, String>(2)?,
                    "severity": r.get::<_, String>(3)?,
                    "pattern": r.get::<_, String>(4)?,
                    "sample_count": r.get::<_, i64>(5)?,
                    "last_seen_at": r.get::<_, String>(6)?,
                }))
            }) {
                out = rows.filter_map(|x| x.ok()).collect();
            }
        }
        out
    };

    Json(serde_json::json!({
        "usage_distribution": usage_dist,
        "traffic_distribution": traffic_dist,
        "parse_warnings": parse_warnings,
        "source_errors": source_errors,
        "source_scan": source_scan,
        "clock_skew_warnings": clock_skew_warns,
        "confidence_distribution": confidence_dist,
        "cursor_status": cursor_status,
        "alerts": alerts,
    }))
    .into_response()
}

/// 延迟分位数统计：范围内 model_calls.duration_ms 的 P50/P95/P99。
/// duration_ms 数据缺失（NULL）时返回空统计，前端诚实标注，不硬造。
pub(crate) async fn usage_latency(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let (mc_filter, mc_fargs) = range_filter_usage(&p);
    let c = st.db.conn();
    let mut durations: Vec<i64> = Vec::new();
    let mut stmt = q!(c.prepare(&format!(
        "SELECT duration_ms FROM model_calls WHERE duration_ms IS NOT NULL AND started_at >= ?1 AND started_at < ?2 {mc_filter}"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, mc_fargs)), |r| r
            .get::<_, i64>(0))
    );
    for row in rows.flatten() {
        durations.push(row);
    }
    durations.sort_unstable();
    let n = durations.len();
    let percentile = |q: f64| -> Option<i64> {
        if n == 0 {
            return None;
        }
        let idx = ((n as f64) * q).ceil() as usize;
        Some(durations[idx.saturating_sub(1).min(n - 1)])
    };
    Json(serde_json::json!({
        "count": n,
        "p50_ms": percentile(0.50),
        "p95_ms": percentile(0.95),
        "p99_ms": percentile(0.99),
        "avg_ms": if n > 0 { Some(durations.iter().sum::<i64>() / n as i64) } else { None },
    }))
    .into_response()
}

pub(crate) async fn usage_latency_timeseries(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let bucket_secs = bucket_granularity(&p, from, to);
    let (mc_filter, mc_fargs) = range_filter_usage(&p);
    let c = st.db.conn();

    // 按 started_at 分桶聚合 duration_ms；分位数在内存计算（与 usage_latency 一致），
    // 空桶由 fill_timeseries 补齐且统计字段为 null（诚实展示，不填充 0）。
    let mut stmt = q!(c.prepare(&format!(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S+00:00',
                    datetime((CAST(strftime('%s', started_at) AS INTEGER) / {bucket_secs}) * {bucket_secs}, 'unixepoch')) AS b,
                duration_ms
             FROM model_calls
             WHERE duration_ms IS NOT NULL AND started_at >= ?1 AND started_at < ?2 {mc_filter}
             ORDER BY b, duration_ms"
    )));
    let mut per_bucket: std::collections::HashMap<String, Vec<i64>> = Default::default();
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, mc_fargs)), |r| {
            let b: String = r.get(0)?;
            let d: i64 = r.get(1)?;
            Ok((b, d))
        })
    );
    for row in rows.flatten() {
        per_bucket.entry(row.0).or_default().push(row.1);
    }
    let percentile = |sorted: &[i64], q: f64| -> Option<i64> {
        let n = sorted.len();
        if n == 0 {
            return None;
        }
        let idx = ((n as f64) * q).ceil() as usize;
        Some(sorted[idx.saturating_sub(1).min(n - 1)])
    };

    let points: Vec<serde_json::Value> = per_bucket
        .into_iter()
        .map(|(bucket, mut values)| {
            values.sort_unstable();
            let count = values.len() as i64;
            let sum: i64 = values.iter().sum();
            serde_json::json!({
                "bucket": bucket,
                "count": count,
                "avg_ms": if count > 0 { Some(sum / count) } else { None },
                "p50_ms": percentile(&values, 0.50),
                "p95_ms": percentile(&values, 0.95),
                "p99_ms": percentile(&values, 0.99),
            })
        })
        .collect();
    let filled = fill_latency_timeseries(points, from, to, bucket_secs);
    Json(serde_json::json!({ "series": filled })).into_response()
}

/// 把延迟序列补齐为范围内完整 bucket 序列；缺失桶统计字段为 null。
fn fill_latency_timeseries(
    points: Vec<serde_json::Value>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    bucket_secs: i64,
) -> Vec<serde_json::Value> {
    let mut by_bucket: std::collections::HashMap<String, serde_json::Value> = Default::default();
    for p in points {
        if let Some(b) = p.get("bucket").and_then(|b| b.as_str()) {
            by_bucket.insert(b.to_string(), p);
        }
    }
    let mut cur = floor_ts(from, bucket_secs);
    let end = floor_ts(to, bucket_secs);
    let mut out = Vec::new();
    while cur <= end {
        let key = if bucket_secs >= 86400 {
            cur.format("%Y-%m-%d").to_string()
        } else {
            cur.to_rfc3339()
        };
        out.push(by_bucket.get(&key).cloned().unwrap_or_else(|| {
            serde_json::json!({ "bucket": key, "count": 0, "avg_ms": serde_json::Value::Null, "p50_ms": serde_json::Value::Null, "p95_ms": serde_json::Value::Null, "p99_ms": serde_json::Value::Null })
        }));
        cur += chrono::Duration::seconds(bucket_secs);
    }
    out
}
