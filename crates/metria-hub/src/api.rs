//! Hub HTTP API：认证、Collector 协议、查询、SSE。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use metria_protocol::{
    CollectorStatusResponse, HeartbeatRequest, HeartbeatResponse, RegisterRequest,
    RegisterResponse, UploadBatch, UploadResponse,
};
use serde::Deserialize;
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use tracing::error;

use crate::config::HubConfig;
use crate::db::HubDb;
use metria_storage::rusqlite::{params, params_from_iter, types::Value as SqlValue};

/// 应用状态。
#[derive(Debug, Clone)]
pub struct AppState {
    pub db: HubDb,
    pub cfg: HubConfig,
    pub sse: SseHub,
    pub sessions: Arc<Mutex<HashMap<String, String>>>,
    pub collector_token: Option<String>,
}

/// SSE 广播。
#[derive(Debug, Clone)]
pub struct SseHub {
    tx: broadcast::Sender<String>,
}

impl SseHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn publish(&self, event: &str, data: &str) {
        let _ = self.tx.send(format!("event: {event}\ndata: {data}\n\n"));
    }
}

impl Default for SseHub {
    fn default() -> Self {
        Self::new()
    }
}

/// 数据库错误快速返回。
macro_rules! q {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => {
                return json_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "db_error",
                    &e.to_string(),
                )
            }
        }
    };
}

/// 构建应用路由。
pub fn app_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/change-password", post(change_password))
        .route("/api/v1/collectors/register", post(register))
        .route("/api/v1/collectors/heartbeat", post(heartbeat))
        .route("/api/v1/collectors/status", get(collector_status))
        .route("/api/v1/collectors/config", get(collector_config))
        .route("/api/v1/events/batch", post(ingest_batch))
        .route("/api/v1/stream", get(sse_stream))
        .route("/api/v1/overview", get(overview))
        .route("/api/v1/usage/timeseries", get(usage_timeseries))
        .route("/api/v1/usage/breakdown", get(usage_breakdown))
        .route("/api/v1/nodes", get(list_nodes))
        .route("/api/v1/nodes/{id}", get(node_detail))
        .route("/api/v1/nodes/{id}/clients", get(node_clients))
        .route("/api/v1/nodes/{id}/sessions", get(node_sessions))
        .route("/api/v1/nodes/{id}/calls", get(node_calls))
        .route("/api/v1/clients", get(list_clients))
        .route("/api/v1/clients/{id}", get(client_detail))
        .route("/api/v1/clients/{id}/models", get(client_models))
        .route("/api/v1/models", get(list_models))
        .route("/api/v1/models/{id}", get(model_detail))
        .route("/api/v1/calls", get(list_calls))
        .route("/api/v1/calls/{id}", get(call_detail))
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/sessions/{id}", get(session_detail))
        .route("/api/v1/sessions/{id}/calls", get(session_calls))
        .route("/api/v1/sessions/{id}/tools", get(session_tools))
        .route("/api/v1/sessions/{id}/timeline", get(session_timeline))
        .route("/api/v1/traffic/summary", get(traffic_summary))
        .route("/api/v1/traffic/by-node", get(traffic_by_node))
        .route("/api/v1/traffic/by-client", get(traffic_by_client))
        .route("/api/v1/traffic/by-model", get(traffic_by_model))
        .route("/api/v1/traffic/by-provider", get(traffic_by_provider))
        .route("/api/v1/data-quality", get(data_quality))
        .route("/api/v1/shares", post(share_create).get(share_list))
        .route("/api/v1/share/{slug}", get(share_view))
        .route("/api/v1/export", get(export_data))
        .route(
            "/api/v1/traffic/profiles",
            get(traffic_profiles_list).post(traffic_profiles_create),
        )
        .route(
            "/api/v1/traffic/profiles/{id}",
            axum::routing::delete(traffic_profiles_delete),
        )
        .route(
            "/api/v1/traffic/profiles/learn",
            post(traffic_profiles_learn),
        )
        .route("/api/v1/traffic/profiles/test", post(traffic_profiles_test))
        .route("/api/v1/traffic/reestimate", post(traffic_reestimate))
        .route("/api/v1/pricing/catalogs", get(pricing_catalogs))
        .route(
            "/api/v1/pricing/catalogs/{id}/refresh",
            post(pricing_catalog_refresh),
        )
        .route("/api/v1/pricing/snapshots", get(pricing_snapshots))
        .route("/api/v1/pricing/reprice", post(pricing_reprice))
        .route(
            "/api/v1/pricing/rules",
            get(pricing_rules).post(pricing_rules_create),
        )
        .route("/api/v1/pricing/test", post(pricing_test))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_mw))
        .layer(TraceLayer::new_for_http())
}

/// 认证中间件：查询端点要求 admin 会话；collector 端点要求 collector token。
async fn auth_mw(
    State(st): State<AppState>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> Response {
    // 忽略 next 的显式绑定：Next 通过闭包调用保持生命周期正确
    let _ = &next;
    let path = req.uri().path().to_string();
    if path == "/healthz" || path == "/api/v1/auth/login" || path.starts_with("/api/v1/share/") {
        return next.run(req).await;
    }
    let headers = req.headers().clone();
    if path.starts_with("/api/v1/collectors/") || path == "/api/v1/events/batch" {
        let tok = bearer(&headers).unwrap_or("");
        if !check_collector_token(&st, tok) {
            return json_err(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "collector token 无效",
            );
        }
        return next.run(req).await;
    }
    if path.starts_with("/api/v1/")
        && auth_user(&st, &headers).is_none()
        && !token_query_ok(&st, req.uri().query())
    {
        return json_err(StatusCode::UNAUTHORIZED, "unauthorized", "未登录");
    }
    next.run(req).await
}

/// 校验 query 中携带的会话 token（用于 SSE EventSource）。
fn token_query_ok(st: &AppState, query: Option<&str>) -> bool {
    let Some(query) = query else {
        return false;
    };
    let sp: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    sp.iter()
        .find(|(k, _)| k == "token")
        .map(|(_, v)| st.sessions.lock().unwrap().contains_key(v.as_str()))
        .unwrap_or(false)
}

// ============ Traffic Profiles ============

async fn traffic_profiles_list(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({ "profiles": st.db.list_traffic_profiles(None) })).into_response()
}

async fn traffic_profiles_create(
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

async fn traffic_profiles_delete(
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

async fn traffic_profiles_learn(
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
struct ProfileTestRequest {
    client: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
}

async fn traffic_profiles_test(
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
struct ReestimateRequest {
    model: Option<String>,
}

async fn traffic_reestimate(
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

async fn pricing_catalogs(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({ "catalogs": st.db.list_pricing_catalogs() })).into_response()
}

async fn pricing_rules(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({ "rules": st.db.list_pricing_rules() })).into_response()
}

async fn pricing_rules_create(
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
struct PricingTestRequest {
    model: Option<String>,
    provider: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
}

async fn pricing_test(State(st): State<AppState>, Json(req): Json<PricingTestRequest>) -> Response {
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

async fn pricing_catalog_refresh(
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

async fn pricing_snapshots(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({ "snapshots": st.db.list_pricing_snapshots() })).into_response()
}

#[derive(Debug, Deserialize)]
struct RepriceRequest {
    // 预留筛选
}

async fn pricing_reprice(State(st): State<AppState>, _req: Json<RepriceRequest>) -> Response {
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
struct ShareCreateRequest {
    kind: String,
    target_id: String,
}

async fn share_create(State(st): State<AppState>, Json(req): Json<ShareCreateRequest>) -> Response {
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

async fn share_list(State(st): State<AppState>) -> Response {
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
async fn share_view(State(st): State<AppState>, AxumPath(slug): AxumPath<String>) -> Response {
    let Some((kind, target)) = crate::share::resolve_share(&st.db, &slug) else {
        return json_err(StatusCode::NOT_FOUND, "share_not_found", "分享链接不存在");
    };
    crate::share::record_view(&st.db, &slug);
    Json(crate::share::build_share_dto(&st.db, &kind, &target)).into_response()
}

#[derive(Debug, Default, Deserialize)]
struct ExportParams {
    kind: Option<String>,
    format: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

async fn export_data(State(st): State<AppState>, Query(p): Query<ExportParams>) -> Response {
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

// ============ 工具 ============

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "version": crate::VERSION }))
}

fn json_err(status: StatusCode, error: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": error, "message": message })),
    )
        .into_response()
}

fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

fn auth_user(st: &AppState, headers: &axum::http::HeaderMap) -> Option<String> {
    let tok = bearer(headers)?;
    st.sessions.lock().unwrap().get(tok).cloned()
}

fn check_collector_token(st: &AppState, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if let Some(configured) = &st.collector_token {
        if configured == token {
            return true;
        }
    }
    st.db.verify_collector_token(token).is_some()
}

fn publish_ingest(st: &AppState, kind: &str) {
    let event = match kind {
        "usage" => "usage.created",
        "call" => "call.updated",
        "session" => "session.updated",
        "traffic" => "traffic.estimated",
        _ => "rollup.updated",
    };
    st.sse.publish(event, "{}");
}

fn body_encoding(body: &[u8]) -> &'static str {
    if body.len() >= 4 && body[..4] == [0x28, 0xB5, 0x2F, 0xFD] {
        "zstd"
    } else {
        "raw"
    }
}

fn zstd_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    zstd::stream::decode_all(data).map_err(|e| e.to_string())
}

fn blake3_hex(s: &str) -> String {
    metria_core::model::ContentHash::hash_str(s)
        .as_str()
        .to_string()
}

fn hash_password(p: &str) -> String {
    // M1 占位哈希；生产部署务必设置 METRIA_ADMIN_PASSWORD（后续替换为 argon2）。
    format!("prehash:{}", blake3_hex(p))
}

fn verify_password(plain: &str, hash: &str) -> bool {
    hash_password(plain) == hash
}

/// 查询参数。
#[derive(Debug, Default, Deserialize)]
pub struct RangeParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub timezone: Option<String>,
    pub granularity: Option<String>,
    pub node_id: Option<String>,
    pub client_id: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub project_id: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    /// SSE 通过 EventSource 连接，无法携带 Authorization 头，允许用 query 传会话 token。
    pub token: Option<String>,
}

fn parse_range(p: &RangeParams) -> (DateTime<Utc>, DateTime<Utc>) {
    let from = p
        .from
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| Utc::now() - chrono::Duration::days(7));
    let to =
        p.to.as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
    (from, to)
}

/// 构建范围过滤 SQL 与参数（?1=?from, ?2=?to 之后追加过滤器参数）。
fn range_filter(p: &RangeParams) -> (String, Vec<SqlValue>) {
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
        parts.push(format!("model = ?{}", args.len() + 2));
    }
    if let Some(v) = &p.provider {
        args.push(v.clone().into());
        parts.push(format!("provider = ?{}", args.len() + 2));
    }
    let cond = if parts.is_empty() {
        String::new()
    } else {
        format!(" AND {}", parts.join(" AND "))
    };
    (cond, args)
}

fn range_args(from: &DateTime<Utc>, to: &DateTime<Utc>, extra: Vec<SqlValue>) -> Vec<SqlValue> {
    let mut v = vec![
        SqlValue::Text(from.to_rfc3339()),
        SqlValue::Text(to.to_rfc3339()),
    ];
    v.extend(extra);
    v
}

// ============ 认证 ============

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ChangePasswordRequest {
    old_password: String,
    new_password: String,
}

fn admin_hash() -> (String, String) {
    let user = std::env::var("METRIA_ADMIN_USER").unwrap_or_else(|_| "admin".into());
    let pass = std::env::var("METRIA_ADMIN_PASSWORD").unwrap_or_else(|_| "metria-admin".into());
    (user, hash_password(&pass))
}

async fn login(State(st): State<AppState>, Json(req): Json<LoginRequest>) -> Response {
    let (user, hash) = admin_hash();
    if req.username != user || !verify_password(&req.password, &hash) {
        return json_err(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "用户名或密码错误",
        );
    }
    let token = format!("sess-{}", metria_core::model::Id::new());
    st.sessions
        .lock()
        .unwrap()
        .insert(token.clone(), req.username.clone());
    Json(serde_json::json!({ "token": token, "username": req.username })).into_response()
}

async fn logout(State(st): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Some(tok) = bearer(&headers) {
        st.sessions.lock().unwrap().remove(tok);
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

async fn me(State(st): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    match auth_user(&st, &headers) {
        Some(u) => Json(serde_json::json!({ "username": u, "ok": true })).into_response(),
        None => json_err(StatusCode::UNAUTHORIZED, "unauthorized", "未登录"),
    }
}

async fn change_password(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
    _req: Json<ChangePasswordRequest>,
) -> Response {
    if auth_user(&st, &headers).is_none() {
        return json_err(StatusCode::UNAUTHORIZED, "unauthorized", "未登录");
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

// ============ Collector 协议 ============

async fn register(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Response {
    let tok = bearer(&headers).unwrap_or("");
    if !check_collector_token(&st, tok) {
        return json_err(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "collector token 无效",
        );
    }
    let now = Utc::now();
    match st.db.register_node_collector(
        &req.node_id,
        &req.node_name,
        req.node_platform.as_deref(),
        req.node_architecture.as_deref(),
        &req.agent_version,
        req.protocol_version,
        now,
    ) {
        Ok((collector_id, _)) => {
            // 持久化 token 哈希供后续鉴权
            let _ = st.db.upsert_collector_token(&collector_id, tok);
            Json(RegisterResponse {
                node_id: req.node_id.clone(),
                collector_id,
                ok: true,
                message: None,
            })
            .into_response()
        }
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "register_failed",
            &e.to_string(),
        ),
    }
}

async fn heartbeat(State(st): State<AppState>, Json(req): Json<HeartbeatRequest>) -> Response {
    match st.db.heartbeat(
        &req.node_id,
        &req.collector_id,
        req.spool_pending_events,
        req.spool_size_bytes,
        Utc::now(),
    ) {
        Ok(()) => Json(HeartbeatResponse {
            ok: true,
            config: None,
        })
        .into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "heartbeat_failed",
            &e.to_string(),
        ),
    }
}

async fn collector_status(State(_st): State<AppState>) -> Response {
    Json(CollectorStatusResponse {
        ok: true,
        node_id: String::new(),
        collector_id: String::new(),
        hub_time: Utc::now(),
    })
    .into_response()
}

async fn collector_config(State(_st): State<AppState>) -> Response {
    Json(metria_protocol::CollectorConfig {
        content_mode: Some("metadata".into()),
        scan_interval_seconds: None,
        pricing_rules_etag: None,
    })
    .into_response()
}

/// 批处理：解压 → 校验 → 幂等落库 → rollup → 部分成功响应。
async fn ingest_batch(State(st): State<AppState>, body: axum::body::Bytes) -> Response {
    let decompressed: Vec<u8> = if body_encoding(body.as_ref()) == "zstd" {
        match zstd_decode(body.as_ref()) {
            Ok(d) => d,
            Err(e) => return json_err(StatusCode::BAD_REQUEST, "decode_failed", &e),
        }
    } else {
        body.to_vec()
    };
    if decompressed.len() > metria_protocol::limits::MAX_UNCOMPRESSED_BODY {
        return json_err(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body_too_large",
            "解压后超过大小上限",
        );
    }
    let batch: UploadBatch = match serde_json::from_slice(&decompressed) {
        Ok(b) => b,
        Err(e) => return json_err(StatusCode::BAD_REQUEST, "invalid_batch", &e.to_string()),
    };
    if let Err(e) = metria_protocol::validate_batch(&batch) {
        return json_err(StatusCode::BAD_REQUEST, "invalid_batch", &e);
    }

    let mut accepted = Vec::new();
    let mut duplicate = Vec::new();
    let mut failed = Vec::new();

    let session_map = st.db.session_key_map(&serde_json::json!({
        "sessions": batch.events.iter().filter(|e| e.kind == "session").map(|e| e.payload.clone()).collect::<Vec<_>>()
    }));

    for ev in &batch.events {
        let v = &ev.payload;
        let node = v
            .get("node_id")
            .and_then(|x| x.as_str())
            .unwrap_or(&batch.node_id);
        let src_sid = v
            .get("source_session_id")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let sid_ulid = v.get("session_id").and_then(|x| x.as_str());
        let resolved: Option<String> = if !src_sid.is_empty() {
            Some(HubDb::session_key(node, src_sid))
        } else if let Some(ulid) = sid_ulid {
            session_map
                .get(ulid)
                .cloned()
                .or_else(|| st.db.resolve_session_key_by_id(ulid))
        } else {
            None
        };

        let result: Result<bool, metria_storage::StorageError> = match ev.kind.as_str() {
            "session" => st.db.upsert_session(v),
            "source" => st.db.upsert_source(v),
            "message" => st
                .db
                .insert_message(v, resolved.as_deref().unwrap_or_default()),
            "call" => st
                .db
                .insert_call(v, resolved.as_deref().unwrap_or_default()),
            "usage" => st
                .db
                .insert_usage(v, resolved.as_deref().unwrap_or_default()),
            "traffic" => st.db.insert_traffic(v),
            "traffic_sample" => st.db.insert_traffic_profile_sample(v),
            "tool" => st.db.insert_tool(v),
            "subagent" => st.db.insert_subagent(v),
            other => Err(metria_storage::StorageError::Query(format!(
                "未知事件类型 {other}"
            ))),
        };

        match result {
            Ok(is_new) => {
                if is_new {
                    accepted.push(ev.event_id.clone());
                    let _ = st.db.rollup_event(&ev.kind, v);
                    publish_ingest(&st, &ev.kind);
                } else {
                    duplicate.push(ev.event_id.clone());
                }
            }
            Err(e) => {
                failed.push(metria_protocol::FailedEvent {
                    event_id: ev.event_id.clone(),
                    reason: e.to_string(),
                    retryable: false,
                });
            }
        }
    }

    let _ = st.db.record_batch(
        &batch.batch_id,
        &batch.node_id,
        &batch.collector_id,
        batch.events.len() as i64,
        decompressed.len() as i64,
    );

    Json(UploadResponse {
        batch_id: batch.batch_id,
        ok: failed.is_empty(),
        accepted,
        duplicate,
        failed,
        message: None,
    })
    .into_response()
}

// ============ SSE ============

async fn sse_stream(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(p): Query<RangeParams>,
) -> Response {
    let authed = auth_user(&st, &headers).is_some()
        || p.token
            .as_deref()
            .map(|t| st.sessions.lock().unwrap().contains_key(t))
            .unwrap_or(false);
    if !authed {
        return json_err(StatusCode::UNAUTHORIZED, "unauthorized", "未登录");
    }
    use axum::response::sse::Event;
    let mut rx = st.sse.tx.subscribe();
    let stream = async_stream::stream! {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                Ok(msg) = rx.recv() => {
                    yield Ok::<_, std::convert::Infallible>(Event::default().data(msg));
                }
                _ = tick.tick() => {
                    yield Ok::<_, std::convert::Infallible>(Event::default().event("ping").data("{}"));
                }
            }
        }
    };
    Sse::new(stream).into_response()
}

// ============ 查询 ============

async fn overview(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
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
        error!(%e, "overview 查询失败");
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

async fn usage_timeseries(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
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

async fn usage_breakdown(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
    let (from, to) = parse_range(&p);
    let (filter, fargs) = range_filter(&p);
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(&format!(
        "SELECT node_id, COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
            COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(model_call_count),0)
         FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 {filter}
         GROUP BY node_id ORDER BY 5 DESC"
    )));
    let rows = q!(
        stmt.query_map(params_from_iter(range_args(&from, &to, fargs)), |r| {
            Ok(serde_json::json!({
                "node_id": r.get::<_, String>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "estimated_traffic_bytes": r.get::<_, i64>(3)?,
                "model_calls": r.get::<_, i64>(4)?,
            }))
        },)
    );
    let items: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "by_node": items })).into_response()
}

async fn list_nodes(State(st): State<AppState>) -> Response {
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

async fn node_detail(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
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
    Json(serde_json::json!({ "node": node, "clients": clients })).into_response()
}

async fn node_clients(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
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

async fn node_sessions(
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
    Json(serde_json::json!({ "sessions": sessions })).into_response()
}

async fn node_calls(
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

async fn list_clients(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
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

async fn client_detail(
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
    Json(serde_json::json!({ "client_id": id, "by_node": by_node })).into_response()
}

async fn client_models(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
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

async fn list_models(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
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

async fn model_detail(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
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

async fn list_calls(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(100).min(1000);
    let mut stmt = q!(c.prepare(
        "SELECT id, client_id, session_id, provider_normalized, model_normalized, started_at, status,
            input_tokens, output_tokens, cache_read_tokens, reasoning_tokens,
            reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd
         FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 ORDER BY started_at DESC LIMIT ?3",
    ));
    let rows = q!(
        stmt.query_map(params![from.to_rfc3339(), to.to_rfc3339(), limit], |r| {
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
        },)
    );
    let calls: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "calls": calls })).into_response()
}

async fn call_detail(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
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

async fn list_sessions(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
    let (from, to) = parse_range(&p);
    let c = st.db.conn();
    let limit = p.limit.unwrap_or(100).min(1000);
    let mut stmt = q!(c.prepare(
        "SELECT id, source_session_id, client_id, title, provider_normalized, primary_model_normalized, started_at, ended_at,
            message_count, tool_call_count, model_call_count, input_tokens, output_tokens,
            reported_cost_micro_usd, estimated_total_bytes
         FROM sessions WHERE started_at >= ?1 AND started_at < ?2 ORDER BY started_at DESC LIMIT ?3",
    ));
    let rows = q!(
        stmt.query_map(params![from.to_rfc3339(), to.to_rfc3339(), limit], |r| {
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
        },)
    );
    let sessions: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "sessions": sessions })).into_response()
}

async fn session_detail(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
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

async fn session_calls(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
    let c = st.db.conn();
    let mut stmt = q!(c.prepare(
        "SELECT id, model_normalized, provider_normalized, started_at, status, input_tokens, output_tokens, cache_read_tokens, reasoning_tokens, calculated_cost_micro_usd
         FROM model_calls WHERE session_id = ?1 ORDER BY started_at",
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
        }))
    }));
    let calls: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Json(serde_json::json!({ "calls": calls })).into_response()
}

async fn session_tools(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
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

async fn session_timeline(State(st): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
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

async fn traffic_summary(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
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
        async fn $name(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
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

async fn data_quality(State(st): State<AppState>, Query(p): Query<RangeParams>) -> Response {
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
