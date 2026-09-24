//! 查询 API handlers：overview / nodes / clients / models / calls / sessions / data-quality。
//!
//! 作为 `api` 模块的子模块，通过 `crate::api::*` 复用类型与工具函数。

use crate::perfrollup::{ceil_hour, floor_hour};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use metria_storage::rusqlite::{params, params_from_iter, types::Value as SqlValue};

use crate::api::{
    add_exclusions, json_err, parse_range, range_args, range_filter, AppState, RangeParams,
};
use crate::db::HubDb;
use crate::q;

/// 会话闲置阈值：最后活跃超过该分钟数视为闲置。
const IDLE_SESSION_MINUTES: i64 = 30;

pub(crate) async fn overview(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let (call_filter, call_fargs) = range_filter_usage(&p);
    // 时长相关统计统一来自同一份聚合：完整小时读小时级预聚合、带维度筛选或预聚合
    // 缺桶时回退明细。必须在取得连接锁之前计算——内部会自行取锁，Mutex 不可重入。
    let perf_agg: Option<crate::perfrollup::PerfHour> =
        crate::perfrollup::fast_aggregate(&st.db, from, to, &call_filter).or_else(|| {
            crate::perfrollup::scan_calls(&st.db, from, to, &call_filter, &call_fargs).ok()
        });
    // 归档水位决定活动类字段能否从明细算出。必须在取得连接锁之前读取
    // （std::sync::Mutex 不可重入，持锁再取锁会把请求永久卡死）。
    let archived_before = crate::archive::watermark(&st.db);
    let activity_archived = archived_before
        .as_deref()
        .and_then(|w| DateTime::parse_from_rfc3339(w).ok())
        .map(|watermark| from < watermark)
        .unwrap_or(false);
    let activity_archived_before: Option<String> = if activity_archived {
        archived_before
    } else {
        None
    };
    let c = st.db.conn();
    // rollup 按 UTC 整点分桶：完整小时读 rollup，两端不完整小时用原始明细补齐，
    // 否则范围起点非整点时会把当前小时整桶丢弃（表现为卡片为 0）。
    let (h_from, h_to) = full_hour_bounds(from, to);
    let row = c.query_row(
        &format!(
            "SELECT
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(reasoning_tokens),0),
                COALESCE(SUM(reported_cost),0), COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_cost),0),
                COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0),
                COALESCE(SUM(message_count),0), COALESCE(SUM(tool_call_count),0)
             FROM hourly_rollups WHERE julianday(bucket) >= julianday(?1) AND julianday(bucket) < julianday(?2) {filter}"
        ),
        params_from_iter(range_args(&h_from, &h_to, fargs)),
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
                "model_calls": r.get::<_, i64>(8)?,
                "sessions": r.get::<_, i64>(9)?,
                "message_count": r.get::<_, i64>(10)?,
                "tool_call_count": r.get::<_, i64>(11)?,
            }))
        },
    );
    let mut body = row.unwrap_or_else(|e| {
        tracing::error!(%e, "overview 查询失败");
        serde_json::json!({})
    });
    add_overview_raw_edges(
        &mut body,
        &c,
        &p,
        from,
        to,
        h_from,
        h_to,
        &call_filter,
        &call_fargs,
    );
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
    body["agent_tools"] = match &perf_agg {
            // 由 perf_agg 的集合合并/逐小时计数直接给出，省掉整段全表扫描；
            // 取不到聚合时保留原 SQL 作为降级路径，绝不把缺失填成 0。
            Some(agg) => serde_json::json!(agg.distinct_clients.len()),
            None => c
        .query_row(
            &format!(
                "SELECT COUNT(DISTINCT client_id) FROM model_calls WHERE {range_clause} {call_filter}"
            ),
            params_from_iter(range_args(&from, &to, call_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .into()
        };
    body["models"] = match &perf_agg {
            // 由 perf_agg 的集合合并/逐小时计数直接给出，省掉整段全表扫描；
            // 取不到聚合时保留原 SQL 作为降级路径，绝不把缺失填成 0。
            Some(agg) => serde_json::json!(agg.distinct_models.len()),
            None => c
        .query_row(
            &format!(
                "SELECT COUNT(DISTINCT model_normalized) FROM model_calls WHERE {range_clause} AND model_normalized IS NOT NULL AND model_normalized != '' {call_filter}"
            ),
            params_from_iter(range_args(&from, &to, call_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .into()
        };
    body["projects"] = match &perf_agg {
        // 由 perf_agg 的集合合并/逐小时计数直接给出，省掉整段全表扫描；
        // 取不到聚合时保留原 SQL 作为降级路径，绝不把缺失填成 0。
        Some(agg) => serde_json::json!(agg.distinct_projects.len()),
        None => c
            .query_row(
                &format!(
                "SELECT COUNT(DISTINCT project_id) FROM model_calls
                 WHERE {range_clause} AND project_id IS NOT NULL AND project_id != '' {call_filter}"
            ),
                params_from_iter(range_args(&from, &to, call_fargs.clone())),
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .into(),
    };
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
    body["failed_calls"] = match &perf_agg {
            // 由 perf_agg 的集合合并/逐小时计数直接给出，省掉整段全表扫描；
            // 取不到聚合时保留原 SQL 作为降级路径，绝不把缺失填成 0。
            Some(agg) => serde_json::json!(agg.errors),
            None => c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM model_calls WHERE {range_clause} AND status != 'success' AND status != 'ok' AND status != 'completed' {call_filter}"
            ),
            params_from_iter(range_args(&from, &to, call_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .into()
        };
    // 计价分母与已计价条数由 perf_agg 给出（原先这里要对 17.5 万行做一次全表扫描，
    // 实测 74ms）；取不到聚合时保留原 SQL 降级，不把缺失填成 0。
    let (total_cost_calls, priced_calls) = match &perf_agg {
        Some(agg) => (agg.token_pair_call_count as i64, agg.priced_call_count as i64),
        None => c
            .query_row(
                &format!(
                    "SELECT COUNT(*), COALESCE(SUM(CASE WHEN reported_cost_micro_usd IS NOT NULL OR calculated_cost_micro_usd IS NOT NULL OR estimated_cost_micro_usd IS NOT NULL THEN 1 ELSE 0 END),0)
                     FROM model_calls WHERE {range_clause}
                       AND (input_tokens IS NOT NULL OR output_tokens IS NOT NULL) {call_filter}"
                ),
                params_from_iter(range_args(&from, &to, call_fargs.clone())),
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .unwrap_or((0, 0)),
    };
    // 未计价模型分组同样来自 perf_agg 的逐小时分组（原 SQL 每次刷新都要全表
    // 扫描 + GROUP BY，实测 85ms）；分组键保留原始 Option，渲染时才 COALESCE，
    // 与 SQL 的 GROUP BY + COALESCE 语义完全一致。
    let unpriced_models: Vec<serde_json::Value> = match &perf_agg {
        Some(agg) => {
            let mut rows: Vec<serde_json::Value> = agg
                .unpriced_by_model
                .iter()
                .map(|(model, provider, calls)| {
                    serde_json::json!({
                        "model": model.clone().unwrap_or_else(|| "(unknown)".to_string()),
                        "provider": provider.clone().unwrap_or_else(|| "(unknown)".to_string()),
                        "calls": calls,
                    })
                })
                .collect();
            rows.sort_by(|a, b| b["calls"].as_i64().cmp(&a["calls"].as_i64()));
            rows.truncate(8);
            rows
        }
        None => {
            let mut out = Vec::new();
            if let Ok(mut stmt) = c.prepare(&format!(
            "SELECT COALESCE(model_normalized,'(unknown)'), COALESCE(provider_normalized,'(unknown)'), COUNT(*)
             FROM model_calls WHERE {range_clause}
               AND (input_tokens IS NOT NULL OR output_tokens IS NOT NULL)
               AND reported_cost_micro_usd IS NULL AND calculated_cost_micro_usd IS NULL
               AND estimated_cost_micro_usd IS NULL {call_filter}
             GROUP BY model_normalized, provider_normalized ORDER BY COUNT(*) DESC LIMIT 8"
        )) {
            if let Ok(rows) = stmt.query_map(
                params_from_iter(range_args(&from, &to, call_fargs.clone())),
                |r| {
                    Ok(serde_json::json!({
                        "model": r.get::<_, String>(0)?,
                        "provider": r.get::<_, String>(1)?,
                        "calls": r.get::<_, i64>(2)?,
                    }))
                },
            ) {
                out = rows.filter_map(Result::ok).collect();
            }
        }
            out
        }
    };
    body["pricing_coverage"] = serde_json::json!({
        "priced_calls": priced_calls,
        "total_calls": total_cost_calls,
        "ratio": if total_cost_calls > 0 { Some(priced_calls as f64 / total_cost_calls as f64) } else { None },
        "unpriced_models": unpriced_models,
    });
    body["token_calls"] = match &perf_agg {
        // 由 perf_agg 的集合合并/逐小时计数直接给出，省掉整段全表扫描；
        // 取不到聚合时保留原 SQL 作为降级路径，绝不把缺失填成 0。
        Some(agg) => serde_json::json!(agg.token_call_count),
        None => c
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM model_calls WHERE {range_clause}
                 AND (input_tokens IS NOT NULL OR output_tokens IS NOT NULL
                      OR cache_read_tokens IS NOT NULL OR cache_write_tokens IS NOT NULL
                      OR reasoning_tokens IS NOT NULL) {call_filter}"
                ),
                params_from_iter(range_args(&from, &to, call_fargs.clone())),
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .into(),
    };
    // D1/D2：合计与平均按窗口裁剪，与「会话总时长」的边界裁剪口径一致。
    // `to` 不早于当前时刻时（今天/近 7 天等常用范围）没有调用能跨越窗口终点——
    // duration_ms 在调用完成后才写入、完成时刻 ≤ 当前时刻——裁剪是空操作，直接用
    // 预聚合，刷新成本不变；否则用一条走 started_at 索引的 SQL 求裁剪后的合计与计数，
    // 仅历史自定义范围触发（实测 40–150ms，仍在 ≤0.5s 的刷新预算内）。
    let duration_stats: Option<(i64, i64)> = if to >= Utc::now() {
        perf_agg
            .as_ref()
            .filter(|agg| agg.duration.count > 0)
            .map(|agg| (agg.duration.sum, agg.duration.count as i64))
    } else {
        match clipped_duration_stats(&c, from, to, &call_filter, &call_fargs) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("按窗口裁剪的调用时长查询失败，回退未裁剪值: {e}");
                perf_agg
                    .as_ref()
                    .filter(|agg| agg.duration.count > 0)
                    .map(|agg| (agg.duration.sum, agg.duration.count as i64))
            }
        }
    };
    // S3.9：平均调用延迟与 duration 分位数。
    // 平均值 = 裁剪后的合计 / 裁剪后的计数（D1/D2）；
    // 分位数仍来自完整调用时长分布的直方图——裁剪需按查询终点重建直方图，
    // 成本与复杂度不成比例，因此不参与裁剪、也不标注为窗口内裁剪值。
    body["avg_duration_ms"] = match duration_stats {
        Some((sum, count)) if count > 0 => {
            serde_json::json!((sum as f64 / count as f64).round() as i64)
        }
        _ => serde_json::Value::Null,
    };
    if let Some(agg) = &perf_agg {
        body["duration_p50_ms"] = serde_json::json!(agg.duration.quantile(0.50));
        body["duration_p95_ms"] = serde_json::json!(agg.duration.quantile(0.95));
        body["duration_p99_ms"] = serde_json::json!(agg.duration.quantile(0.99));
        body["quantile_approx"] = serde_json::json!(true);
    } else {
        // 取不到聚合（明细回退也失败）时仍要写键，前端应读到诚实的 null 而不是 undefined。
        body["duration_p50_ms"] = serde_json::Value::Null;
        body["duration_p95_ms"] = serde_json::Value::Null;
        body["duration_p99_ms"] = serde_json::Value::Null;
        body["quantile_approx"] = serde_json::Value::Null;
    }
    // 活跃时长 = 窗口内裁剪后的调用时长合计（与会话总时长同口径）；
    // 只统计明确记录了 duration_ms 的调用，没有样本时返回 null。
    body["active_duration_ms"] = match duration_stats {
        Some((sum, count)) if count > 0 => serde_json::json!(sum),
        _ => serde_json::Value::Null,
    };
    // 会话/消息/工具按“窗口内的活动与明细”统计，而不是按会话开始时间归属。
    let activity = overview_activity(&c, &p, from, to, activity_archived);
    // Option 直接落 JSON：归档区间为 null（诚实标注「已归档，不可计算」），
    // 否则是真实数值；绝不把不可算写成 0。
    let opt_json = |v: Option<i64>| {
        v.map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null)
    };
    body["sessions"] = opt_json(activity.sessions);
    body["active_sessions"] = opt_json(activity.active_sessions);
    body["message_count"] = opt_json(activity.messages);
    body["user_message_count"] = opt_json(activity.user_messages);
    body["tool_call_count"] = opt_json(activity.tools);
    body["session_duration_ms"] = opt_json(activity.session_duration_ms);
    // 前端据此把上面这些 null 显示为「已归档，不可计算」而不是「无数据」
    body["activity_archived_before"] = activity_archived_before
        .map(serde_json::Value::from)
        .unwrap_or(serde_json::Value::Null);
    body["freshness"] = freshness_summary(&c, &p);
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
    match query_usage_timeseries(&st.db, &p) {
        Ok(points) => Json(serde_json::json!({ "series": points })).into_response(),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error", &e),
    }
}

/// 全局筛选器选项：节点始终返回全部已登记节点；Agent 与模型只返回所选时间范围内
/// 有数据的项（`model_calls.started_at` 或 `usage_events.timestamp`），避免提供当前
/// 范围必然为空的候选项。选项不受当前筛选值影响，避免用户选择后选项把自己隐藏掉。
pub(crate) async fn usage_filter_options(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let bounds = [from.to_rfc3339(), to.to_rfc3339()];
    let c = st.db.conn();
    let mut node_stmt = q!(c.prepare("SELECT id, name FROM nodes ORDER BY name, id"));
    let node_rows = q!(node_stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "label": r.get::<_, String>(1)?,
        }))
    }));
    let nodes: Vec<serde_json::Value> = node_rows.filter_map(Result::ok).collect();

    let mut agent_stmt = q!(c.prepare(
        "SELECT id, MAX(label) AS label FROM (
             SELECT mc.client_id AS id, COALESCE(cl.display_name, mc.client_id) AS label
             FROM model_calls mc LEFT JOIN clients cl ON cl.id = mc.client_id
             WHERE mc.started_at >= ?1 AND mc.started_at < ?2
               AND mc.client_id IS NOT NULL AND mc.client_id != ''
             UNION ALL
             SELECT ue.client_id AS id, COALESCE(cl.display_name, ue.client_id) AS label
             FROM usage_events ue LEFT JOIN clients cl ON cl.id = ue.client_id
             WHERE ue.timestamp >= ?1 AND ue.timestamp < ?2
               AND ue.client_id IS NOT NULL AND ue.client_id != ''
         ) GROUP BY id ORDER BY label, id",
    ));
    let agent_rows = q!(agent_stmt.query_map(params_from_iter(bounds.clone()), |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "label": r.get::<_, String>(1)?,
        }))
    }));
    let agents: Vec<serde_json::Value> = agent_rows.filter_map(Result::ok).collect();

    let mut model_stmt = q!(c.prepare(
        "SELECT model_normalized FROM (
             SELECT model_normalized FROM model_calls
             WHERE started_at >= ?1 AND started_at < ?2
               AND model_normalized IS NOT NULL AND model_normalized != ''
             UNION
             SELECT model_normalized FROM usage_events
             WHERE timestamp >= ?1 AND timestamp < ?2
               AND model_normalized IS NOT NULL AND model_normalized != ''
         ) ORDER BY model_normalized",
    ));
    let model_rows = q!(model_stmt.query_map(params_from_iter(bounds), |r| {
        let value = r.get::<_, String>(0)?;
        Ok(serde_json::json!({ "id": value, "label": value }))
    }));
    let models: Vec<serde_json::Value> = model_rows.filter_map(Result::ok).collect();

    Json(serde_json::json!({ "nodes": nodes, "agents": agents, "models": models })).into_response()
}

/// 按用户时区聚合最近可观测的模型调用，返回固定 7×24 网格。
pub(crate) async fn usage_heatmap(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    match query_usage_heatmap(&st.db, &p) {
        Ok(body) => Json(body).into_response(),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error", &e),
    }
}

/// 热力图与日趋势共用的按小时数据源：完整小时读预聚合，头部/当前小时回退明细。
///
/// 以下情况整体回退到按小时分组的明细，宁可慢也不改口径：
///   * 带维度筛选（预聚合不带维度）；
///   * 预聚合缺行或 payload 版本过期；
///   * 时区偏移不是整小时（如 Asia/Kolkata +5:30），此时一个 UTC 整点桶会横跨
///     两个本地小时/日期，按桶映射会把数据算到错误的格子里。
fn hourly_heatmap_source(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    filter: &str,
    fargs: &[SqlValue],
    timezone: &Tz,
) -> Result<Vec<(String, crate::perfrollup::HeatmapAgg)>, String> {
    if let Some(list) = crate::perfrollup::fast_hours(db, from, to, filter) {
        let aligned = list.iter().all(|(bucket, _)| {
            DateTime::parse_from_rfc3339(&crate::perfrollup::hour_bucket(*bucket))
                .map(|ts| ts.with_timezone(timezone).minute() == 0)
                .unwrap_or(false)
        });
        if aligned {
            return Ok(list
                .into_iter()
                .map(|(bucket, agg)| (crate::perfrollup::hour_bucket(bucket), agg.heatmap))
                .collect());
        }
    }
    let grouped = crate::perfrollup::scan_heatmap_grouped(db, from, to, filter, fargs)
        .map_err(|e| e.to_string())?;
    Ok(grouped.into_iter().collect())
}

fn query_usage_heatmap(db: &HubDb, p: &RangeParams) -> Result<serde_json::Value, String> {
    let (from, to) = parse_range(p);
    let timezone = requested_timezone(p);
    let (filter, fargs) = range_filter_usage(p);

    let grouped = hourly_heatmap_source(db, from, to, &filter, &fargs, &timezone)?;

    let mut cells = (0..168)
        .map(|_| ActivityCell::default())
        .collect::<Vec<_>>();
    for (bucket, agg) in grouped {
        let Ok(started) = DateTime::parse_from_rfc3339(&bucket) else {
            continue;
        };
        let started = started.with_timezone(&Utc);
        let local = started.with_timezone(&timezone);
        let index = local.weekday().num_days_from_monday() as usize * 24 + local.hour() as usize;
        let cell = &mut cells[index];
        cell.input_tokens += agg.input_tokens;
        cell.output_tokens += agg.output_tokens;
        cell.cache_read_tokens += agg.cache_read_tokens;
        cell.cache_write_tokens += agg.cache_write_tokens;
        cell.reasoning_tokens += agg.reasoning_tokens;
        cell.cost_micro_usd += agg.reported_cost + agg.calculated_cost + agg.estimated_cost;
        cell.model_calls += agg.model_calls as i64;
        if agg.duration_count > 0 {
            cell.duration_ms = Some(cell.duration_ms.unwrap_or(0) + agg.duration_ms);
        }
        if let Some(latest) = agg
            .latest_started_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|ts| ts.with_timezone(&Utc))
        {
            if cell
                .latest_started_at
                .is_none_or(|current| latest > current)
            {
                cell.latest_started_at = Some(latest);
            }
        }
    }

    let cells = cells
        .into_iter()
        .enumerate()
        .map(|(index, cell)| {
            let weekday = index / 24;
            let hour = index % 24;
            let (latest_from, latest_to) = cell
                .latest_started_at
                .and_then(|value| local_hour_bounds(value, &timezone))
                .map_or((None, None), |(start, end)| (Some(start), Some(end)));
            serde_json::json!({
                "weekday": weekday,
                "hour": hour,
                "input_tokens": cell.input_tokens,
                "output_tokens": cell.output_tokens,
                "cache_read_tokens": cell.cache_read_tokens,
                "cache_write_tokens": cell.cache_write_tokens,
                "reasoning_tokens": cell.reasoning_tokens,
                "tokens": cell.input_tokens + cell.output_tokens + cell.reasoning_tokens
                    + cell.cache_read_tokens + cell.cache_write_tokens,
                "cost_micro_usd": cell.cost_micro_usd,
                "model_calls": cell.model_calls,
                "duration_ms": cell.duration_ms,
                "latest_from": latest_from,
                "latest_to": latest_to,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "timezone": timezone.to_string(),
        "cells": cells,
    }))
}

/// 按用户时区聚合每日分层数据。调用记录同时提供 Token、费用和 duration，
/// 因而不会用没有时长字段的 rollup 伪造“时长趋势”。
pub(crate) async fn usage_daily(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    match query_usage_daily(&st.db, &p) {
        Ok(series) => Json(serde_json::json!({ "series": series })).into_response(),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error", &e),
    }
}

fn query_usage_daily(db: &HubDb, p: &RangeParams) -> Result<Vec<serde_json::Value>, String> {
    let (from, to) = parse_range(p);
    let timezone = requested_timezone(p);
    let (filter, fargs) = range_filter_usage(p);
    // 与热力图共用按小时数据源：完整小时读预聚合（原先这里每行要跑 5 个相关子查询，
    // 30 天范围实测 1.16s），两端不整点片段与当前小时回退明细；时区偏移非整小时
    // 或带维度筛选时整体回退明细，保证本地日期归属正确。
    let hourly = hourly_heatmap_source(db, from, to, &filter, &fargs, &timezone)?;
    let mut by_day: std::collections::BTreeMap<String, DailyActivity> = Default::default();
    for (bucket, agg) in hourly {
        let Ok(started) = DateTime::parse_from_rfc3339(&bucket) else {
            continue;
        };
        let day = started.with_timezone(&timezone).date_naive().to_string();
        let entry = by_day.entry(day).or_default();
        entry.input_tokens += agg.input_tokens;
        entry.output_tokens += agg.output_tokens;
        entry.cache_read_tokens += agg.cache_read_tokens;
        entry.reasoning_tokens += agg.reasoning_tokens;
        entry.cache_write_tokens += agg.cache_write_tokens;
        entry.cost_micro_usd += agg.reported_cost + agg.calculated_cost + agg.estimated_cost;
        entry.model_calls += agg.model_calls as i64;
        if agg.duration_count > 0 {
            entry.duration_ms = Some(entry.duration_ms.unwrap_or(0) + agg.duration_ms);
        }
    }

    if from >= to {
        return Ok(Vec::new());
    }
    let first = from.with_timezone(&timezone).date_naive();
    let last = (to - Duration::nanoseconds(1))
        .with_timezone(&timezone)
        .date_naive();
    let mut day = first;
    let mut series = Vec::new();
    while day <= last {
        let key = day.to_string();
        let value = by_day.remove(&key).unwrap_or_default();
        series.push(serde_json::json!({
            "bucket": key,
            "input_tokens": value.input_tokens,
            "output_tokens": value.output_tokens,
            "cache_read_tokens": value.cache_read_tokens,
            "cache_write_tokens": value.cache_write_tokens,
            "reasoning_tokens": value.reasoning_tokens,
            "tokens": value.input_tokens + value.output_tokens + value.reasoning_tokens
                + value.cache_read_tokens + value.cache_write_tokens,
            "cost_micro_usd": value.cost_micro_usd,
            "model_calls": value.model_calls,
            "duration_ms": value.duration_ms,
        }));
        day += Duration::days(1);
    }
    Ok(series)
}

#[derive(Default)]
struct ActivityCell {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    cost_micro_usd: i64,
    model_calls: i64,
    duration_ms: Option<i64>,
    latest_started_at: Option<DateTime<Utc>>,
}

#[derive(Default)]
struct DailyActivity {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    cost_micro_usd: i64,
    model_calls: i64,
    duration_ms: Option<i64>,
}

fn requested_timezone(p: &RangeParams) -> Tz {
    p.timezone
        .as_deref()
        .and_then(|value| value.parse::<Tz>().ok())
        .unwrap_or(Tz::UTC)
}

fn local_hour_bounds(value: DateTime<Utc>, timezone: &Tz) -> Option<(String, String)> {
    let local = value.with_timezone(timezone);
    let naive = local.date_naive().and_hms_opt(local.hour(), 0, 0)?;
    let start = timezone
        .from_local_datetime(&naive)
        .earliest()?
        .with_timezone(&Utc);
    Some((
        start.to_rfc3339(),
        (start + Duration::hours(1)).to_rfc3339(),
    ))
}

fn available_cost(total: i64, priced_rows: i64) -> Option<i64> {
    (priced_rows > 0).then_some(total)
}

/// 缓存命中率：`cache_read / (input + cache_write + cache_read)`。
///
/// 无任何缓存数据或分母为 0 时返回 `None`（调用方展示为不可用），不硬造 0。
/// 前端口径与 `web/src/services/format.js:cacheHitRate` 保持一致。
fn cache_hit_rate(input: i64, cache_write: i64, cache_read: i64) -> Option<f64> {
    let cacheable = input + cache_write + cache_read;
    if cacheable <= 0 || (cache_read <= 0 && cache_write <= 0) {
        return None;
    }
    Some(cache_read as f64 / cacheable as f64)
}

/// 查询趋势数据，供 Web API 与邮件报告共用同一套分桶和补零规则。
pub(crate) fn query_usage_timeseries(
    db: &HubDb,
    p: &RangeParams,
) -> Result<Vec<serde_json::Value>, String> {
    let (from, to) = parse_range(p);
    let bucket_secs = bucket_granularity(p, from, to);
    // 仅细粒度（<1h）从原始事件表分桶，其余走 rollup
    let use_raw = bucket_secs < 3600;

    // 维度列：raw 路径从 usage_events(u) 分桶，需要 u. 前缀；rollup
    // 路径列名不带前缀。
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
        range_filter_usage(p)
    } else {
        range_filter(p)
    };
    // raw 路径使用 u/tm 前缀消除同名列歧义。
    let prefixed_filter = if use_raw {
        prefix_usage_filter(&filter)
    } else {
        filter.clone()
    };

    let c = db.conn();
    let points: Vec<serde_json::Value> = if use_raw {
        // 原始事件细粒度分桶；不读取历史估算流量表。
        let mut stmt = c.prepare(&format!(
            "SELECT strftime('%Y-%m-%dT%H:%M:%S+00:00',
                        datetime((CAST(strftime('%s', u.timestamp) AS INTEGER) / {bucket_secs}) * {bucket_secs}, 'unixepoch')) AS b{dim_sql},
                COALESCE(SUM(u.input_tokens),0), COALESCE(SUM(u.output_tokens),0),
                COALESCE(SUM(u.cache_read_tokens),0), COALESCE(SUM(u.cache_write_tokens),0),
                COALESCE(SUM(u.reasoning_tokens),0),
                COALESCE(SUM(COALESCE(u.reported_cost_micro_usd,0)+COALESCE(u.calculated_cost_micro_usd,0)+COALESCE(u.estimated_cost_micro_usd,0)),0),
                COUNT(*)
             FROM usage_events u
             LEFT JOIN model_calls tm ON tm.id = u.model_call_id
             WHERE u.timestamp >= ?1 AND u.timestamp < ?2 {prefixed_filter}
             GROUP BY {group_sql} ORDER BY b"
        ))
        .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
                Ok(serde_json::json!({
                    "bucket": r.get::<_, String>(0)?,
                    "dimension": r.get::<_, Option<String>>(1)?,
                    "input_tokens": r.get::<_, i64>(2)?,
                    "output_tokens": r.get::<_, i64>(3)?,
                    "cache_read_tokens": r.get::<_, i64>(4)?,
                    "cache_write_tokens": r.get::<_, i64>(5)?,
                    "reasoning_tokens": r.get::<_, i64>(6)?,
                    "cost_micro_usd": r.get::<_, i64>(7)?,
                    "model_calls": r.get::<_, i64>(8)?,
                }))
            })
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    } else {
        let (source, cte) = if bucket_secs >= 86400 {
            ("daily_rollups", String::new())
        } else {
            ("rollup_src", rollup_source_cte(1, 2, from, to))
        };
        let bucket_col = if bucket_secs >= 86400 {
            "substr(bucket,1,10)"
        } else {
            // 统一历史数据可能使用的 `Z` 与当前写入使用的 `+00:00`。
            "strftime('%Y-%m-%dT%H:%M:%S+00:00', bucket)"
        };
        let mut stmt = c
            .prepare(&format!(
                "{cte}SELECT {bucket_col} AS b{dim_sql},
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(reasoning_tokens),0),
                COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
                COALESCE(SUM(model_call_count),0)
             FROM {source} WHERE julianday(bucket) >= julianday(?1) AND julianday(bucket) < julianday(?2) {filter}
             GROUP BY {group_sql} ORDER BY b"
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
                Ok(serde_json::json!({
                    "bucket": r.get::<_, String>(0)?,
                    "dimension": r.get::<_, Option<String>>(1)?,
                    "input_tokens": r.get::<_, i64>(2)?,
                    "output_tokens": r.get::<_, i64>(3)?,
                    "cache_read_tokens": r.get::<_, i64>(4)?,
                    "cache_write_tokens": r.get::<_, i64>(5)?,
                    "reasoning_tokens": r.get::<_, i64>(6)?,
                    "cost_micro_usd": r.get::<_, i64>(7)?,
                    "model_calls": r.get::<_, i64>(8)?,
                }))
            })
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let has_dim = !dim_group.is_empty();
    Ok(fill_timeseries(points, from, to, bucket_secs, has_dim))
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
    if let Some(v) = &p.project_id {
        args.push(v.clone().into());
        parts.push(format!("project_id = ?{}", args.len() + 2));
    }
    let cond = if parts.is_empty() {
        String::new()
    } else {
        format!(" AND {}", parts.join(" AND "))
    };
    (cond, args)
}

/// sessions 表专用范围过滤，列名带 s. 以便复用于消息和会话时长查询。
fn range_filter_sessions(p: &RangeParams) -> (String, Vec<SqlValue>) {
    let mut parts = Vec::new();
    let mut args: Vec<SqlValue> = Vec::new();
    if let Some(v) = &p.node_id {
        args.push(v.clone().into());
        parts.push(format!("s.node_id = ?{}", args.len() + 2));
    }
    if let Some(v) = &p.client_id {
        args.push(v.clone().into());
        parts.push(format!("s.client_id = ?{}", args.len() + 2));
    }
    if let Some(v) = &p.model {
        args.push(v.clone().into());
        parts.push(format!("s.primary_model_normalized = ?{}", args.len() + 2));
    }
    add_exclusions(
        &mut parts,
        &mut args,
        "s.client_id",
        p.exclude_client_ids.as_deref(),
    );
    add_exclusions(
        &mut parts,
        &mut args,
        "s.primary_model_normalized",
        p.exclude_models.as_deref(),
    );
    if let Some(v) = &p.provider {
        args.push(v.clone().into());
        parts.push(format!("s.provider_normalized = ?{}", args.len() + 2));
    }
    if let Some(v) = &p.project_id {
        args.push(v.clone().into());
        parts.push(format!("s.project_id = ?{}", args.len() + 2));
    }
    let cond = if parts.is_empty() {
        String::new()
    } else {
        format!(" AND {}", parts.join(" AND "))
    };
    (cond, args)
}

fn source_scope_filter(p: &RangeParams) -> (String, Vec<SqlValue>) {
    let mut parts = Vec::new();
    let mut args: Vec<SqlValue> = Vec::new();
    if let Some(v) = &p.node_id {
        args.push(v.clone().into());
        parts.push(format!("s.node_id = ?{}", args.len()));
    }
    if let Some(v) = &p.client_id {
        args.push(v.clone().into());
        parts.push(format!("s.client_id = ?{}", args.len()));
    }
    let cond = if parts.is_empty() {
        String::new()
    } else {
        format!(" AND {}", parts.join(" AND "))
    };
    (cond, args)
}

fn collector_scope_filter(p: &RangeParams) -> (String, Vec<SqlValue>) {
    let mut parts = Vec::new();
    let mut args: Vec<SqlValue> = Vec::new();
    if let Some(v) = &p.node_id {
        args.push(v.clone().into());
        parts.push(format!("col.node_id = ?{}", args.len()));
    }
    if let Some(v) = &p.client_id {
        args.push(v.clone().into());
        parts.push(format!(
            "EXISTS (SELECT 1 FROM sources cs WHERE cs.collector_id = col.id AND cs.client_id = ?{})",
            args.len()
        ));
    }
    let cond = if parts.is_empty() {
        String::new()
    } else {
        format!(" AND {}", parts.join(" AND "))
    };
    (cond, args)
}

/// 返回与当前节点/Agent范围相关的采集健康状态。模型和项目不影响采集器本身的健康。
fn freshness_summary(
    c: &metria_storage::rusqlite::Connection,
    p: &RangeParams,
) -> serde_json::Value {
    let (source_filter, mut source_args) = source_scope_filter(p);
    let cutoff = (Utc::now() - Duration::minutes(10)).to_rfc3339();
    let cutoff_index = source_args.len() + 1;
    source_args.push(cutoff.into());
    let source = c
        .query_row(
            &format!(
                "SELECT
                    COALESCE(SUM(CASE WHEN s.status != 'missing' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN status = 'active' AND last_error IS NULL AND last_scan_at IS NOT NULL THEN 1 ELSE 0 END),0),
                    MAX(last_scan_at), MAX(last_event_at),
                    COALESCE(SUM(CASE WHEN s.status != 'missing' AND (last_scan_at IS NULL OR last_scan_at < ?{cutoff_index}) THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN last_error IS NOT NULL AND last_error != '' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN s.status = 'missing' THEN 1 ELSE 0 END),0)
                 FROM sources s WHERE 1 = 1 {source_filter}"
            ),
            params_from_iter(source_args),
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            },
        )
        .unwrap_or((0, 0, None, None, 0, 0, 0));

    let (collector_filter, mut collector_args) = collector_scope_filter(p);
    let online_index = collector_args.len() + 1;
    collector_args.push((Utc::now() - Duration::minutes(5)).to_rfc3339().into());
    let collectors = c
        .query_row(
            &format!(
                "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN col.last_heartbeat_at >= ?{online_index} THEN 1 ELSE 0 END),0),
                    MAX(col.last_upload_at)
                 FROM collectors col WHERE 1 = 1 {collector_filter}"
            ),
            params_from_iter(collector_args),
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .unwrap_or((0, 0, None));

    let status = if source.5 > 0 {
        "error"
    } else if source.4 > 0 {
        "delayed"
    } else if source.0 == 0 {
        "unavailable"
    } else {
        "fresh"
    };
    serde_json::json!({
        "status": status,
        "last_scan_at": source.2,
        "last_event_at": source.3,
        "last_upload_at": collectors.2,
        "source_total": source.0,
        "source_healthy": source.1,
        "source_stale": source.4,
        "source_errors": source.5,
        "source_missing": source.6,
        "coverage": if source.0 > 0 { Some(source.1 as f64 / source.0 as f64) } else { None },
        "collectors_total": collectors.0,
        "collectors_online": collectors.1,
    })
}

/// 为 raw 分桶查询（usage_events u LEFT JOIN model_calls tm）的过滤条件
/// 加 u/tm 前缀，避免两表同名列歧义。
fn prefix_usage_filter(filter: &str) -> String {
    // 仅替换过滤条件中出现的裸列名；条件形如 " AND node_id = ?3 AND client_id = ?4"
    filter
        .replace("node_id = ", "u.node_id = ")
        .replace("client_id = ", "u.client_id = ")
        .replace("client_id NOT IN", "u.client_id NOT IN")
        .replace("model_normalized = ", "u.model_normalized = ")
        .replace("model_normalized NOT IN", "u.model_normalized NOT IN")
        .replace("provider_normalized = ", "u.provider_normalized = ")
        .replace("project_id = ", "tm.project_id = ")
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
        "reasoning_tokens": 0,
        "cost_micro_usd": 0,
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
    let cte = rollup_source_cte(1, 2, from, to);
    let mut stmt = q!(c.prepare(&format!(
        "{cte}SELECT {col}, COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
            COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
            COALESCE(SUM(reasoning_tokens),0), COALESCE(SUM(model_call_count),0),
            COALESCE(SUM(session_count),0), COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
            COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_cost),0), COALESCE(SUM(reported_cost),0),
            COUNT(CASE WHEN pricing_source != '' THEN 1 END)
         FROM rollup_src WHERE 1=1 {filter}{null_filter}
         GROUP BY {col} ORDER BY 10 DESC"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok(serde_json::json!({
                "dimension": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "cache_read_tokens": r.get::<_, i64>(3)?,
                "cache_write_tokens": r.get::<_, i64>(4)?,
                "reasoning_tokens": r.get::<_, i64>(5)?,
                "model_calls": r.get::<_, i64>(6)?,
                "sessions": r.get::<_, i64>(7)?,
                "cost_micro_usd": available_cost(r.get::<_, i64>(8)?, r.get::<_, i64>(12)?),
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
        "SELECT COALESCE(model_normalized,'(unknown)'), COUNT(*), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(COALESCE(reported_cost_micro_usd,0)+COALESCE(calculated_cost_micro_usd,0)+COALESCE(estimated_cost_micro_usd,0)),0),
                COUNT(CASE WHEN reported_cost_micro_usd IS NOT NULL OR calculated_cost_micro_usd IS NOT NULL OR estimated_cost_micro_usd IS NOT NULL THEN 1 END)
         FROM model_calls WHERE node_id = ?1 AND model_normalized IS NOT NULL GROUP BY model_normalized ORDER BY 2 DESC LIMIT 10",
    ) {
        if let Ok(rows) = stmt.query_map([&id], |r| {
            let cost = r.get::<_, i64>(4)?;
            let priced_calls = r.get::<_, i64>(5)?;
            Ok(serde_json::json!({
                "model": r.get::<_, String>(0)?,
                "calls": r.get::<_, i64>(1)?,
                "input_tokens": r.get::<_, i64>(2)?,
                "output_tokens": r.get::<_, i64>(3)?,
                "cost_micro_usd": available_cost(cost, priced_calls),
            }))
        }) {
            by_model = rows.filter_map(|x| x.ok()).collect();
        }
    }
    let mut by_project = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT COALESCE(project_id,'(none)'), COUNT(*),
                COALESCE(SUM(COALESCE(reported_cost_micro_usd,0)+COALESCE(calculated_cost_micro_usd,0)+COALESCE(estimated_cost_micro_usd,0)),0),
                COUNT(CASE WHEN reported_cost_micro_usd IS NOT NULL OR calculated_cost_micro_usd IS NOT NULL OR estimated_cost_micro_usd IS NOT NULL THEN 1 END)
         FROM sessions WHERE node_id = ?1 AND parent_session_id IS NULL GROUP BY project_id ORDER BY 2 DESC LIMIT 10",
    ) {
        if let Ok(rows) = stmt.query_map([&id], |r| {
            let cost = r.get::<_, i64>(2)?;
            let priced_sessions = r.get::<_, i64>(3)?;
            Ok(serde_json::json!({
                "project_id": r.get::<_, String>(0)?,
                "sessions": r.get::<_, i64>(1)?,
                "cost_micro_usd": available_cost(cost, priced_sessions),
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
    let cte = rollup_source_cte(2, 3, from, to);
    let mut clients = Vec::new();
    if let Ok(mut stmt) = c.prepare(&format!(
        "{cte}SELECT s.client_id, COUNT(DISTINCT s.id) as src_count,
                COALESCE((SELECT SUM(mc.model_call_count) FROM rollup_src mc
                          WHERE mc.node_id = s.node_id AND mc.client_id = s.client_id),0)
         FROM sources s
         WHERE s.node_id = ?1 GROUP BY s.client_id"
    )) {
        if let Ok(rows) = stmt.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        }) {
            for row in rows.flatten() {
                clients.push(serde_json::json!({
                    "client_id": row.0, "source_count": row.1, "model_calls": row.2
                }));
            }
        }
    }

    // S3.4：时间范围统计（token/cost/calls/sessions/cache）
    let range_summary = c
        .query_row(
            &format!(
                "{cte}SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
                    COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                    COALESCE(SUM(reasoning_tokens),0)
             FROM rollup_src WHERE node_id = ?1"
            ),
            params![id, from.to_rfc3339(), to.to_rfc3339()],
            |r| {
                Ok(serde_json::json!({
                    "input_tokens": r.get::<_, i64>(0)?,
                    "output_tokens": r.get::<_, i64>(1)?,
                    "cost_micro_usd": r.get::<_, i64>(2)?,
                    "model_calls": r.get::<_, i64>(3)?,
                    "sessions": r.get::<_, i64>(4)?,
                    "cache_read_tokens": r.get::<_, i64>(5)?,
                    "cache_write_tokens": r.get::<_, i64>(6)?,
                    "reasoning_tokens": r.get::<_, i64>(7)?,
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
        "SELECT client_id, adapter_id, adapter_version, source_path_hash, capabilities, status, client_version, last_scan_at, last_error, last_event_at
         FROM sources WHERE node_id = ?1 ORDER BY client_id",
    ));
    let rows = q!(stmt.query_map([&id], |r| {
        Ok(serde_json::json!({
            "client_id": r.get::<_, String>(0)?,
            "adapter_id": r.get::<_, String>(1)?,
            "adapter_version": r.get::<_, String>(2)?,
            "source_path_hash": r.get::<_, String>(3)?,
            "capabilities": serde_json::from_str::<serde_json::Value>(&r.get::<_, String>(4)?).unwrap_or_else(|_| serde_json::json!([])),
            "status": r.get::<_, String>(5)?,
            "client_version": r.get::<_, Option<String>>(6)?,
            "last_scan_at": r.get::<_, Option<String>>(7)?,
            "last_error": r.get::<_, Option<String>>(8)?,
            "last_event_at": r.get::<_, Option<String>>(9)?,
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
    // 归档水位需要取连接锁读 settings，而 std::sync::Mutex 不可重入，
    // 必须在取得连接锁之前读取，否则与查询共用的锁会把请求永久卡死。
    let archived_before = crate::archive::watermark(&st.db);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(50).min(500);
    let tcol = crate::api::time_column(p.allocation_mode.as_deref());
    let (sql, args): (String, Vec<SqlValue>) = if let Some(cur) = &p.cursor {
        match crate::api::decode_cursor(cur) {
            Some((ts, sid)) => (
                format!(
                    "SELECT id, source_session_id, client_id, title, primary_model_normalized, started_at, ended_at, message_count, model_call_count, input_tokens, output_tokens
                     FROM sessions WHERE node_id = ?1 AND {tcol} >= ?2 AND {tcol} < ?3 AND parent_session_id IS NULL
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
                "SELECT id, source_session_id, client_id, title, primary_model_normalized, started_at, ended_at, message_count, model_call_count, input_tokens, output_tokens
                 FROM sessions WHERE node_id = ?1 AND {tcol} >= ?2 AND {tcol} < ?3 AND parent_session_id IS NULL ORDER BY {tcol} DESC, id DESC LIMIT ?4",
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
        }))
    },));
    let sessions: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    let next_cursor = sessions.last().and_then(|v| {
        let id = v.get("id")?.as_str()?;
        let ts = v.get("started_at")?.as_str()?;
        Some(crate::api::encode_cursor(ts, id))
    });
    Json(serde_json::json!({
        "sessions": sessions,
        "next_cursor": next_cursor,
        "archived_before": archived_before,
    }))
    .into_response()
}

pub(crate) async fn node_calls(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    // 归档水位需要取连接锁读 settings，而 std::sync::Mutex 不可重入，
    // 必须在取得连接锁之前读取，否则与查询共用的锁会把请求永久卡死。
    let archived_before = crate::archive::watermark(&st.db);
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
    // 归档水位：早于该时间的明细已删除，前端据此显示「已归档」而非「无数据」。
    Json(serde_json::json!({
        "calls": calls,
        "next_cursor": next_cursor,
        "archived_before": archived_before,
    }))
    .into_response()
}

pub(crate) async fn list_clients(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let c = st.db.conn();
    let cte = rollup_source_cte(1, 2, from, to);
    let mut stmt = q!(c.prepare(&format!(
        "{cte}SELECT client_id,
            COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
            COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0), COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0)
         FROM rollup_src WHERE 1=1 {filter} GROUP BY client_id ORDER BY 2 DESC"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok(serde_json::json!({
                "client_id": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "model_calls": r.get::<_, i64>(3)?,
                "sessions": r.get::<_, i64>(4)?,
                "cost_micro_usd": r.get::<_, i64>(5)?,
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
    let cte = rollup_source_cte(2, 3, from, to);
    let mut stmt = q!(c.prepare(&format!(
        "{cte}SELECT node_id, COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0),
                COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0), COUNT(CASE WHEN pricing_source != '' THEN 1 END)
         FROM rollup_src WHERE client_id = ?1 GROUP BY node_id ORDER BY 2 DESC"
    )));
    let rows = q!(
        stmt.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
            let cost = r.get::<_, i64>(5)?;
            let priced_rows = r.get::<_, i64>(6)?;
            Ok(serde_json::json!({
                "node_id": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "model_calls": r.get::<_, i64>(3)?,
                "sessions": r.get::<_, i64>(4)?,
                "cost_micro_usd": available_cost(cost, priced_rows),
            }))
        },)
    );
    let by_node: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();

    // 最近 sessions（Agent Tools Detail）
    let recent: Vec<serde_json::Value> = {
        let mut st = q!(c.prepare(
            "SELECT id, source_session_id, node_id, title, primary_model_normalized, started_at,
                    model_call_count, input_tokens, output_tokens
             FROM sessions WHERE client_id = ?1 AND started_at >= ?2 AND started_at < ?3 AND parent_session_id IS NULL
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
                }))
            },)
        );
        r.filter_map(|x| x.ok()).collect()
    };

    // 汇总（三口径 cost / token / 缓存命中率）
    let summary = c
        .query_row(
            &format!(
                "{cte}SELECT COALESCE(SUM(reported_cost),0),
                    COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_cost),0),
                    COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0)
             FROM rollup_src WHERE client_id = ?1"
            ),
            params![id, from.to_rfc3339(), to.to_rfc3339()],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            },
        )
        .unwrap_or((0, 0, 0, 0, 0, 0, 0));
    let (reported_cost, calculated_cost, estimated_cost, in_tok, out_tok, cr_tok, cw_tok) = summary;

    // S3.5：按 Project 分布
    let by_project: Vec<serde_json::Value> = {
        let mut st = q!(c.prepare(
            "SELECT COALESCE(project_id,'(none)'), COUNT(*), COALESCE(SUM(model_call_count),0),
                    COALESCE(SUM(input_tokens),0),
                    COALESCE(SUM(COALESCE(reported_cost_micro_usd,0)+COALESCE(calculated_cost_micro_usd,0)+COALESCE(estimated_cost_micro_usd,0)),0),
                    COUNT(CASE WHEN reported_cost_micro_usd IS NOT NULL OR calculated_cost_micro_usd IS NOT NULL OR estimated_cost_micro_usd IS NOT NULL THEN 1 END)
             FROM sessions WHERE client_id = ?1 AND started_at >= ?2 AND started_at < ?3 AND parent_session_id IS NULL
             GROUP BY project_id ORDER BY 3 DESC LIMIT 10",
        ));
        let r = q!(
            st.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
                let cost = r.get::<_, i64>(4)?;
                let priced_sessions = r.get::<_, i64>(5)?;
                Ok(serde_json::json!({
                    "project_id": r.get::<_, String>(0)?,
                    "sessions": r.get::<_, i64>(1)?,
                    "model_calls": r.get::<_, i64>(2)?,
                    "input_tokens": r.get::<_, i64>(3)?,
                    "cost_micro_usd": available_cost(cost, priced_sessions),
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

    let observation_modes: Vec<&str> = {
        let mut modes = Vec::new();
        if let Ok(mut stmt) = c.prepare("SELECT capabilities FROM sources WHERE client_id = ?1") {
            if let Ok(rows) = stmt.query_map([&id], |r| r.get::<_, String>(0)) {
                for value in rows.flatten() {
                    let runtime = serde_json::from_str::<serde_json::Value>(&value)
                        .ok()
                        .and_then(|v| v.as_array().cloned())
                        .is_some_and(|items| {
                            items
                                .iter()
                                .any(|item| item.as_str() == Some("runtime_observation"))
                        });
                    let mode = if runtime {
                        "native_runtime"
                    } else {
                        "ordinary"
                    };
                    if !modes.contains(&mode) {
                        modes.push(mode);
                    }
                }
            }
        }
        modes
    };

    Json(serde_json::json!({
        "client_id": id,
        "by_node": by_node,
        "by_project": by_project,
        "recent_sessions": recent,
        "recent_calls": recent_calls,
        "source_health": source_health,
        "version_dist": version_dist,
        "observation_modes": observation_modes,
        "reported_cost_micro_usd": reported_cost,
        "calculated_cost_micro_usd": calculated_cost,
        "estimated_cost_micro_usd": estimated_cost,
        "cost_micro_usd": reported_cost + calculated_cost + estimated_cost,
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
            "SELECT model_normalized, provider_normalized, COUNT(*) as cnt, COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(COALESCE(reported_cost_micro_usd,0)+COALESCE(calculated_cost_micro_usd,0)+COALESCE(estimated_cost_micro_usd,0)),0),
                    COUNT(CASE WHEN reported_cost_micro_usd IS NOT NULL OR calculated_cost_micro_usd IS NOT NULL OR estimated_cost_micro_usd IS NOT NULL THEN 1 END)
             FROM model_calls WHERE client_id = ?1 AND model_normalized IS NOT NULL GROUP BY model_normalized, provider_normalized ORDER BY cnt DESC",
        ));
        let rows = q!(stmt.query_map([&id], |r| {
            let cost = r.get::<_, i64>(5)?;
            let priced_calls = r.get::<_, i64>(6)?;
            Ok(serde_json::json!({
                "model": r.get::<_, String>(0)?,
                "provider": r.get::<_, Option<String>>(1)?,
                "calls": r.get::<_, i64>(2)?,
                "input_tokens": r.get::<_, i64>(3)?,
                "output_tokens": r.get::<_, i64>(4)?,
                "cost_micro_usd": available_cost(cost, priced_calls),
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
        let cte = rollup_source_cte(1, 2, from, to);
        let mut stmt = q!(c.prepare(&format!(
            "{cte}SELECT model, MAX(provider),
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0), COUNT(DISTINCT client_id), COUNT(DISTINCT node_id),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_cost),0),
                COALESCE(SUM(reported_cost),0), COALESCE(SUM(reasoning_tokens),0), COALESCE(SUM(cache_write_tokens),0)
             FROM rollup_src WHERE 1=1 AND model != '' {filter} GROUP BY model ORDER BY 6 DESC"
        )));
        let rows = q!(
            stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
                Ok(serde_json::json!({
                    "model": r.get::<_, String>(0)?,
                    "provider": r.get::<_, String>(1)?,
                    "input_tokens": r.get::<_, i64>(2)?,
                    "output_tokens": r.get::<_, i64>(3)?,
                    "model_calls": r.get::<_, i64>(4)?,
                    "sessions": r.get::<_, i64>(5)?,
                    "clients": r.get::<_, i64>(6)?,
                    "nodes": r.get::<_, i64>(7)?,
                    "cache_read_tokens": r.get::<_, i64>(8)?,
                    "calculated_cost_micro_usd": r.get::<_, i64>(9)?,
                    "estimated_cost_micro_usd": r.get::<_, i64>(10)?,
                    "reported_cost_micro_usd": r.get::<_, i64>(11)?,
                    "reasoning_tokens": r.get::<_, i64>(12)?,
                    "cache_write_tokens": r.get::<_, i64>(13)?,
                }))
            },)
        );
        let mut models: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
        // 缓存命中率：cache_read / (input + cache_write + cache_read)
        for m in &mut models {
            let in_t = m.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let cr = m
                .get("cache_read_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let cw = m
                .get("cache_write_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            m["cache_hit_rate"] = match cache_hit_rate(in_t, cw, cr) {
                Some(rate) => serde_json::json!(rate),
                None => serde_json::Value::Null,
            };
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
        let cte = rollup_source_cte(2, 3, from, to);
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

        // 汇总（Token/Cost/Cache Hit）
        let summary = match c.query_row(
            &format!(
                "{cte}SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                    COALESCE(SUM(reasoning_tokens),0),
                    COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
                    COALESCE(SUM(model_call_count),0),
                    COALESCE(SUM(session_count),0)
             FROM rollup_src WHERE model = ?1"
            ),
            params![id, from.to_rfc3339(), to.to_rfc3339()],
            |r| {
                Ok(serde_json::json!({
                    "input_tokens": r.get::<_, i64>(0)?,
                    "output_tokens": r.get::<_, i64>(1)?,
                    "cache_read_tokens": r.get::<_, i64>(2)?,
                    "cache_write_tokens": r.get::<_, i64>(3)?,
                    "reasoning_tokens": r.get::<_, i64>(4)?,
                    "cost_micro_usd": r.get::<_, i64>(5)?,
                    "model_calls": r.get::<_, i64>(6)?,
                    "sessions": r.get::<_, i64>(7)?,
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
                        model_call_count, input_tokens, output_tokens, cache_read_tokens,
                        reasoning_tokens
                 FROM sessions WHERE primary_model_normalized = ?1 AND started_at >= ?2 AND started_at < ?3 AND parent_session_id IS NULL
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
                        "reasoning_tokens": r.get::<_, Option<i64>>(10)?,
                    }))
                },)
            );
            r.filter_map(|x| x.ok()).collect()
        };

        // S3.6：最近 Calls
        let recent_calls: Vec<serde_json::Value> = {
            let mut st = q!(c.prepare(
                "SELECT m.id, m.session_id, m.provider_normalized, m.model_normalized, m.started_at, m.status,
                        m.input_tokens, m.output_tokens, m.calculated_cost_micro_usd
                 FROM model_calls m
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
                    }))
                },)
            );
            r.filter_map(|x| x.ok()).collect()
        };

        // Token/Cost 时间序列（按小时）
        let series: Vec<serde_json::Value> = {
            let mut st = q!(c.prepare(&format!(
                "{cte}SELECT bucket,
                        COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                        COALESCE(SUM(reported_cost+calculated_cost+estimated_cost),0),
                        COALESCE(SUM(model_call_count),0),
                        COALESCE(SUM(reasoning_tokens),0)
                 FROM rollup_src WHERE model = ?1
                 GROUP BY bucket ORDER BY bucket"
            )));
            let r = q!(
                st.query_map(params![id, from.to_rfc3339(), to.to_rfc3339()], |r| {
                    Ok(serde_json::json!({
                        "bucket": r.get::<_, String>(0)?,
                        "input_tokens": r.get::<_, i64>(1)?,
                        "output_tokens": r.get::<_, i64>(2)?,
                        "cost_micro_usd": r.get::<_, i64>(3)?,
                        "model_calls": r.get::<_, i64>(4)?,
                        "reasoning_tokens": r.get::<_, i64>(5)?,
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
    // 归档水位需要取连接锁读 settings，而 std::sync::Mutex 不可重入，
    // 必须在取得连接锁之前读取，否则与查询共用的锁会把请求永久卡死。
    let archived_before = crate::archive::watermark(&st.db);
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
                        reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                        ttft_ms, output_tokens_per_second_milli, first_byte_latency_ms, generation_duration_ms,
                        observability_source, observability_quality, observed_request_payload_bytes,
                        observed_response_payload_bytes
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
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                    ttft_ms, output_tokens_per_second_milli, first_byte_latency_ms, generation_duration_ms,
                    observability_source, observability_quality, observed_request_payload_bytes,
                    observed_response_payload_bytes
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
            "ttft_ms": r.get::<_, Option<i64>>(14)?,
            "output_tokens_per_second_milli": r.get::<_, Option<i64>>(15)?,
            "first_byte_latency_ms": r.get::<_, Option<i64>>(16)?,
            "generation_duration_ms": r.get::<_, Option<i64>>(17)?,
            "observability_source": r.get::<_, Option<String>>(18)?,
            "observability_quality": r.get::<_, Option<String>>(19)?,
            "observed_request_payload_bytes": r.get::<_, Option<i64>>(20)?,
            "observed_response_payload_bytes": r.get::<_, Option<i64>>(21)?,
        }))
    },));
    let calls: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    // 生成下一页游标（取最后一条）
    let next_cursor = calls.last().and_then(|v| {
        let id = v.get("id")?.as_str()?;
        let ts = v.get("started_at")?.as_str()?;
        Some(crate::api::encode_cursor(ts, id))
    });
    // 归档水位：早于该时间的明细已删除，前端据此显示「已归档」而不是「暂无数据」。
    Json(serde_json::json!({
        "calls": calls,
        "next_cursor": next_cursor,
        "archived_before": archived_before,
    }))
    .into_response()
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
                COALESCE(estimated_cost_micro_usd, (SELECT u.estimated_cost_micro_usd FROM usage_events u WHERE u.model_call_id = model_calls.id OR u.event_id = model_calls.usage_event_id LIMIT 1)),
                first_byte_at, first_token_at, last_output_at, first_byte_latency_ms, ttft_ms,
                generation_duration_ms, output_tokens_per_second_milli, inter_token_latency_avg_ms,
                inter_token_latency_p95_ms, stall_count, stall_duration_ms, observability_source,
                observability_quality, endpoint, finish_reason, error_kind, rate_limited,
                observed_request_payload_bytes, observed_response_payload_bytes,
                observed_request_wire_bytes, observed_response_wire_bytes, streaming, stream_completed, retry_count,
                status_code, first_response_at
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
                    "first_byte_at": r.get::<_, Option<String>>(20)?,
                    "first_token_at": r.get::<_, Option<String>>(21)?,
                    "last_output_at": r.get::<_, Option<String>>(22)?,
                    "first_byte_latency_ms": r.get::<_, Option<i64>>(23)?,
                    "ttft_ms": r.get::<_, Option<i64>>(24)?,
                    "generation_duration_ms": r.get::<_, Option<i64>>(25)?,
                    "output_tokens_per_second_milli": r.get::<_, Option<i64>>(26)?,
                    "inter_token_latency_avg_ms": r.get::<_, Option<i64>>(27)?,
                    "inter_token_latency_p95_ms": r.get::<_, Option<i64>>(28)?,
                    "stall_count": r.get::<_, Option<i64>>(29)?,
                    "stall_duration_ms": r.get::<_, Option<i64>>(30)?,
                    "observability_source": r.get::<_, Option<String>>(31)?,
                    "observability_quality": r.get::<_, Option<String>>(32)?,
                    "endpoint": r.get::<_, Option<String>>(33)?,
                    "finish_reason": r.get::<_, Option<String>>(34)?,
                    "error_kind": r.get::<_, Option<String>>(35)?,
                    "rate_limited": r.get::<_, Option<bool>>(36)?,
                    "observed_request_payload_bytes": r.get::<_, Option<i64>>(37)?,
                    "observed_response_payload_bytes": r.get::<_, Option<i64>>(38)?,
                    "observed_request_wire_bytes": r.get::<_, Option<i64>>(39)?,
                    "observed_response_wire_bytes": r.get::<_, Option<i64>>(40)?,
                    "streaming": r.get::<_, bool>(41)?,
                    "stream_completed": r.get::<_, Option<bool>>(42)?,
                    "retry_count": r.get::<_, i64>(43)?,
                    "status_code": r.get::<_, Option<i64>>(44)?,
                    "first_response_at": r.get::<_, Option<String>>(45)?,
                }))
            },
        )
        .ok()
        .unwrap_or(serde_json::json!({}));
    Json(serde_json::json!({ "call": call })).into_response()
}

pub(crate) async fn list_sessions(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    // 归档水位需要取连接锁读 settings，而 std::sync::Mutex 不可重入，
    // 必须在取得连接锁之前读取，否则与查询共用的锁会把请求永久卡死。
    let archived_before = crate::archive::watermark(&st.db);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(100).min(1000);
    let tcol = crate::api::time_column(p.allocation_mode.as_deref());
    let (session_filter, session_fargs) = range_filter_sessions(&p);
    // 会话在所选时间范围内仍活跃（last_activity_at 落在范围内）也展示，
    // 而非仅按 started_at 过滤，避免"还在用"的会话从列表消失。
    let range_overlap = format!(
        "((s.{tcol} >= ?1 AND s.{tcol} < ?2) OR (s.last_activity_at >= ?1 AND s.last_activity_at < ?2))"
    );
    // 默认只返回主 Agent 会话；显式 include_subagents=true 时返回全部。
    let subagent_filter = if p.include_subagents == Some(true) {
        ""
    } else {
        " AND s.parent_session_id IS NULL"
    };
    let (sql, args): (String, Vec<SqlValue>) = if let Some(cur) = &p.cursor {
        match crate::api::decode_cursor(cur) {
            Some((ts, id)) => {
                let cursor_index = 3 + session_fargs.len();
                let limit_index = cursor_index + 2;
                let mut args = range_args(&from, &to, session_fargs.clone());
                args.extend([
                    SqlValue::Text(ts),
                    SqlValue::Text(id),
                    SqlValue::Integer(limit),
                ]);
                (
                    format!(
                    "SELECT s.id, s.source_session_id, s.client_id, s.node_id, s.title, s.provider_normalized, s.primary_model_normalized, s.started_at, s.ended_at, s.last_activity_at,
                        message_count, tool_call_count, model_call_count, input_tokens, output_tokens, cache_read_tokens,
                        reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                        status,
                        CASE WHEN COALESCE(last_activity_at, ended_at) IS NOT NULL AND started_at IS NOT NULL
                             THEN CAST((julianday(COALESCE(last_activity_at, ended_at)) - julianday(started_at)) * 86400000 AS INTEGER) END AS duration_ms,
                        reasoning_tokens
                     FROM sessions s WHERE {range_overlap} {session_filter} {subagent_filter}
                       AND (s.{tcol} < ?{cursor_index} OR (s.{tcol} = ?{cursor_index} AND s.id < ?{next_cursor_index}))
                     ORDER BY s.{tcol} DESC, s.id DESC LIMIT ?{limit_index}",
                    next_cursor_index = cursor_index + 1,
                ),
                    args,
                )
            }
            None => return json_err(StatusCode::BAD_REQUEST, "invalid_cursor", "分页游标无效"),
        }
    } else {
        let limit_index = 3 + session_fargs.len();
        let mut args = range_args(&from, &to, session_fargs);
        args.push(SqlValue::Integer(limit));
        (
            format!(
                "SELECT s.id, s.source_session_id, s.client_id, s.node_id, s.title, s.provider_normalized, s.primary_model_normalized, s.started_at, s.ended_at, s.last_activity_at,
                    message_count, tool_call_count, model_call_count, input_tokens, output_tokens, cache_read_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                    status,
                    CASE WHEN COALESCE(last_activity_at, ended_at) IS NOT NULL AND started_at IS NOT NULL
                         THEN CAST((julianday(COALESCE(last_activity_at, ended_at)) - julianday(started_at)) * 86400000 AS INTEGER) END AS duration_ms,
                    reasoning_tokens
                 FROM sessions s WHERE {range_overlap} {session_filter} {subagent_filter}
                 ORDER BY s.{tcol} DESC, s.id DESC LIMIT ?{limit_index}"
            ),
            args,
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
            "status": r.get::<_, String>(19)?,
            "duration_ms": r.get::<_, Option<i64>>(20)?,
            "reasoning_tokens": r.get::<_, Option<i64>>(21)?,
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
    Json(serde_json::json!({
        "sessions": sessions,
        "next_cursor": next_cursor,
        "archived_before": archived_before,
    }))
    .into_response()
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
                COALESCE((SELECT p.id FROM sessions p
                          WHERE p.id = sessions.parent_session_id
                             OR p.source_session_id = sessions.parent_session_id),
                         sessions.parent_session_id)
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
                    "parent_session_id": r.get::<_, Option<String>>(24)?,
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
                m.duration_ms, m.ttft_ms, m.output_tokens_per_second_milli,
                m.observability_source, m.observability_quality,
                m.observed_request_payload_bytes, m.observed_response_payload_bytes,
                m.observed_request_wire_bytes, m.observed_response_wire_bytes,
                m.first_byte_latency_ms, m.generation_duration_ms,
                m.inter_token_latency_avg_ms, m.inter_token_latency_p95_ms,
                m.stall_count, m.stall_duration_ms, m.status_code, m.endpoint,
                m.finish_reason, m.error_kind, m.rate_limited, m.retry_count
         FROM model_calls m WHERE m.session_id = ?1 ORDER BY m.started_at",
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
            "duration_ms": r.get::<_, Option<i64>>(12)?,
            "ttft_ms": r.get::<_, Option<i64>>(13)?,
            "output_tokens_per_second_milli": r.get::<_, Option<i64>>(14)?,
            "observability_source": r.get::<_, Option<String>>(15)?,
            "observability_quality": r.get::<_, Option<String>>(16)?,
            "observed_request_payload_bytes": r.get::<_, Option<i64>>(17)?,
            "observed_response_payload_bytes": r.get::<_, Option<i64>>(18)?,
            "observed_request_wire_bytes": r.get::<_, Option<i64>>(19)?,
            "observed_response_wire_bytes": r.get::<_, Option<i64>>(20)?,
            "first_byte_latency_ms": r.get::<_, Option<i64>>(21)?,
            "generation_duration_ms": r.get::<_, Option<i64>>(22)?,
            "inter_token_latency_avg_ms": r.get::<_, Option<i64>>(23)?,
            "inter_token_latency_p95_ms": r.get::<_, Option<i64>>(24)?,
            "stall_count": r.get::<_, Option<i64>>(25)?,
            "stall_duration_ms": r.get::<_, Option<i64>>(26)?,
            "status_code": r.get::<_, Option<i64>>(27)?,
            "endpoint": r.get::<_, Option<String>>(28)?,
            "finish_reason": r.get::<_, Option<String>>(29)?,
            "error_kind": r.get::<_, Option<String>>(30)?,
            "rate_limited": r.get::<_, Option<bool>>(31)?,
            "retry_count": r.get::<_, Option<i64>>(32)?,
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
    // 同一子会话只保留最早一条关系（历史 Agent 曾按扫描轮次重复写入）
    let mut stmt = q!(c.prepare(
        "SELECT MIN(id) AS id, child_session_id, MIN(relation) AS relation, MIN(created_at) AS created_at
         FROM subagent_relations WHERE session_id = ?1
         GROUP BY child_session_id ORDER BY MIN(created_at)",
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
    let (children, totals): (Vec<serde_json::Value>, serde_json::Value) = if child_ids.is_empty() {
        (Vec::new(), serde_json::Value::Null)
    } else {
        let placeholders = child_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, source_session_id, title, primary_model_normalized, message_count, tool_call_count, model_call_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                    started_at, last_activity_at, status,
                    CASE WHEN COALESCE(last_activity_at, ended_at) IS NOT NULL AND started_at IS NOT NULL
                         THEN CAST((julianday(COALESCE(last_activity_at, ended_at)) - julianday(started_at)) * 86400000 AS INTEGER) END AS duration_ms
             FROM sessions WHERE id IN ({placeholders}) OR source_session_id IN ({placeholders})
             ORDER BY started_at, id"
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
                    "tool_call_count": r.get::<_, i64>(5)?,
                    "model_call_count": r.get::<_, i64>(6)?,
                    "input_tokens": r.get::<_, Option<i64>>(7)?,
                    "output_tokens": r.get::<_, Option<i64>>(8)?,
                    "cache_read_tokens": r.get::<_, Option<i64>>(9)?,
                    "cache_write_tokens": r.get::<_, Option<i64>>(10)?,
                    "reasoning_tokens": r.get::<_, Option<i64>>(11)?,
                    "reported_cost_micro_usd": r.get::<_, Option<i64>>(12)?,
                    "calculated_cost_micro_usd": r.get::<_, Option<i64>>(13)?,
                    "estimated_cost_micro_usd": r.get::<_, Option<i64>>(14)?,
                    "started_at": r.get::<_, String>(15)?,
                    "last_activity_at": r.get::<_, Option<String>>(16)?,
                    "status": r.get::<_, String>(17)?,
                    "duration_ms": r.get::<_, Option<i64>>(18)?,
                }))
            })
        );
        let children: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
        // 合计：Token 直接求和；费用沿用列表口径（calculated 优先、回退 estimated），
        // 任一子会话有可用费用才给出合计，否则为不可用。
        let sum_opt = |key: &str| -> i64 {
            children
                .iter()
                .map(|c| c.get(key).and_then(|v| v.as_i64()).unwrap_or(0))
                .sum()
        };
        let mut cost_total = 0i64;
        let mut cost_available = false;
        let mut reported_total = 0i64;
        let mut calculated_total = 0i64;
        let mut estimated_total = 0i64;
        for child in &children {
            let reported = child
                .get("reported_cost_micro_usd")
                .and_then(|v| v.as_i64());
            let calculated = child
                .get("calculated_cost_micro_usd")
                .and_then(|v| v.as_i64());
            let estimated = child
                .get("estimated_cost_micro_usd")
                .and_then(|v| v.as_i64());
            reported_total += reported.unwrap_or(0);
            calculated_total += calculated.unwrap_or(0);
            estimated_total += estimated.unwrap_or(0);
            if let Some(cost) = calculated.or(estimated) {
                cost_total += cost;
                cost_available = true;
            }
        }
        let totals = serde_json::json!({
            "children": children.len(),
            "model_calls": sum_opt("model_call_count"),
            "messages": sum_opt("message_count"),
            "input_tokens": sum_opt("input_tokens"),
            "output_tokens": sum_opt("output_tokens"),
            "cache_read_tokens": sum_opt("cache_read_tokens"),
            "cache_write_tokens": sum_opt("cache_write_tokens"),
            "reasoning_tokens": sum_opt("reasoning_tokens"),
            "cost_micro_usd": if cost_available { serde_json::json!(cost_total) } else { serde_json::Value::Null },
            "reported_cost_micro_usd": if reported_total > 0 { serde_json::json!(reported_total) } else { serde_json::Value::Null },
            "calculated_cost_micro_usd": if calculated_total > 0 { serde_json::json!(calculated_total) } else { serde_json::Value::Null },
            "estimated_cost_micro_usd": if estimated_total > 0 { serde_json::json!(estimated_total) } else { serde_json::Value::Null },
        });
        (children, totals)
    };

    Json(serde_json::json!({ "relations": rels, "children": children, "totals": totals }))
        .into_response()
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

/// 调用时长的口径异常检测（防线，不是修复手段）。
///
/// 阈值由生产分布支撑：p50≈37.6s、p99≈6.9h，因此 **6h 只作「占总时长比例」的口径
/// 统计**（6h 会命中 1.3% 的行，作告警线会天天全红），24h 与首响应不自洽才作 error。
///
/// 已知根因（只读排查结论）：opencode adapter 把 turn 起点当作调用起点，同一 turn 内
/// 多条调用共享同一 `started_at`，求和时被重复累加；`call_granularity` 标为 message
/// 而时间跨度是 turn 级。这里只做可见化，不改历史数值。
fn duration_outliers(
    c: &metria_storage::rusqlite::Connection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    p: &RangeParams,
) -> serde_json::Value {
    let (filter, fargs) = range_filter_usage(p);
    let range = |sql: &str, fargs: Vec<SqlValue>| -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        if let Ok(mut stmt) = c.prepare(sql) {
            if let Ok(rows) = stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
                Ok(serde_json::json!({
                    "client_id": r.get::<_, String>(0)?,
                    "count": r.get::<_, i64>(1)?,
                    "call_granularity": r.get::<_, Option<String>>(2)?,
                    "timing_source": r.get::<_, Option<String>>(3)?,
                }))
            }) {
                out = rows.filter_map(|x| x.ok()).collect();
            }
        }
        out
    };

    // 范围内总时长与「>6h」贡献的时长（口径统计用）
    let (total_ms, over6h_ms) = c
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(duration_ms), 0),
                        COALESCE(SUM(CASE WHEN duration_ms > 21600000 THEN duration_ms ELSE 0 END), 0)
                 FROM model_calls
                 WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms IS NOT NULL {filter}"
            ),
            params_from_iter(range_args(&from, &to, fargs.clone())),
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
        .unwrap_or((0, 0));

    let over_24h = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM model_calls
                 WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms > 86400000 {filter}"
            ),
            params_from_iter(range_args(&from, &to, fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);

    // 首响应距起点 >1h，或干脆晚于该调用自身的时长（语义不可能，实测命中 TTFT=24.2h）
    let ttft_invalid = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM model_calls
                 WHERE started_at >= ?1 AND started_at < ?2 {filter}
                   AND first_response_at IS NOT NULL
                   AND ((julianday(first_response_at) - julianday(started_at)) * 86400000.0 > 3600000.0
                        OR (julianday(first_response_at) - julianday(started_at)) * 86400000.0 > duration_ms)"
            ),
            params_from_iter(range_args(&from, &to, fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);

    // 同一 (session_id, started_at) 分组：多条调用共享起点且累计时长 > 组内墙钟 5 倍
    let duplicate_start_groups = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM (
                    SELECT session_id, started_at,
                           SUM(COALESCE(duration_ms, 0)) AS s,
                           (julianday(MAX(COALESCE(completed_at, started_at)))
                            - julianday(MIN(started_at))) * 86400000.0 AS span
                     FROM model_calls
                     WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms IS NOT NULL {filter}
                     GROUP BY session_id, started_at
                     HAVING COUNT(*) > 1 AND span > 0 AND s > 5 * span
                 )"
            ),
            params_from_iter(range_args(&from, &to, fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);

    let by_client = range(
        &format!(
            "SELECT client_id, COUNT(*), call_granularity, timing_source
             FROM model_calls
             WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms > 21600000 {filter}
             GROUP BY client_id, call_granularity, timing_source
             ORDER BY COUNT(*) DESC LIMIT 20"
        ),
        fargs.clone(),
    );

    serde_json::json!({
        "label": "疑似采集口径异常（turn 起点被用作调用起点 / 同回合重复累计），不等同于真实模型响应时长",
        "share_threshold_hours": 6,
        "total_duration_hours": total_ms as f64 / 3_600_000.0,
        "over_threshold_duration_hours": over6h_ms as f64 / 3_600_000.0,
        "share_of_total_duration": if total_ms > 0 { over6h_ms as f64 / total_ms as f64 } else { 0.0 },
        "checks": [
            serde_json::json!({
                "key": "over_24h", "severity": "error",
                "threshold": "单条时长 > 24 小时", "count": over_24h,
            }),
            serde_json::json!({
                "key": "ttft_invalid", "severity": "error",
                "threshold": "首响应 > 1 小时或超过该调用时长", "count": ttft_invalid,
            }),
            serde_json::json!({
                "key": "duplicate_start", "severity": "warning",
                "threshold": "同一会话内共享起始时刻的分组，累计时长 > 组内墙钟 5 倍",
                "count": duplicate_start_groups,
            }),
        ],
        "by_client": by_client,
    })
}

pub(crate) async fn data_quality(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();

    // 用量来源分布：仅统计 Usage 行（Call/Session 行不带 Token 与 usage_source），
    // 按来源汇总 Token 构成，避免与调用计数混在一张表里产生镜像行。
    let mut usage_dist = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT usage_source,
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(reasoning_tokens),0)
         FROM hourly_rollups
         WHERE julianday(bucket) >= julianday(?1) AND julianday(bucket) < julianday(?2)
           AND usage_source != ''
         GROUP BY usage_source
         ORDER BY 2 DESC, 1",
    ) {
        if let Ok(rows) = stmt.query_map(params![from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        }) {
            for row in rows.flatten() {
                usage_dist.push(serde_json::json!({
                    "usage_source": row.0,
                    "tokens": row.1,
                    "output_tokens": row.2,
                    "cache_read_tokens": row.3,
                    "cache_write_tokens": row.4,
                    "reasoning_tokens": row.5,
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

    // S3.8：Source cursor 状态（来自 sources 表：scan/cursor 健康 + 最近数据新鲜度）
    let cursor_status: Vec<serde_json::Value> = {
        let st = c.prepare(
            "SELECT id, client_id, adapter_id, status, last_scan_at, last_error, last_event_at
             FROM sources
             ORDER BY COALESCE(last_event_at,'') DESC, last_scan_at DESC
             LIMIT 100",
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
                    "last_event_at": r.get::<_, Option<String>>(6)?,
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

    // 时长口径异常检测（防线，不是修复）：见 duration_outliers 的阈值依据说明。
    let duration_outliers = duration_outliers(&c, from, to, &p);

    Json(serde_json::json!({
        "usage_distribution": usage_dist,
        "parse_warnings": parse_warnings,
        "source_errors": source_errors,
        "source_scan": source_scan,
        "clock_skew_warnings": clock_skew_warns,
        "cursor_status": cursor_status,
        "alerts": alerts,
        "duration_outliers": duration_outliers,
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
    // 快速路径读小时级预聚合；带维度筛选或预聚合缺桶时回退明细。
    // 两条路径共用 perfrollup 的累加逻辑，样本数、平均值精确，分位数为直方图近似。
    let duration = match crate::perfrollup::fast_aggregate(&st.db, from, to, &mc_filter) {
        Some(agg) => agg.duration,
        None => {
            let agg = q!(crate::perfrollup::scan_calls(
                &st.db, from, to, &mc_filter, &mc_fargs
            ));
            agg.duration
        }
    };
    Json(serde_json::json!({
        "count": duration.count,
        "p50_ms": duration.quantile(0.50),
        "p95_ms": duration.quantile(0.95),
        "p99_ms": duration.quantile(0.99),
        "avg_ms": duration.avg(),
        // 分位数由对数分箱直方图合并得出；count 与 avg_ms 为精确统计
        "quantile_approx": true,
    }))
    .into_response()
}

/// Runtime/日志性能指标；缺失字段保持 null，并按来源分别统计覆盖率。
pub(crate) async fn usage_performance(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter_usage(&p);
    // 快速路径读小时级预聚合（完整小时）；带维度筛选或预聚合缺桶时回退明细。
    // 两条路径共用 perfrollup 的逐行累加逻辑，保证口径一致。
    let acc = match crate::perfrollup::fast_aggregate(&st.db, from, to, &filter) {
        Some(agg) => agg,
        None => q!(crate::perfrollup::scan_calls(
            &st.db, from, to, &filter, &fargs
        )),
    };
    Json(performance_json(&acc)).into_response()
}

/// 把聚合结果渲染成 `/usage/performance` 响应。
///
/// `count` / `avg_*` / `coverage` / 可靠性来自精确累加；P50/P95/P99 由对数分箱直方图
/// 合并得出，响应以 `quantile_approx` 标注，界面据此显示「近似」而不是当成精确值。
fn performance_json(acc: &crate::perfrollup::PerfHour) -> serde_json::Value {
    let total = acc.call_count;
    let coverage = |count: u64| (total > 0).then(|| count as f64 / total as f64);
    let observed = |index: usize| -> (serde_json::Value, u64) {
        match acc.observed_bytes.get(index) {
            Some((bytes, count)) if *count > 0 => (serde_json::json!(bytes), *count),
            _ => (serde_json::Value::Null, 0),
        }
    };
    let (req_payload, req_payload_count) = observed(0);
    let (resp_payload, resp_payload_count) = observed(1);
    let (req_wire, req_wire_count) = observed(2);
    let (resp_wire, resp_wire_count) = observed(3);
    let rows = |map: &crate::perfrollup::SourceCounts| map.clone().into_sorted_rows();

    serde_json::json!({
        "total_calls": total,
        "reliability": {
            "success_count": acc.success,
            "error_count": acc.errors,
            "success_rate": (total > 0).then(|| acc.success as f64 / total as f64),
            "error_rate": (total > 0).then(|| acc.errors as f64 / total as f64),
        },
        "first_byte": {
            "count": acc.first_byte.count,
            "avg_ms": acc.first_byte.avg(),
            "p50_ms": acc.first_byte.quantile(0.50),
            "p95_ms": acc.first_byte.quantile(0.95),
            "p99_ms": acc.first_byte.quantile(0.99),
            "coverage": coverage(acc.first_byte.count),
            "sources": rows(&acc.first_byte_sources),
        },
        "ttft": {
            "count": acc.ttft.count,
            "avg_ms": acc.ttft.avg(),
            "p50_ms": acc.ttft.quantile(0.50),
            "p95_ms": acc.ttft.quantile(0.95),
            "p99_ms": acc.ttft.quantile(0.99),
            "coverage": coverage(acc.ttft.count),
            "sources": rows(&acc.ttft_sources),
        },
        "generation": {
            "count": acc.generation.count,
            "avg_ms": acc.generation.avg(),
            "p50_ms": acc.generation.quantile(0.50),
            "p95_ms": acc.generation.quantile(0.95),
            "p99_ms": acc.generation.quantile(0.99),
            "coverage": coverage(acc.generation.count),
            "sources": rows(&acc.generation_sources),
        },
        "output_speed": {
            "count": acc.speed.count,
            "avg_tokens_per_second": acc.speed.avg(),
            "p50_tokens_per_second": acc.speed.quantile(0.50),
            "p95_tokens_per_second": acc.speed.quantile(0.95),
            "coverage": coverage(acc.speed.count),
            "sources": rows(&acc.speed_sources),
        },
        "inter_token_latency": {
            "count": acc.inter_token.count,
            "avg_ms": acc.inter_token.avg(),
            "p50_ms": acc.inter_token.quantile(0.50),
            "p95_ms": acc.inter_token.quantile(0.95),
            "p99_ms": acc.inter_token.quantile(0.99),
            "coverage": coverage(acc.inter_token.count),
            "sources": rows(&acc.inter_token_sources),
        },
        "stalls": {
            "count": acc.stalls.count,
            "avg_count": acc.stalls.avg(),
            "p95_count": acc.stalls.quantile(0.95),
        },
        "observed_bytes": {
            "request_payload": { "bytes": req_payload, "count": req_payload_count },
            "response_payload": { "bytes": resp_payload, "count": resp_payload_count },
            "request_wire": { "bytes": req_wire, "count": req_wire_count },
            "response_wire": { "bytes": resp_wire, "count": resp_wire_count },
        },
        "sources": rows(&acc.sources),
        // 分位数为直方图近似值；样本数、平均值与覆盖率为精确统计
        "quantile_approx": true,
    })
}

/// 按时间桶返回运行时性能指标；空桶使用 null，避免把没有观测误报为 0。
pub(crate) async fn usage_performance_timeseries(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let (from, to) = parse_range(&p);
    let bucket_secs = bucket_granularity(&p, from, to);
    let (filter, fargs) = range_filter_usage(&p);
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(&format!(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S+00:00',
                    datetime((CAST(strftime('%s', started_at) AS INTEGER) / {bucket_secs}) * {bucket_secs}, 'unixepoch')) AS b,
                ttft_ms, first_byte_latency_ms, generation_duration_ms,
                output_tokens_per_second_milli, inter_token_latency_avg_ms
         FROM model_calls
         WHERE started_at >= ?1 AND started_at < ?2 {filter}"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })
    );
    let mut buckets: std::collections::HashMap<String, Vec<[Option<i64>; 5]>> = Default::default();
    for row in rows.flatten() {
        buckets
            .entry(row.0)
            .or_default()
            .push([row.1, row.2, row.3, row.4, row.5]);
    }
    let mut points = Vec::new();
    for (bucket, values) in buckets {
        let avg = |index: usize| -> Option<i64> {
            let values: Vec<i64> = values.iter().filter_map(|row| row[index]).collect();
            if values.is_empty() {
                None
            } else {
                Some(values.iter().sum::<i64>() / values.len() as i64)
            }
        };
        points.push(serde_json::json!({
            "bucket": bucket,
            "count": values.len(),
            "ttft_avg_ms": avg(0),
            "first_byte_avg_ms": avg(1),
            "generation_avg_ms": avg(2),
            "output_tokens_per_second_milli_avg": avg(3),
            "inter_token_latency_avg_ms": avg(4),
        }));
    }
    points.sort_by(|a, b| a["bucket"].as_str().cmp(&b["bucket"].as_str()));
    Json(serde_json::json!({
        "series": fill_performance_timeseries(points, from, to, bucket_secs)
    }))
    .into_response()
}

fn fill_performance_timeseries(
    points: Vec<serde_json::Value>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    bucket_secs: i64,
) -> Vec<serde_json::Value> {
    let mut by_bucket = std::collections::HashMap::new();
    for point in points {
        if let Some(bucket) = point.get("bucket").and_then(|v| v.as_str()) {
            by_bucket.insert(bucket.to_string(), point);
        }
    }
    let mut current = floor_ts(from, bucket_secs);
    let end = floor_ts(to, bucket_secs);
    let mut output = Vec::new();
    while current <= end {
        let bucket = if bucket_secs >= 86_400 {
            current.format("%Y-%m-%d").to_string()
        } else {
            current.to_rfc3339()
        };
        output.push(by_bucket.remove(&bucket).unwrap_or_else(|| {
            serde_json::json!({
                "bucket": bucket,
                "count": 0,
                "ttft_avg_ms": serde_json::Value::Null,
                "first_byte_avg_ms": serde_json::Value::Null,
                "generation_avg_ms": serde_json::Value::Null,
                "output_tokens_per_second_milli_avg": serde_json::Value::Null,
                "inter_token_latency_avg_ms": serde_json::Value::Null,
            })
        }));
        current += chrono::Duration::seconds(bucket_secs);
    }
    output
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
    // 该接口仍在明细上按桶精确计算分位数，明确标注为非近似，避免前端误标「近似」。
    Json(serde_json::json!({ "series": filled, "quantile_approx": false })).into_response()
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

/// 完整小时边界：from 向上取整、to 向下取整（UTC 整点）。
/// 按窗口裁剪后的调用时长合计与计数：每条调用只累计落在 `[from, to)` 内的那部分时长，
/// 与「会话总时长」按边界裁剪的口径一致（design D1）。
///
/// 只在 `to` 早于当前时刻时才需要调用——否则没有调用能跨越窗口终点。
/// SQLite 标量 `MIN(a, NULL)` 返回 NULL：无法解析 `started_at` 的行在 `SUM` 与
/// `COUNT(expr)` 中同时被跳过，合计与计数因此保持一致。
/// `julianday` 的亚毫秒精度会被截断（约 1ms），仅影响跨越终点的少数调用，可接受。
fn clipped_duration_stats(
    c: &metria_storage::rusqlite::Connection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    filter: &str,
    fargs: &[SqlValue],
) -> Result<Option<(i64, i64)>, metria_storage::rusqlite::Error> {
    let sql = format!(
        "SELECT COALESCE(SUM(CAST(MIN(duration_ms, ROUND((julianday(?2) - julianday(started_at)) * 86400000.0)) AS INTEGER)), 0),
                COUNT(CAST(MIN(duration_ms, ROUND((julianday(?2) - julianday(started_at)) * 86400000.0)) AS INTEGER))
         FROM model_calls
         WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms IS NOT NULL {filter}"
    );
    let (sum, count) = c.query_row(
        &sql,
        params_from_iter(range_args(&from, &to, fargs.to_vec())),
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    )?;
    Ok((count > 0).then_some((sum, count)))
}

fn full_hour_bounds(from: DateTime<Utc>, to: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let ceil = |ts: DateTime<Utc>| {
        let secs = ts.timestamp();
        let rem = secs.rem_euclid(3600);
        let aligned = if rem == 0 { secs } else { secs - rem + 3600 };
        Utc.timestamp_opt(aligned, 0).single().unwrap_or(ts)
    };
    let floor = |ts: DateTime<Utc>| {
        let secs = ts.timestamp();
        Utc.timestamp_opt(secs - secs.rem_euclid(3600), 0)
            .single()
            .unwrap_or(ts)
    };
    (ceil(from), floor(to))
}

/// rollup 无法覆盖的两端不完整小时窗口（最多两段；不含完整小时区间）。
fn edge_windows(
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    h_from: DateTime<Utc>,
    h_to: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    if h_from >= h_to {
        return if from < to {
            vec![(from, to)]
        } else {
            Vec::new()
        };
    }
    let mut windows = Vec::new();
    if from < h_from {
        windows.push((from, h_from));
    }
    if h_to < to {
        windows.push((h_to, to));
    }
    windows
}

/// 用原始明细补齐总览在两端不完整小时的聚合（token/费用/调用数/会话/消息）。
#[allow(clippy::too_many_arguments)]
fn add_overview_raw_edges(
    body: &mut serde_json::Value,
    c: &metria_storage::rusqlite::Connection,
    _p: &RangeParams,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    h_from: DateTime<Utc>,
    h_to: DateTime<Utc>,
    call_filter: &str,
    call_fargs: &[SqlValue],
) {
    for (w_from, w_to) in edge_windows(from, to, h_from, h_to) {
        if let Ok(values) = c.query_row(
            &format!(
                "SELECT
                    COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                    COALESCE(SUM(reasoning_tokens),0),
                    COALESCE(SUM(reported_cost_micro_usd),0), COALESCE(SUM(calculated_cost_micro_usd),0), COALESCE(SUM(estimated_cost_micro_usd),0),
                    COUNT(*)
                 FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 {call_filter}"
            ),
            params_from_iter(range_args(&w_from, &w_to, call_fargs.to_vec())),
            |r| {
                let mut out = Vec::with_capacity(9);
                for i in 0..9 {
                    out.push(r.get::<_, i64>(i).unwrap_or(0));
                }
                Ok(out)
            },
        ) {
            const CALL_KEYS: [&str; 9] = [
                "input_tokens",
                "output_tokens",
                "cache_read_tokens",
                "cache_write_tokens",
                "reasoning_tokens",
                "reported_cost_micro_usd",
                "calculated_cost_micro_usd",
                "estimated_cost_micro_usd",
                "model_calls",
            ];
            for (key, value) in CALL_KEYS.iter().zip(values.iter()) {
                add_json_number(body, key, *value);
            }
        }
    }
}

/// 概览「活动与健康 / 消息与数据状态」的窗口口径统计。
struct OverviewActivity {
    /// `None` 表示所需明细已被归档、该指标不可算——诚实返回空值而不是偏小的假数字。
    sessions: Option<i64>,
    active_sessions: Option<i64>,
    messages: Option<i64>,
    user_messages: Option<i64>,
    tools: Option<i64>,
    session_duration_ms: Option<i64>,
}

/// 新建会话按窗口内开始计；会话时长按与窗口重叠的跨度裁剪；
/// 消息与工具按明细自身的发生时间统计（开启内容采集后才会有数据）。
fn overview_activity(
    c: &metria_storage::rusqlite::Connection,
    p: &RangeParams,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    archived: bool,
) -> OverviewActivity {
    let (session_filter, session_fargs) = range_filter_sessions(p);

    // 请求范围早于归档水位：这些指标全部依赖已删除的明细行。
    // - 会话数在「整点对齐 + 无会话筛选」时可由聚合表精确给出（聚合完整保留，
    //   与归档前完全一致，正是默认时间范围的形态）；
    // - 其余指标（消息按 created_at、工具按 started_at、活跃会话与会话时长按会话跨度）
    //   没有对应的聚合口径，一律返回 None → 响应里是 null，配合
    //   activity_archived_before 由界面标注「已归档，不可计算」，
    //   绝不返回偏小的数值或 0。
    if archived {
        let sessions = if session_filter.trim().is_empty()
            && from.timestamp() % 3600 == 0
            && to.timestamp() % 3600 == 0
        {
            c.query_row(
                "SELECT COALESCE(SUM(session_count), 0) FROM hourly_rollups
                 WHERE bucket >= ?1 AND bucket < ?2",
                [from.to_rfc3339(), to.to_rfc3339()],
                |r| r.get::<_, i64>(0),
            )
            .ok()
        } else {
            None
        };
        return OverviewActivity {
            sessions,
            active_sessions: None,
            messages: None,
            user_messages: None,
            tools: None,
            session_duration_ms: None,
        };
    }
    // 没有会话筛选时 JOIN 是多余的：messages/tool_events 按时间列即可命中索引
    // （实测带 JOIN 的三次计数合计 47ms，去掉后只剩索引扫描）。
    let has_session_scope = !session_filter.trim().is_empty();
    let messages_join = if has_session_scope {
        "\n                 JOIN sessions s ON s.id = m.session_id"
    } else {
        ""
    };
    let tools_join = if has_session_scope {
        "\n                 JOIN sessions s ON s.id = t.session_id"
    } else {
        ""
    };
    let sessions_raw = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM sessions s
                 WHERE s.started_at >= ?1 AND s.started_at < ?2 {session_filter}
                   AND s.parent_session_id IS NULL"
            ),
            params_from_iter(range_args(&from, &to, session_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let sessions_value = Some(sessions_raw);
    // 活跃会话：窗口内新建，或窗口之前开始但在窗口内仍有活动。
    let active_sessions_raw = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM sessions s
                 WHERE ((s.started_at >= ?1 AND s.started_at < ?2)
                    OR (s.started_at < ?2
                        AND COALESCE(s.last_activity_at, s.ended_at, s.started_at) >= ?1)) {session_filter}
                   AND s.parent_session_id IS NULL"
            ),
            params_from_iter(range_args(&from, &to, session_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let active_sessions_value = Some(active_sessions_raw);
    let messages_raw = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM messages m{messages_join}
                 WHERE m.created_at >= ?1 AND m.created_at < ?2 {session_filter}"
            ),
            params_from_iter(range_args(&from, &to, session_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let messages_value = Some(messages_raw);
    let user_messages_raw = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM messages m{messages_join}
                 WHERE m.created_at >= ?1 AND m.created_at < ?2
                   AND m.role = 'user' {session_filter}"
            ),
            params_from_iter(range_args(&from, &to, session_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let user_messages_value = Some(user_messages_raw);
    let tools_raw = c
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM tool_events t{tools_join}
                 WHERE t.started_at >= ?1 AND t.started_at < ?2 {session_filter}"
            ),
            params_from_iter(range_args(&from, &to, session_fargs.clone())),
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let tools_value = Some(tools_raw);
    // 与窗口重叠的会话跨度（裁剪到窗口边界），允许会话之间重叠。
    let (overlap_count, overlap_ms) = c
        .query_row(
            &format!(
                "SELECT COUNT(*), COALESCE(SUM(
                    CASE WHEN julianday(MIN(COALESCE(s.last_activity_at, s.ended_at, s.started_at), ?2))
                              > julianday(MAX(s.started_at, ?1))
                         THEN CAST((julianday(MIN(COALESCE(s.last_activity_at, s.ended_at, s.started_at), ?2))
                                  - julianday(MAX(s.started_at, ?1))) * 86400000 AS INTEGER)
                         ELSE 0 END),0)
                 FROM sessions s
                 WHERE s.started_at < ?2
                   AND COALESCE(s.last_activity_at, s.ended_at, s.started_at) > ?1 {session_filter}
                   AND s.parent_session_id IS NULL"
            ),
            params_from_iter(range_args(&from, &to, session_fargs)),
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)),
        )
        .unwrap_or((0, None));
    OverviewActivity {
        sessions: sessions_value,
        active_sessions: active_sessions_value,
        messages: messages_value,
        user_messages: user_messages_value,
        tools: tools_value,
        session_duration_ms: if overlap_count > 0 { overlap_ms } else { None },
    }
}

fn add_json_number(body: &mut serde_json::Value, key: &str, delta: i64) {
    if delta == 0 {
        return;
    }
    let current = body
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    body[key] = serde_json::json!(current + delta);
}

/// `hourly_rollups` 完整小时 + 两端不完整小时明细补齐 的 CTE（来源名 `rollup_src`）。
///
/// 列名与 `hourly_rollups` 对齐，便于把查询里的 `hourly_rollups` 直接换成 `rollup_src`；
/// `from_ph`/`to_ph` 指定 from/to 绑定的参数序号（只使用这两个参数）。
/// 明细侧无法提供的字段（usage_source/pricing_source、
/// session/message/tool/turn/subagent 计数）以空串或 0 补齐。
fn rollup_source_cte(from_ph: u8, to_ph: u8, from: DateTime<Utc>, to: DateTime<Utc>) -> String {
    // 边缘判定原先对**每一行**执行 CAST(strftime(started_at))（解析整个时间戳），
    // 30 天范围 17.5 万行上是 breakdown/timeseries 各 ~70ms 的主要来源。
    // 改用 13 位小时前缀 `YYYY-MM-DDTHH` 比较：与 `Z` / `+00:00` 尾缀无关，
    // 也不需要解析时间戳，语义仍是「整点小时落在 [ceil(from), floor(to)) 内的走 rollup」。
    let full_from = ceil_hour(from).format("%Y-%m-%dT%H").to_string();
    let full_to = floor_hour(to).format("%Y-%m-%dT%H").to_string();
    format!(
        r#"WITH rollup_src AS (
            SELECT bucket, node_id, collector_id, client_id, source_id, project_id, provider, model,
                   usage_source, usage_granularity, pricing_source,
                   input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                   reported_cost, calculated_cost, estimated_cost,
                   session_count, model_call_count, turn_count, message_count, tool_call_count, subagent_count
            FROM hourly_rollups
            WHERE substr(bucket, 1, 13) >= '{full_from}'
              AND substr(bucket, 1, 13) < '{full_to}'
            UNION ALL
            SELECT strftime('%Y-%m-%dT%H:00:00+00:00', started_at), node_id, collector_id, client_id, source_id,
                   COALESCE(project_id,''), COALESCE(provider_normalized,''), COALESCE(model_normalized,''),
                   '', '', '',
                   COALESCE(input_tokens,0), COALESCE(output_tokens,0), COALESCE(cache_read_tokens,0), COALESCE(cache_write_tokens,0), COALESCE(reasoning_tokens,0),
                   COALESCE(reported_cost_micro_usd,0), COALESCE(calculated_cost_micro_usd,0), COALESCE(estimated_cost_micro_usd,0),
                   0, 1, 0, 0, 0, 0
            FROM model_calls
            WHERE started_at >= ?{from_ph} AND started_at < ?{to_ph}
              AND NOT (substr(started_at, 1, 13) >= '{full_from}'
                   AND substr(started_at, 1, 13) < '{full_to}')
        ) "#
    )
}

#[cfg(test)]
mod rollup_window_tests {
    use super::{edge_windows, full_hour_bounds};
    use chrono::{DateTime, Utc};

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn full_hour_bounds_ceil_and_floor() {
        let (hf, ht) = full_hour_bounds(ts("2026-09-16T00:29:14.165Z"), ts("2026-09-16T03:15:00Z"));
        assert_eq!(hf, ts("2026-09-16T01:00:00Z"));
        assert_eq!(ht, ts("2026-09-16T03:00:00Z"));
        let (hf2, ht2) = full_hour_bounds(ts("2026-09-16T01:00:00Z"), ts("2026-09-16T03:00:00Z"));
        assert_eq!(hf2, ts("2026-09-16T01:00:00Z"));
        assert_eq!(ht2, ts("2026-09-16T03:00:00Z"));
    }

    #[test]
    fn edge_windows_cover_only_partial_hours() {
        // 两边都不完整
        let w = edge_windows(
            ts("2026-09-16T00:29:00Z"),
            ts("2026-09-16T03:15:00Z"),
            ts("2026-09-16T01:00:00Z"),
            ts("2026-09-16T03:00:00Z"),
        );
        assert_eq!(
            w,
            vec![
                (ts("2026-09-16T00:29:00Z"), ts("2026-09-16T01:00:00Z")),
                (ts("2026-09-16T03:00:00Z"), ts("2026-09-16T03:15:00Z"))
            ]
        );
        // 对齐：无补边
        assert!(edge_windows(
            ts("2026-09-16T01:00:00Z"),
            ts("2026-09-16T03:00:00Z"),
            ts("2026-09-16T01:00:00Z"),
            ts("2026-09-16T03:00:00Z"),
        )
        .is_empty());
        // 不足一小时且不对齐：整段走明细
        let short = edge_windows(
            ts("2026-09-16T00:29:00Z"),
            ts("2026-09-16T00:45:00Z"),
            ts("2026-09-16T01:00:00Z"),
            ts("2026-09-16T00:00:00Z"),
        );
        assert_eq!(
            short,
            vec![(ts("2026-09-16T00:29:00Z"), ts("2026-09-16T00:45:00Z"))]
        );
    }
}

#[cfg(test)]
mod cache_hit_rate_tests {
    use super::cache_hit_rate;

    #[test]
    fn includes_cache_write_in_the_denominator() {
        // 900 / (100 + 100 + 900)
        let rate = cache_hit_rate(100, 100, 900).unwrap();
        assert!((rate - 900.0 / 1100.0).abs() < 1e-12);
        assert_eq!(cache_hit_rate(100, 0, 900), Some(0.9));
    }

    #[test]
    fn unavailable_without_cache_data() {
        assert_eq!(cache_hit_rate(100, 0, 0), None);
        assert_eq!(cache_hit_rate(0, 0, 0), None);
    }

    #[test]
    fn only_cache_writes_still_reports_zero_hit_rate() {
        assert_eq!(cache_hit_rate(0, 500, 0), Some(0.0));
    }
}
