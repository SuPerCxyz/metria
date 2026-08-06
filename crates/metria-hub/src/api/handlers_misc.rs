//! 补充 handlers：Traffic Profiles / Pricing / Share / Export。

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::api::{json_err, AppState, RangeParams};

// ============ Traffic Profiles ============

pub(crate) async fn traffic_profiles_list(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({ "profiles": st.db.list_traffic_profiles(None) })).into_response()
}

pub(crate) async fn traffic_profiles_create(
    State(st): State<AppState>,
    Json(v): Json<serde_json::Value>,
) -> Response {
    match st.db.insert_user_profile(&v) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "profile_failed",
            &e.to_string(),
        ),
    }
}

pub(crate) async fn traffic_profiles_delete(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match st.db.delete_user_profile(&id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "delete_failed",
            &e.to_string(),
        ),
    }
}

pub(crate) async fn traffic_profiles_learn(
    State(st): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Response {
    let min_samples = p.limit.unwrap_or(1).max(1);
    match st.db.aggregate_learned_profiles(min_samples) {
        Ok(n) => {
            st.sse.publish("traffic.profile_updated", "{}");
            Json(serde_json::json!({ "ok": true, "profiles_created": n })).into_response()
        }
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "learn_failed",
            &e.to_string(),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProfileTestRequest {
    client: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
}

pub(crate) async fn traffic_profiles_test(
    State(st): State<AppState>,
    Json(req): Json<ProfileTestRequest>,
) -> Response {
    let profiles = st.db.load_traffic_profiles_parsed();
    let client = req.client.clone().unwrap_or_else(|| "claude-code".into());
    let est = metria_traffic::estimate_with_candidates(
        &metria_traffic::EstimateInput {
            client: &client,
            provider: req.provider.as_deref(),
            model: req.model.as_deref(),
            input_tokens: req.input_tokens,
            output_tokens: req.output_tokens,
            cache_read_tokens: req.cache_read_tokens,
            cache_write_tokens: req.cache_write_tokens,
            reasoning_tokens: req.reasoning_tokens,
            streaming: true,
            request_text: None,
            response_text: None,
            request_reconstruction_quality: metria_core::model::ReconstructionQuality::None,
            response_reconstruction_quality: metria_core::model::ReconstructionQuality::None,
            context_transport_mode: metria_core::model::ContextTransportMode::Unknown,
            cache_transport_behavior: metria_core::model::CacheTransportBehavior::Unknown,
        },
        &profiles,
    );
    match est {
        Ok(out) => Json(serde_json::json!({
            "ok": true,
            "estimated_total_wire_bytes": out.estimated_total_wire_bytes,
            "lower_bound_bytes": out.lower_bound_bytes,
            "upper_bound_bytes": out.upper_bound_bytes,
            "confidence": out.confidence,
            "estimation_source": serde_json::to_value(out.estimation_source).unwrap_or(serde_json::json!("unknown")),
            "notes": out.notes,
        }))
        .into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "test_failed",
            &e.to_string(),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReestimateRequest {
    model: Option<String>,
}

pub(crate) async fn traffic_reestimate(
    State(st): State<AppState>,
    Json(req): Json<ReestimateRequest>,
) -> Response {
    match st.db.reestimate_calls(req.model.as_deref()) {
        Ok(n) => {
            st.sse.publish("traffic.profile_updated", "{}");
            Json(serde_json::json!({
                "ok": true,
                "reestimated": n,
                "note": "重新估算生成新版本并保留旧版本",
            }))
            .into_response()
        }
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "reestimate_failed",
            &e.to_string(),
        ),
    }
}

// ============ Pricing ============

pub(crate) async fn pricing_catalogs(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({ "catalogs": st.db.list_pricing_catalogs() })).into_response()
}

pub(crate) async fn pricing_rules(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({ "rules": st.db.list_pricing_rules() })).into_response()
}

pub(crate) async fn pricing_rules_create(
    State(st): State<AppState>,
    Json(v): Json<serde_json::Value>,
) -> Response {
    match st.db.insert_pricing_rule(&v) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rule_failed",
            &e.to_string(),
        ),
    }
}

/// 规则测试：给定 model/provider/tokens，返回匹配规则与计算费用。
#[derive(Debug, Deserialize)]
pub(crate) struct PricingTestRequest {
    model: Option<String>,
    provider: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
}

/// 更新用户价格规则（编辑/停用/生效区间）。
pub(crate) async fn pricing_rule_update(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(v): Json<serde_json::Value>,
) -> Response {
    match st.db.update_pricing_rule(&id, &v) {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => json_err(
            StatusCode::NOT_FOUND,
            "rule_not_found",
            "规则不存在或非用户规则",
        ),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rule_failed",
            &e.to_string(),
        ),
    }
}

/// 删除用户价格规则。
pub(crate) async fn pricing_rule_delete(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match st.db.delete_pricing_rule(&id) {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => json_err(
            StatusCode::NOT_FOUND,
            "rule_not_found",
            "规则不存在或非用户规则",
        ),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rule_failed",
            &e.to_string(),
        ),
    }
}

pub(crate) async fn pricing_test(
    State(st): State<AppState>,
    Json(req): Json<PricingTestRequest>,
) -> Response {
    // 从 DB 加载全部规则（用户 + 目录快照）
    let rules = st.db.load_all_rules();
    let mut engine = metria_pricing::PricingEngine::new();
    for r in rules {
        engine.add_rule(r);
    }
    let usage = metria_core::model::Usage {
        input: req.input_tokens,
        output: req.output_tokens,
        cache_read: req.cache_read_tokens,
        cache_write: req.cache_write_tokens,
        reasoning: req.reasoning_tokens,
    };
    match engine.compute(
        &usage,
        req.model.as_deref(),
        req.provider.as_deref(),
        Utc::now(),
        None,
    ) {
        Ok(c) => Json(serde_json::json!({
            "ok": true,
            "reported_micro_usd": c.reported_micro_usd,
            "calculated_micro_usd": c.calculated_micro_usd,
            "estimated_micro_usd": c.estimated_micro_usd,
            "rule_id": c.rule_id,
            "pricing_available": c.pricing_available,
            "note": "内置价格仅为近似参考，非厂商直连保证",
        }))
        .into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "pricing_test_failed",
            &e.to_string(),
        ),
    }
}

pub(crate) async fn pricing_catalog_refresh(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let defs = crate::catalog::catalogs_from_db(&st.db);
    let Some(cat) = defs.into_iter().find(|c| c.id == id) else {
        return json_err(
            StatusCode::NOT_FOUND,
            "catalog_not_found",
            "目录不存在或未启用",
        );
    };
    match crate::catalog::sync_catalog(&st.db, &cat) {
        Ok(r) => {
            st.sse.publish("pricing.updated", "{}");
            Json(serde_json::json!({
                "ok": true,
                "fetched": r.fetched,
                "rules": r.rules,
                "etag": r.etag,
            }))
            .into_response()
        }
        Err(e) => {
            let _ = st.db.mark_catalog_error(&cat.id, &e);
            Json(serde_json::json!({
                "ok": false,
                "error": e,
                "note": "同步失败，继续使用最后一个有效快照",
            }))
            .into_response()
        }
    }
}

pub(crate) async fn pricing_snapshots(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({ "snapshots": st.db.list_pricing_snapshots() })).into_response()
}

#[derive(Debug, Deserialize)]
pub(crate) struct RepriceRequest {
    // 预留筛选
}

pub(crate) async fn pricing_reprice(
    State(st): State<AppState>,
    _req: Json<RepriceRequest>,
) -> Response {
    let rules = st.db.load_all_rules();
    let mut engine = metria_pricing::PricingEngine::new();
    for r in rules {
        engine.add_rule(r);
    }
    match st.db.reprice_all(&engine) {
        Ok(n) => {
            st.sse.publish("pricing.updated", "{}");
            Json(serde_json::json!({
                "ok": true,
                "repriced": n,
                "note": "重新计价生成新版本并保留历史 pricing_matches",
            }))
            .into_response()
        }
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "reprice_failed",
            &e.to_string(),
        ),
    }
}

// ============ Share / Export ============

#[derive(Debug, Deserialize)]
pub(crate) struct ShareCreateRequest {
    kind: String,
    target_id: String,
}

pub(crate) async fn share_create(
    State(st): State<AppState>,
    Json(req): Json<ShareCreateRequest>,
) -> Response {
    match crate::share::create_share(&st.db, &req.kind, &req.target_id) {
        Ok(slug) => {
            Json(serde_json::json!({ "ok": true, "slug": slug, "url": format!("/s/{slug}") }))
                .into_response()
        }
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "share_failed",
            &e.to_string(),
        ),
    }
}

pub(crate) async fn share_list(State(st): State<AppState>) -> Response {
    let c = st.db.conn();
    let mut out = Vec::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT slug, kind, target_id, created_at FROM share_links ORDER BY created_at DESC LIMIT 100",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok(serde_json::json!({
                "slug": r.get::<_, String>(0)?,
                "kind": r.get::<_, String>(1)?,
                "target_id": r.get::<_, String>(2)?,
                "created_at": r.get::<_, String>(3)?,
            }))
        }) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    Json(serde_json::json!({ "shares": out })).into_response()
}

/// 公开只读分享视图（无鉴权，返回脱敏 DTO）。
pub(crate) async fn share_view(
    State(st): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let Some((kind, target)) = crate::share::resolve_share(&st.db, &slug) else {
        return json_err(StatusCode::NOT_FOUND, "share_not_found", "分享链接不存在");
    };
    crate::share::record_view(&st.db, &slug);
    Json(crate::share::build_share_dto(&st.db, &kind, &target)).into_response()
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ExportParams {
    kind: Option<String>,
    format: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

pub(crate) async fn export_data(
    State(st): State<AppState>,
    Query(p): Query<ExportParams>,
) -> Response {
    let fmt = match p.format.as_deref().and_then(crate::export::parse_format) {
        Some(f) => f,
        None => {
            return json_err(
                StatusCode::BAD_REQUEST,
                "bad_format",
                "format 支持 json/ndjson/csv",
            )
        }
    };
    let (from, to) = (
        p.from
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now() - chrono::Duration::days(30)),
        p.to.as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(Utc::now),
    );
    let result = match p.kind.as_deref() {
        Some("calls") | None => crate::export::export_calls(&st.db, from, to, &fmt),
        Some("sessions") => crate::export::export_sessions(&st.db, from, to, &fmt),
        Some(other) => {
            return json_err(
                StatusCode::BAD_REQUEST,
                "bad_kind",
                &format!("kind 支持 sessions/calls，得到 {other}"),
            )
        }
    };
    match result {
        Ok((body, filename)) => {
            let ct = match fmt {
                crate::export::Format::Json => "application/json",
                crate::export::Format::Ndjson => "application/x-ndjson",
                crate::export::Format::Csv => "text/csv",
            };
            (
                [
                    (axum::http::header::CONTENT_TYPE, ct),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        &format!("attachment; filename=\"{filename}\""),
                    ),
                ],
                body,
            )
                .into_response()
        }
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, "export_failed", &e),
    }
}
