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

use crate::config::HubConfig;
use crate::db::HubDb;
use metria_storage::rusqlite::types::Value as SqlValue;

pub mod handlers_misc;
pub mod handlers_query;
use handlers_misc::*;
use handlers_query::*;

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
#[macro_export]
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
        .route(
            "/api/v1/collectors/{id}/tokens",
            get(list_collector_tokens_handler).post(rotate_collector_token_handler),
        )
        .route(
            "/api/v1/collectors/{id}/tokens/revoke",
            post(revoke_collector_token_handler),
        )
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
        .route("/api/v1/sessions/{id}/subagents", get(session_subagents))
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
        .route(
            "/api/v1/pricing/rules/{id}",
            axum::routing::put(pricing_rule_update).delete(pricing_rule_delete),
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
    // token 管理端点需要 Admin 会话（rotete/revoke 是运维操作，非 collector 请求）
    if path.contains("/collectors/") && path.contains("/tokens") {
        if auth_user(&st, &headers).is_none() {
            return json_err(StatusCode::UNAUTHORIZED, "unauthorized", "需要 Admin 会话");
        }
        return next.run(req).await;
    }
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

// ============ 工具 ============

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "version": crate::VERSION }))
}

pub(crate) fn json_err(status: StatusCode, error: &str, message: &str) -> Response {
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
    if let Some(u) = verify_session(tok) {
        return Some(u);
    }
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
    use std::io::Read;
    let mut dec = zstd::stream::read::Decoder::new(data).map_err(|e| e.to_string())?;
    let cap = metria_protocol::limits::MAX_UNCOMPRESSED_BODY;
    // 限制解码输出大小，防止 zip bomb 爆内存（S2.10）
    let mut out = Vec::with_capacity(cap.min(1 << 20));
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = dec
            .read(&mut buf)
            .map_err(|e| format!("zstd 解码失败: {e}"))?;
        if n == 0 {
            break;
        }
        if out.len() + n > cap {
            return Err("解压后超过大小上限".into());
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

fn blake3_hex(s: &str) -> String {
    metria_core::model::ContentHash::hash_str(s)
        .as_str()
        .to_string()
}

pub(crate) fn hash_password(p: &str) -> String {
    use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default()
        .hash_password(p.as_bytes(), &salt)
        .map(|h| h.to_string())
        .unwrap_or_else(|_| format!("prehash:{}", blake3_hex(p)))
}

fn verify_password(plain: &str, hash: &str) -> bool {
    // 兼容旧版占位哈希（M1 之前版本）
    if let Some(old) = hash.strip_prefix("prehash:") {
        return blake3_hex(plain) == old;
    }
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    match PasswordHash::new(hash) {
        Ok(parsed) => argon2::Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
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
    /// 调用归属时间口径：call_start（默认，用 started_at）或 call_end（用 completed_at）。
    pub allocation_mode: Option<String>,
    /// 汇总维度：node/client/model/provider/project（breakdown 用）。
    pub dim: Option<String>,
    /// SSE 通过 EventSource 连接，无法携带 Authorization 头，允许用 query 传会话 token。
    pub token: Option<String>,
}

pub(crate) fn parse_range(p: &RangeParams) -> (DateTime<Utc>, DateTime<Utc>) {
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
pub(crate) fn range_filter(p: &RangeParams) -> (String, Vec<SqlValue>) {
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

pub(crate) fn range_args(
    from: &DateTime<Utc>,
    to: &DateTime<Utc>,
    extra: Vec<SqlValue>,
) -> Vec<SqlValue> {
    let mut v = vec![
        SqlValue::Text(from.to_rfc3339()),
        SqlValue::Text(to.to_rfc3339()),
    ];
    v.extend(extra);
    v
}

/// 解析 allocation_mode：call_start（默认）/ call_end。
pub(crate) fn time_column(mode: Option<&str>) -> &'static str {
    match mode {
        Some("call_end") => "completed_at",
        _ => "started_at",
    }
}

/// 编码分页游标：base64("<ts>|<id>")。
pub(crate) fn encode_cursor(ts: &str, id: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(format!("{ts}|{id}"))
}

/// 解码分页游标，失败返回 None。
pub(crate) fn decode_cursor(c: &str) -> Option<(String, String)> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD.decode(c).ok()?;
    let s = String::from_utf8(raw).ok()?;
    let (ts, id) = s.split_once('|')?;
    Some((ts.to_string(), id.to_string()))
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

fn session_secret() -> Vec<u8> {
    use sha2::Digest;
    let secret = std::env::var("METRIA_SESSION_SECRET")
        .unwrap_or_else(|_| "metria-dev-session-secret-change-me".into());
    sha2::Sha256::digest(secret.as_bytes()).to_vec()
}

/// 签发签名会话 token：`sess.<username>.<sig>`（sig = HMAC-SHA256(secret, username)）。
fn sign_session(username: &str) -> String {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    type H = Hmac<sha2::Sha256>;
    let mut mac = H::new_from_slice(&session_secret()).expect("hmac key");
    mac.update(username.as_bytes());
    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!(
        "sess.{}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(username),
        sig
    )
}

/// 校验签名会话 token，返回用户名（验签失败返回 None）。
fn verify_session(token: &str) -> Option<String> {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    type H = Hmac<sha2::Sha256>;
    let mut parts = token.split('.');
    let prefix = parts.next()?;
    if prefix != "sess" {
        return None;
    }
    let user_b64 = parts.next()?;
    let sig_b64 = parts.next()?;
    let username = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(user_b64)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())?;
    let mut mac = H::new_from_slice(&session_secret()).ok()?;
    mac.update(username.as_bytes());
    let expected =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    if expected == sig_b64 {
        Some(username)
    } else {
        None
    }
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
    let token = sign_session(&req.username);
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
    // 协议版本协商：不兼容直接拒绝，避免静默 schema 错配
    if req.protocol_version != metria_protocol::limits::PROTOCOL_VERSION {
        return json_err(
            StatusCode::BAD_REQUEST,
            "protocol_version_unsupported",
            &format!(
                "不支持的协议版本 {}（Hub 支持 {}）",
                req.protocol_version,
                metria_protocol::limits::PROTOCOL_VERSION
            ),
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
    // 时钟偏移检测：agent_clock 与 Hub 本地时钟之差（秒）
    let skew = (Utc::now() - req.agent_clock).num_seconds();
    match st.db.heartbeat(
        &req.node_id,
        &req.collector_id,
        req.spool_pending_events,
        req.spool_size_bytes,
        Utc::now(),
        skew,
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

/// 列出 collector 的 token（需 Admin 会话）。
async fn list_collector_tokens_handler(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    Json(serde_json::json!({ "tokens": st.db.list_collector_tokens(&id) })).into_response()
}

/// 轮换 collector token：吊销旧 token，签发新 token 返回明文。
async fn rotate_collector_token_handler(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let new_token = format!("mct-{}", metria_core::model::Id::new());
    match st.db.rotate_collector_token(&id, &new_token) {
        Ok(()) => Json(serde_json::json!({
            "ok": true, "collector_id": id, "token": new_token
        }))
        .into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_failed",
            &e.to_string(),
        ),
    }
}

/// 吊销 collector 全部 active token。
async fn revoke_collector_token_handler(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match st.db.revoke_collector_token(&id) {
        Ok(n) => Json(serde_json::json!({ "ok": true, "revoked": n })).into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "revoke_failed",
            &e.to_string(),
        ),
    }
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

#[cfg(test)]
mod auth_tests {
    use super::*;

    #[test]
    fn argon2_hash_and_verify_roundtrip() {
        let h = hash_password("s3cret");
        assert!(h.starts_with("$argon2"), "应生成 PHC argon2 哈希: {h}");
        assert!(verify_password("s3cret", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn legacy_prehash_still_verifies() {
        let old = format!("prehash:{}", blake3_hex("oldpass"));
        assert!(verify_password("oldpass", &old));
        assert!(!verify_password("nope", &old));
    }

    #[test]
    fn signed_session_roundtrip() {
        let tok = sign_session("admin");
        assert!(tok.starts_with("sess."));
        assert_eq!(verify_session(&tok).as_deref(), Some("admin"));
        // 篡改 token 应校验失败
        assert_eq!(verify_session(&format!("{tok}x")), None);
        assert_eq!(verify_session("sess.garbage"), None);
    }
}
