//! 补充 handlers：Traffic Profiles / Pricing / Share / Export。

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::net::IpAddr;

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

const PRICE_FIELDS: [&str; 6] = [
    "input_price",
    "output_price",
    "cache_read_price",
    "cache_write_price",
    "reasoning_price",
    "request_price",
];

fn validate_pricing_rule(
    v: &serde_json::Value,
    require_model: bool,
    require_price: bool,
) -> Result<(), String> {
    let model = v.get("model_pattern").and_then(|x| x.as_str());
    if require_model && model.map(str::trim).unwrap_or("").is_empty() {
        return Err("模型匹配不能为空".into());
    }
    if model.is_some_and(|value| value.trim().is_empty()) {
        return Err("模型匹配不能为空".into());
    }

    let mut has_price = false;
    for key in PRICE_FIELDS {
        let Some(value) = v.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let parsed = value
            .as_i64()
            .or_else(|| value.as_str().and_then(|s| s.trim().parse::<i64>().ok()));
        let Some(price) = parsed else {
            return Err(format!("{key} 必须是非负整数微美元/百万 Token"));
        };
        if price < 0 {
            return Err(format!("{key} 不能为负数"));
        }
        has_price = true;
    }
    if require_price && !has_price {
        return Err("至少填写一项价格，免费模型请填写 0".into());
    }
    Ok(())
}

pub(crate) async fn pricing_rules_create(
    State(st): State<AppState>,
    Json(v): Json<serde_json::Value>,
) -> Response {
    if let Err(message) = validate_pricing_rule(&v, true, true) {
        return json_err(StatusCode::BAD_REQUEST, "invalid_pricing_rule", &message);
    }
    match st.db.insert_pricing_rule(&v) {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })).into_response(),
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
    if let Err(message) = validate_pricing_rule(&v, false, false) {
        return json_err(StatusCode::BAD_REQUEST, "invalid_pricing_rule", &message);
    }
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
            "pricing_source": engine.pricing_source(
                req.model.as_deref(),
                req.provider.as_deref(),
                Utc::now(),
            ),
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
            let repriced = if r.fetched {
                match crate::catalog::reprice_from_rules(&st.db, false) {
                    Ok(n) => Some(n),
                    Err(e) => {
                        tracing::warn!("同步后重新计价失败: {e}");
                        None
                    }
                }
            } else {
                None
            };
            st.sse.publish("pricing.updated", "{}");
            Json(serde_json::json!({
                "ok": true,
                "fetched": r.fetched,
                "rules": r.rules,
                "etag": r.etag,
                "repriced": repriced,
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

/// 更新价格目录（计费模板链接 / 启用状态）。
pub(crate) async fn pricing_catalog_update(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(v): Json<serde_json::Value>,
) -> Response {
    match st.db.update_pricing_catalog(&id, &v) {
        Ok(true) => {
            st.sse.publish("pricing.updated", "{}");
            Json(serde_json::json!({ "ok": true })).into_response()
        }
        Ok(false) => json_err(StatusCode::NOT_FOUND, "catalog_not_found", "价格目录不存在"),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "catalog_update_failed",
            &e.to_string(),
        ),
    }
}
#[derive(Debug, Deserialize)]
pub(crate) struct RepriceRequest {
    // 预留筛选
}

pub(crate) async fn pricing_reprice(
    State(st): State<AppState>,
    _req: Json<RepriceRequest>,
) -> Response {
    match crate::catalog::reprice_from_rules(&st.db, false) {
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

/// 删除（吊销）分享链接（需 Admin 会话）。
pub(crate) async fn share_delete(
    State(st): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    match crate::share::delete_share(&st.db, &slug) {
        Ok(true) => Json(serde_json::json!({ "ok": true, "slug": slug })).into_response(),
        Ok(false) => json_err(StatusCode::NOT_FOUND, "share_not_found", "分享链接不存在"),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "share_delete_failed",
            &e.to_string(),
        ),
    }
}

/// 分享查看审计（需 Admin 会话）。
pub(crate) async fn share_audits(State(st): State<AppState>) -> Response {
    Json(serde_json::json!({
        "audits": crate::share::list_share_audits(&st.db)
    }))
    .into_response()
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
            );
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
            );
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

// ============ 节点管理（前端创建 / 安装命令 / 编辑 / 删除） ============

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentTarget {
    pub platform: &'static str,
    pub architecture: &'static str,
    pub asset: &'static str,
}

/// 归一化节点安装目标，避免把 x86_64/arm64 等别名直接拼进资产名。
pub(crate) fn normalize_agent_target(
    platform: Option<&str>,
    architecture: Option<&str>,
) -> Result<AgentTarget, String> {
    let platform = platform.unwrap_or("linux").trim().to_ascii_lowercase();
    let architecture = architecture.unwrap_or("amd64").trim().to_ascii_lowercase();
    let architecture = match architecture.as_str() {
        "amd64" | "x86_64" | "x64" => "amd64",
        "arm64" | "aarch64" => "arm64",
        other => return Err(format!("不支持的 Agent 架构: {other}")),
    };
    match platform.as_str() {
        "linux" => Ok(AgentTarget {
            platform: "linux",
            architecture,
            asset: match architecture {
                "amd64" => "metria-linux-amd64",
                "arm64" => "metria-linux-arm64",
                _ => unreachable!(),
            },
        }),
        "windows" | "win" => {
            if architecture != "amd64" {
                return Err("Windows Agent 当前仅支持 amd64".into());
            }
            Ok(AgentTarget {
                platform: "windows",
                architecture: "amd64",
                asset: "metria-windows-amd64.exe",
            })
        }
        other => Err(format!("不支持的 Agent 平台: {other}")),
    }
}

fn current_agent_target() -> AgentTarget {
    normalize_agent_target(Some(std::env::consts::OS), Some(std::env::consts::ARCH))
        .expect("当前构建目标必须是受支持的 Agent 平台")
}

fn target_from_node(node: &serde_json::Value) -> Result<AgentTarget, String> {
    normalize_agent_target(
        node.get("platform").and_then(|v| v.as_str()),
        node.get("architecture").and_then(|v| v.as_str()),
    )
}

/// 校验并规范化 Agent 地址（Hub 主动拉取目标）：host[:port]，默认使用 HTTP。
fn normalize_agent_url(raw: &str) -> Option<String> {
    let value = raw.trim().trim_end_matches('/');
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let candidate = if value.contains("://") {
        value.to_string()
    } else if let Ok(IpAddr::V6(_)) = value.parse::<IpAddr>() {
        format!("http://[{value}]")
    } else {
        format!("http://{value}")
    };
    let parsed = url::Url::parse(&candidate).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    Some(candidate)
}

fn normalize_hub_url(raw: &str) -> Option<String> {
    let value = raw.trim().trim_end_matches('/');
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let parsed = url::Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    Some(value.to_string())
}

fn hub_url_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if let Some(origin) = normalize_hub_url(origin) {
            return Some(origin);
        }
    }
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|v| v.to_str().ok())?;
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| matches!(*v, "http" | "https"))
        .unwrap_or("http");
    normalize_hub_url(&format!("{scheme}://{host}"))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 创建节点请求。
#[derive(Deserialize)]
pub(crate) struct NodeCreateRequest {
    pub name: String,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub hub_url: Option<String>,
    #[serde(default)]
    pub agent_url: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
    /// 上报间隔（秒）：无状态轮询 Agent 的采集周期，缺省 60。
    #[serde(default)]
    pub poll_interval_seconds: Option<i64>,
}

/// 更新节点请求。
#[derive(Deserialize)]
pub(crate) struct NodeUpdateRequest {
    pub name: String,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub hub_url: Option<String>,
    #[serde(default)]
    pub agent_url: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
    /// 上报间隔（秒）：None 表示不修改。
    #[serde(default)]
    pub poll_interval_seconds: Option<i64>,
}

/// 上报间隔合法范围：5 秒 ~ 24 小时。
fn valid_poll_interval(v: i64) -> bool {
    (5..=86_400).contains(&v)
}

/// 校验节点 IP：合法 IPv4/IPv6（含端口）、或常见主机名（字母数字、点、横线）。
fn valid_node_ip(raw: &str) -> bool {
    let value = raw.trim();
    if value.is_empty() {
        return false;
    }
    // 允许显式端口（如 203.0.113.10:8080），取地址部分校验。
    let host = value
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty())
        .map(|(h, _)| h)
        .unwrap_or(value);
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    // 主机名：各段非空、长度 ≤63、字符集为字母数字-，总长 ≤253。
    if host.len() > 253
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// 创建节点：预建 collector 与专属 token，返回一次性明文 token。
pub(crate) async fn node_create(
    State(st): State<AppState>,
    Json(req): Json<NodeCreateRequest>,
) -> Response {
    let name = req.name.trim();
    if name.is_empty() {
        return json_err(StatusCode::BAD_REQUEST, "bad_name", "节点名称不能为空");
    }
    if name.len() > 200 {
        return json_err(StatusCode::BAD_REQUEST, "bad_name", "节点名称过长");
    }
    let ip = req.ip.as_deref().map(str::trim).unwrap_or("");
    if ip.is_empty() {
        return json_err(StatusCode::BAD_REQUEST, "bad_ip", "节点 IP 不能为空");
    }
    if !valid_node_ip(ip) {
        return json_err(
            StatusCode::BAD_REQUEST,
            "bad_ip",
            "节点 IP 必须为合法 IPv4/IPv6 或主机名",
        );
    }
    if req.labels.len() > 50 {
        return json_err(StatusCode::BAD_REQUEST, "bad_labels", "labels 数量过多");
    }
    let hub_url = match req.hub_url.as_deref() {
        Some(value) if !value.trim().is_empty() => match normalize_hub_url(value) {
            Some(value) => Some(value),
            None => {
                return json_err(
                    StatusCode::BAD_REQUEST,
                    "bad_hub_url",
                    "Hub 地址必须是合法的 http/https 地址",
                )
            }
        },
        _ => None,
    };
    let agent_url = match req.agent_url.as_deref() {
        Some(value) if !value.trim().is_empty() => match normalize_agent_url(value) {
            Some(value) => Some(value),
            None => {
                return json_err(
                    StatusCode::BAD_REQUEST,
                    "bad_agent_url",
                    "Agent 地址必须是合法的 IP/域名[:端口]",
                )
            }
        },
        _ => None,
    };
    let target = match normalize_agent_target(req.platform.as_deref(), req.architecture.as_deref())
    {
        Ok(target) => target,
        Err(message) => return json_err(StatusCode::BAD_REQUEST, "bad_agent_target", &message),
    };
    if let Some(v) = req.poll_interval_seconds {
        if !valid_poll_interval(v) {
            return json_err(
                StatusCode::BAD_REQUEST,
                "bad_poll_interval",
                "上报间隔必须为 5~86400 秒",
            );
        }
    }
    let now = Utc::now();
    match st.db.create_node(
        name,
        req.description.as_deref(),
        req.labels,
        Some(ip),
        hub_url.as_deref(),
        target.platform,
        target.architecture,
        req.poll_interval_seconds,
        now,
    ) {
        Ok((node_id, collector_id, token)) => {
            // pull 模式：保存 Agent 地址与节点级加密 token（Hub 调度时解密使用）
            if agent_url.is_some() {
                let token_enc = crate::crypto::encrypt_node_token(&token).ok();
                let _ = st.db.set_node_agent_config(
                    &node_id,
                    agent_url.as_deref(),
                    token_enc.as_deref(),
                    now,
                );
            }
            Json(serde_json::json!({
                "ok": true,
                "node_id": node_id,
                "collector_id": collector_id,
                "name": name,
                "token": token,
                "ip": ip,
                "hub_url": hub_url,
                "agent_url": agent_url,
                "platform": target.platform,
                "architecture": target.architecture,
            }))
            .into_response()
        }
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "node_create_failed",
            &e.to_string(),
        ),
    }
}

/// 编辑节点信息。
pub(crate) async fn node_update(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<NodeUpdateRequest>,
) -> Response {
    let name = req.name.trim();
    if name.is_empty() {
        return json_err(StatusCode::BAD_REQUEST, "bad_name", "节点名称不能为空");
    }
    let ip = req.ip.as_deref().map(str::trim).unwrap_or("");
    if ip.is_empty() {
        return json_err(StatusCode::BAD_REQUEST, "bad_ip", "节点 IP 不能为空");
    }
    if !valid_node_ip(ip) {
        return json_err(
            StatusCode::BAD_REQUEST,
            "bad_ip",
            "节点 IP 必须为合法 IPv4/IPv6 或主机名",
        );
    }
    let hub_url = match req.hub_url.as_deref() {
        Some(value) if !value.trim().is_empty() => match normalize_hub_url(value) {
            Some(value) => Some(value),
            None => {
                return json_err(
                    StatusCode::BAD_REQUEST,
                    "bad_hub_url",
                    "Hub 地址必须是合法的 http/https 地址",
                )
            }
        },
        _ => None,
    };
    let agent_url = match req.agent_url.as_deref() {
        Some(value) if !value.trim().is_empty() => match normalize_agent_url(value) {
            Some(value) => Some(value),
            None => {
                return json_err(
                    StatusCode::BAD_REQUEST,
                    "bad_agent_url",
                    "Agent 地址必须是合法的 IP/域名[:端口]",
                )
            }
        },
        // 显式传空串表示清除
        Some(_) => Some(String::new()),
        None => None,
    };
    let target = if req.platform.is_some() || req.architecture.is_some() {
        match normalize_agent_target(req.platform.as_deref(), req.architecture.as_deref()) {
            Ok(target) => Some(target),
            Err(message) => return json_err(StatusCode::BAD_REQUEST, "bad_agent_target", &message),
        }
    } else {
        None
    };
    if let Some(v) = req.poll_interval_seconds {
        if !valid_poll_interval(v) {
            return json_err(
                StatusCode::BAD_REQUEST,
                "bad_poll_interval",
                "上报间隔必须为 5~86400 秒",
            );
        }
    };
    match st.db.update_node(
        &id,
        name,
        req.description.as_deref(),
        req.labels,
        Some(ip),
        hub_url.as_deref(),
        target.map(|value| value.platform),
        target.map(|value| value.architecture),
        req.poll_interval_seconds,
        Utc::now(),
    ) {
        Ok(true) => {
            if let Some(value) = &agent_url {
                let _ = st.db.set_node_agent_config(
                    &id,
                    if value.is_empty() { None } else { Some(value) },
                    None,
                    Utc::now(),
                );
            }
            Json(serde_json::json!({ "ok": true })).into_response()
        }
        Ok(false) => json_err(StatusCode::NOT_FOUND, "not_found", "节点不存在"),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "node_update_failed",
            &e.to_string(),
        ),
    }
}

/// 删除节点（保留业务数据，仅清身份关联）。
pub(crate) async fn node_delete(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match st.db.delete_node(&id) {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => json_err(StatusCode::NOT_FOUND, "not_found", "节点不存在"),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "node_delete_failed",
            &e.to_string(),
        ),
    }
}

/// 生成 Docker 安装命令模板（含 node_id / hub_url / token / 上报间隔 / 客户端只读挂载）。
fn docker_install_command(
    node_id: &str,
    hub_url: &str,
    token: &str,
    poll_interval: u64,
    target: AgentTarget,
) -> String {
    if target.platform == "windows" {
        return "Windows Agent 请使用“原生安装”PowerShell 命令；当前 Docker Agent 镜像仅支持 Linux。".into();
    }
    format!(
        "docker run -d --name metria-agent --restart unless-stopped \\\n  -e METRIA_NODE_ID={} \\\n  -e METRIA_HUB_URL={} \\\n  -e METRIA_AGENT_TOKEN={} \\\n  -e METRIA_POLL_INTERVAL={} \\\n  -e METRIA_CLAUDE_PATH=/sources/claude \\\n  -e METRIA_CODEX_PATH=/sources/codex \\\n  -e METRIA_OPENCODE_PATH=/sources/opencode \\\n  -v $HOME/.claude:/sources/claude:ro \\\n  -v $HOME/.codex:/sources/codex:ro \\\n  -v $HOME/.local/share/opencode:/sources/opencode:ro \\\n  -v metria-agent-data:/data \\\n  ghcr.io/supercxyz/metria:latest agent",
        shell_quote(node_id),
        shell_quote(hub_url),
        shell_quote(token),
        poll_interval
    )
}

/// Pull 模式 Docker 安装命令：仅 token + 只读挂载（Hub 主动拉取，无需 Hub 地址与 Node ID）。
fn docker_install_command_pull(token: &str, listen_port: u16, target: AgentTarget) -> String {
    if target.platform == "windows" {
        return "Windows Agent 请使用“原生安装”PowerShell 命令；当前 Docker Agent 镜像仅支持 Linux。".into();
    }
    format!(
        "docker run -d --name metria-agent --restart unless-stopped \\\n  -e METRIA_AGENT_TOKEN={} \\\n  -e METRIA_LISTEN_PORT={} \\\n  -e METRIA_CLAUDE_PATH=/sources/claude \\\n  -e METRIA_CODEX_PATH=/sources/codex \\\n  -e METRIA_OPENCODE_PATH=/sources/opencode \\\n  -v $HOME/.claude:/sources/claude:ro \\\n  -v $HOME/.codex:/sources/codex:ro \\\n  -v $HOME/.local/share/opencode:/sources/opencode:ro \\\n  -v metria-agent-data:/data \\\n  -p {listen_port}:{listen_port} \\\n  ghcr.io/supercxyz/metria:latest agent\n\n# Hub 将主动访问 Agent 地址 http://<本机>:{listen_port}，请确保该端口可达",
        shell_quote(token),
        listen_port
    )
}

fn linux_env_arg(name: &str, value: &str) -> String {
    shell_quote(&format!("{name}={}", shell_quote(value)))
}

fn powershell_config_line(name: &str, value: &str) -> String {
    powershell_quote(&format!("$env:{name} = {}", powershell_quote(value)))
}

fn powershell_native_install_command(
    download_url: &str,
    config_lines: &[String],
    summary: &str,
) -> String {
    let config_lines = config_lines
        .iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "$ErrorActionPreference = 'Stop'\n\n# 1) 下载 Windows Agent（公开下载，无需 Token）\n$installDir = Join-Path $env:LOCALAPPDATA 'Metria'\nNew-Item -ItemType Directory -Force -Path $installDir | Out-Null\n$agentPath = Join-Path $installDir 'metria.exe'\n$configPath = Join-Path $installDir 'agent.ps1'\n$dataDir = Join-Path $installDir 'data'\nNew-Item -ItemType Directory -Force -Path $dataDir | Out-Null\nInvoke-WebRequest -UseBasicParsing -Uri {} -OutFile $agentPath\n\n# 2) 写入持久化配置与启动脚本\n$configLines = @(\n{}\n)\nSet-Content -Path $configPath -Value $configLines -Encoding UTF8\n\n# 3) 注册并立即启动登录自启动任务\n$taskAction = New-ScheduledTaskAction -Execute 'PowerShell.exe' -Argument ('-NoProfile -ExecutionPolicy Bypass -File \"{{0}}\"' -f $configPath) -WorkingDirectory $installDir\n$taskTrigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME\n$taskPrincipal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType InteractiveToken -RunLevel Limited\n$taskSettings = New-ScheduledTaskSettingsSet -StartWhenAvailable\nRegister-ScheduledTask -TaskName 'Metria Agent' -Action $taskAction -Trigger $taskTrigger -Principal $taskPrincipal -Settings $taskSettings -Description 'Metria Agent collector' -Force | Out-Null\nStart-ScheduledTask -TaskName 'Metria Agent'\n\n# {}\n# 配置文件：$configPath；任务名：Metria Agent",
        powershell_quote(download_url),
        config_lines,
        summary
    )
}

fn linux_native_install_command(env_lines: &[String], download_url: &str, summary: &str) -> String {
    let env_lines = env_lines.join(" \\\n  ");
    format!(
        "# 1) 下载 Linux Agent（公开下载，无需 Token）\ncurl -fsSL {} -o metria && chmod +x metria\n\n# 2) 安装固定路径的 Agent 与持久化数据目录\nsudo install -m 0755 metria /usr/local/bin/metria\nmkdir -p \"$HOME/.local/share/metria\"\n\n# 3) 写入 root-only 运行时配置\nsudo install -d -m 0750 /etc/metria\nsudo install -m 0600 /dev/null /etc/metria/metria-agent.env\nprintf '%s\\n' \\\n  {} | sudo tee /etc/metria/metria-agent.env >/dev/null\n\n# 4) 注册并立即启动 systemd 服务\nsudo tee /etc/systemd/system/metria-agent.service >/dev/null <<EOF\n[Unit]\nDescription=Metria Agent collector\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser=$USER\nWorkingDirectory=$HOME\nEnvironmentFile=/etc/metria/metria-agent.env\nExecStart=/usr/local/bin/metria agent\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=multi-user.target\nEOF\nsudo systemctl daemon-reload\nsudo systemctl enable --now metria-agent.service\n\n# {}\n# 配置文件：/etc/metria/metria-agent.env；服务：metria-agent.service",
        shell_quote(download_url),
        env_lines,
        summary
    )
}

/// Pull 模式原生命令：下载二进制后配置 token 与客户端路径，并持久化启动。
fn native_install_command_pull(
    node_id: &str,
    hub_url: &str,
    token: &str,
    listen_port: u16,
    target: AgentTarget,
) -> String {
    let download_url = format!("{hub_url}/api/v1/nodes/{node_id}/agent/download");
    if target.platform == "windows" {
        let config_lines = vec![
            powershell_config_line("METRIA_AGENT_TOKEN", token),
            powershell_config_line("METRIA_LISTEN_PORT", &listen_port.to_string()),
            powershell_quote("$env:METRIA_CLAUDE_PATH = Join-Path $env:USERPROFILE '.claude'"),
            powershell_quote("$env:METRIA_CODEX_PATH = Join-Path $env:USERPROFILE '.codex'"),
            powershell_quote("$env:METRIA_OPENCODE_PATH = Join-Path $env:LOCALAPPDATA 'opencode'"),
            powershell_quote("$env:METRIA_DATA_DIR = Join-Path $PSScriptRoot 'data'"),
            powershell_quote("& (Join-Path $PSScriptRoot 'metria.exe') agent"),
        ];
        return powershell_native_install_command(
            &download_url,
            &config_lines,
            &format!("配置并启动 Agent（pull 模式：Hub 主动拉取，监听端口 {listen_port}）"),
        );
    }
    let env_lines = vec![
        linux_env_arg("METRIA_AGENT_TOKEN", token),
        format!("\"METRIA_LISTEN_PORT={listen_port}\""),
        "\"METRIA_CLAUDE_PATH=$HOME/.claude\"".into(),
        "\"METRIA_CODEX_PATH=$HOME/.codex\"".into(),
        "\"METRIA_OPENCODE_PATH=$HOME/.local/share/opencode\"".into(),
        "\"METRIA_DATA_DIR=$HOME/.local/share/metria\"".into(),
    ];
    linux_native_install_command(
        &env_lines,
        &download_url,
        &format!("配置并启动 Agent（pull 模式：Hub 主动拉取，监听端口 {listen_port}）"),
    )
}

/// 生成原生安装命令模板：公开下载对应节点平台/架构的二进制，并持久化运行配置。
fn native_install_command(
    node_id: &str,
    hub_url: &str,
    token: &str,
    poll_interval: u64,
    target: AgentTarget,
) -> String {
    let download_url = format!("{hub_url}/api/v1/nodes/{node_id}/agent/download");
    let poll = poll_interval.to_string();
    if target.platform == "windows" {
        let config_lines = vec![
            powershell_config_line("METRIA_NODE_ID", node_id),
            powershell_config_line("METRIA_HUB_URL", hub_url),
            powershell_config_line("METRIA_AGENT_TOKEN", token),
            powershell_config_line("METRIA_POLL_INTERVAL", &poll),
            powershell_quote("$env:METRIA_CLAUDE_PATH = Join-Path $env:USERPROFILE '.claude'"),
            powershell_quote("$env:METRIA_CODEX_PATH = Join-Path $env:USERPROFILE '.codex'"),
            powershell_quote("$env:METRIA_OPENCODE_PATH = Join-Path $env:LOCALAPPDATA 'opencode'"),
            powershell_quote("$env:METRIA_DATA_DIR = Join-Path $PSScriptRoot 'data'"),
            powershell_quote("& (Join-Path $PSScriptRoot 'metria.exe') agent"),
        ];
        return powershell_native_install_command(
            &download_url,
            &config_lines,
            "配置并启动 Agent（push 模式）",
        );
    }
    let env_lines = vec![
        linux_env_arg("METRIA_NODE_ID", node_id),
        linux_env_arg("METRIA_HUB_URL", hub_url),
        linux_env_arg("METRIA_AGENT_TOKEN", token),
        linux_env_arg("METRIA_POLL_INTERVAL", &poll),
        "\"METRIA_CLAUDE_PATH=$HOME/.claude\"".into(),
        "\"METRIA_CODEX_PATH=$HOME/.codex\"".into(),
        "\"METRIA_OPENCODE_PATH=$HOME/.local/share/opencode\"".into(),
        "\"METRIA_DATA_DIR=$HOME/.local/share/metria\"".into(),
    ];
    linux_native_install_command(&env_lines, &download_url, "配置并启动 Agent（push 模式）")
}

/// 获取安装命令：为新签发 token 生成 Docker 与原生命令。
pub(crate) async fn node_install(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<NodeInstallQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(node) = st.db.get_node(&id) else {
        return json_err(StatusCode::NOT_FOUND, "not_found", "节点不存在");
    };
    let Some(collector_id) = st.db.get_node_collector_id(&id) else {
        return json_err(StatusCode::NOT_FOUND, "not_found", "节点不存在");
    };
    let token = match st.db.issue_collector_token(&collector_id, Utc::now()) {
        Ok(t) => t,
        Err(e) => {
            return json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "token_issue_failed",
                &e.to_string(),
            );
        }
    };
    // pull 模式：同步更新节点级加密 token（Hub 调度时解密向 Agent 认证）
    let agent_url = node
        .get("agent_url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if agent_url.is_some() {
        if let Ok(enc) = crate::crypto::encrypt_node_token(&token) {
            let _ = st.db.set_node_token_enc(&id, &enc);
        }
    }
    let target = match target_from_node(&node) {
        Ok(target) => target,
        Err(message) => return json_err(StatusCode::CONFLICT, "bad_agent_target", &message),
    };
    // 前端传入当前环境 origin 优先，避免节点历史配置或占位 host 生成不可用命令。
    let hub_url = if let Some(value) = query.hub_url.as_deref() {
        match normalize_hub_url(value) {
            Some(value) => Some(value),
            None => {
                return json_err(
                    StatusCode::BAD_REQUEST,
                    "bad_hub_url",
                    "Hub 地址必须是合法的 http/https 地址",
                )
            }
        }
    } else {
        node.get("hub_url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .and_then(normalize_hub_url)
            .or_else(|| {
                node.get("ip")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|ip| format!("http://{ip}:8080"))
            })
            .or_else(|| hub_url_from_headers(&headers))
    };
    // pull 模式：hub_url 仅用于二进制下载地址（取当前页面 origin），命令本身不含 METRIA_HUB_URL
    let download_base = hub_url
        .clone()
        .or_else(|| hub_url_from_headers(&headers))
        .unwrap_or_else(|| "http://localhost:8080".into());
    if agent_url.is_some() {
        let listen_port = std::env::var("METRIA_AGENT_LISTEN_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8090);
        return Json(serde_json::json!({
            "node_id": id,
            "name": node.get("name"),
            "hub_url": download_base,
            "agent_url": agent_url,
            "mode": "pull",
            "platform": target.platform,
            "architecture": target.architecture,
            "agent_asset": target.asset,
            "token": token,
            "docker_command": docker_install_command_pull(&token, listen_port, target),
            "native_command": native_install_command_pull(&id, &download_base, &token, listen_port, target),
        }))
        .into_response();
    }
    let Some(hub_url) = hub_url else {
        return json_err(
            StatusCode::CONFLICT,
            "hub_url_unavailable",
            "无法确定 Hub 地址，请从当前 Hub 页面重新生成安装命令",
        );
    };
    // 上报间隔：节点记录值，缺省 60（无状态轮询 Agent 采集周期）
    let poll_interval = node
        .get("poll_interval_seconds")
        .and_then(|v| v.as_i64())
        .filter(|v| *v > 0)
        .unwrap_or(60) as u64;
    Json(serde_json::json!({
        "node_id": id,
        "name": node.get("name"),
        "hub_url": hub_url,
        "mode": "push",
        "platform": target.platform,
        "architecture": target.architecture,
        "agent_asset": target.asset,
        "token": token,
        "docker_command": docker_install_command(&id, &hub_url, &token, poll_interval, target),
        "native_command": native_install_command(&id, &hub_url, &token, poll_interval, target),
    }))
    .into_response()
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct NodeInstallQuery {
    pub hub_url: Option<String>,
}

fn binary_response(bytes: Vec<u8>, filename: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
        .header(
            axum::http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(axum::body::Body::from(bytes))
        .unwrap_or_else(|e| {
            json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "download_failed",
                &e.to_string(),
            )
        })
}

fn binary_from_configured_dir(asset: &str) -> Option<Vec<u8>> {
    let dir = std::env::var("METRIA_AGENT_BINARIES_DIR").ok()?;
    std::fs::read(std::path::Path::new(&dir).join(asset)).ok()
}

/// 下载 Hub 当前架构二进制（公开接口，兼容旧命令）。
pub(crate) async fn agent_download(State(_st): State<AppState>) -> Response {
    let target = current_agent_target();
    match std::env::current_exe() {
        Ok(path) => match std::fs::read(&path) {
            Ok(bytes) => binary_response(bytes, target.asset),
            Err(e) => json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "download_failed",
                &e.to_string(),
            ),
        },
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "download_failed",
            &e.to_string(),
        ),
    }
}

/// 根据节点登记的平台/架构下载对应 Agent 二进制；接口公开但只读节点元数据。
pub(crate) async fn agent_download_for_node(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let Some(node) = st.db.get_node(&id) else {
        return json_err(StatusCode::NOT_FOUND, "not_found", "节点不存在");
    };
    let target = match target_from_node(&node) {
        Ok(target) => target,
        Err(message) => return json_err(StatusCode::CONFLICT, "bad_agent_target", &message),
    };
    let current = current_agent_target();
    if target == current {
        return match std::env::current_exe() {
            Ok(path) => match std::fs::read(path) {
                Ok(bytes) => binary_response(bytes, target.asset),
                Err(e) => json_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "download_failed",
                    &e.to_string(),
                ),
            },
            Err(e) => json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "download_failed",
                &e.to_string(),
            ),
        };
    }
    if let Some(bytes) = binary_from_configured_dir(target.asset) {
        return binary_response(bytes, target.asset);
    }
    if let Ok(base) = std::env::var("METRIA_AGENT_DOWNLOAD_BASE_URL") {
        let base = base.trim_end_matches('/');
        if !base.is_empty() {
            return Redirect::temporary(&format!("{base}/{}", target.asset)).into_response();
        }
    }
    json_err(
        StatusCode::NOT_IMPLEMENTED,
        "agent_binary_unavailable",
        &format!(
            "当前 Hub 未配置目标架构二进制 `{}`；请设置 METRIA_AGENT_BINARIES_DIR 或 METRIA_AGENT_DOWNLOAD_BASE_URL",
            target.asset
        ),
    )
}

#[cfg(test)]
mod pricing_rule_validation_tests {
    use super::{
        docker_install_command, docker_install_command_pull, native_install_command,
        native_install_command_pull, normalize_agent_target, normalize_agent_url,
        validate_pricing_rule,
    };
    use serde_json::json;

    #[test]
    fn accepts_zero_price_rule() {
        let result = validate_pricing_rule(
            &json!({"model_pattern": "mimo-v2.5", "input_price": "0"}),
            true,
            true,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_negative_price_rule() {
        let result = validate_pricing_rule(
            &json!({"model_pattern": "mimo-v2.5", "input_price": "-1"}),
            true,
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn update_can_change_only_enabled_state() {
        let result = validate_pricing_rule(&json!({"enabled": false}), false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn normalizes_supported_agent_targets() {
        assert_eq!(
            normalize_agent_target(Some("linux"), Some("x86_64"))
                .unwrap()
                .asset,
            "metria-linux-amd64"
        );
        assert_eq!(
            normalize_agent_target(Some("linux"), Some("aarch64"))
                .unwrap()
                .asset,
            "metria-linux-arm64"
        );
        assert_eq!(
            normalize_agent_target(Some("windows"), Some("amd64"))
                .unwrap()
                .asset,
            "metria-windows-amd64.exe"
        );
        assert!(normalize_agent_target(Some("windows"), Some("arm64")).is_err());
    }

    #[test]
    fn normalizes_agent_addresses_without_scheme() {
        assert_eq!(
            normalize_agent_url("203.0.113.10:8090").as_deref(),
            Some("http://203.0.113.10:8090")
        );
        assert_eq!(
            normalize_agent_url("agent.example.com").as_deref(),
            Some("http://agent.example.com")
        );
        assert_eq!(
            normalize_agent_url("https://agent.example.com:8443/").as_deref(),
            Some("https://agent.example.com:8443")
        );
        assert_eq!(
            normalize_agent_url("[2001:db8::1]:8090").as_deref(),
            Some("http://[2001:db8::1]:8090")
        );
        assert!(normalize_agent_url("agent.example.com/path").is_none());
    }

    #[test]
    fn install_commands_keep_download_public_and_runtime_token_private() {
        let target = normalize_agent_target(Some("linux"), Some("amd64")).unwrap();
        let native = native_install_command(
            "node-test",
            "https://hub.example",
            "mct-runtime-token",
            60,
            target,
        );
        assert!(native.contains("/api/v1/nodes/node-test/agent/download"));
        assert!(!native.contains("Authorization"));
        assert!(native.contains("METRIA_AGENT_TOKEN"));
        assert!(native.contains("METRIA_POLL_INTERVAL"));
        assert!(native.contains("EnvironmentFile=/etc/metria/metria-agent.env"));
        assert!(native.contains("systemctl enable --now metria-agent.service"));
        assert!(!native.contains("nohup"));

        let pull = native_install_command_pull(
            "node-pull",
            "https://hub.example",
            "mct-pull-token",
            8090,
            target,
        );
        assert!(pull.contains("METRIA_LISTEN_PORT"));
        assert!(pull.contains("systemctl enable --now metria-agent.service"));
        assert!(!pull.contains("nohup"));

        let windows = normalize_agent_target(Some("windows"), Some("amd64")).unwrap();
        let powershell = native_install_command(
            "node-win",
            "https://hub.example",
            "mct-runtime-token",
            120,
            windows,
        );
        assert!(powershell.contains("Invoke-WebRequest"));
        assert!(powershell.contains("metria.exe"));
        assert!(powershell.contains("Register-ScheduledTask"));
        assert!(powershell.contains("Set-Content"));
        assert!(powershell_contains_poll(&powershell, 120));
        assert!(!powershell.contains("Start-Process"));
        assert!(!powershell.contains("Authorization"));
        let docker =
            docker_install_command("node-test", "https://hub.example", "token", 60, target);
        assert!(docker.contains("\\\n"));
        assert!(docker.contains("METRIA_POLL_INTERVAL=60"));
        assert!(!docker.ends_with("\\"));
        let pull_docker = docker_install_command_pull("token", 8090, target);
        assert!(pull_docker.contains("\\\n"));
        assert!(!pull_docker.ends_with("\\"));
        assert!(
            docker_install_command("node-win", "https://hub.example", "token", 60, windows)
                .contains("PowerShell")
        );
    }

    fn powershell_contains_poll(cmd: &str, interval: u64) -> bool {
        cmd.contains("$env:METRIA_POLL_INTERVAL") && cmd.contains(&interval.to_string())
    }
}
