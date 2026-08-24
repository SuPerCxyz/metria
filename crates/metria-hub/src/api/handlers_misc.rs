//! 补充 handlers：Traffic Profiles / Pricing / Share / Export。

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
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
    let now = Utc::now();
    match st.db.create_node(
        name,
        req.description.as_deref(),
        req.labels,
        Some(ip),
        req.hub_url.as_deref(),
        now,
    ) {
        Ok((node_id, collector_id, token)) => Json(serde_json::json!({
            "ok": true,
            "node_id": node_id,
            "collector_id": collector_id,
            "name": name,
            "token": token,
            "ip": ip,
            "hub_url": req.hub_url,
        }))
        .into_response(),
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
    match st.db.update_node(
        &id,
        name,
        req.description.as_deref(),
        req.labels,
        Some(ip),
        req.hub_url.as_deref(),
        Utc::now(),
    ) {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
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

/// 生成 Docker 安装命令模板（含 node_id / hub_url / token / 客户端只读挂载）。
fn docker_install_command(node_id: &str, hub_url: &str, token: &str) -> String {
    format!(
        "docker run -d --name metria-agent --restart unless-stopped\n  -e METRIA_NODE_ID={node_id}\n  -e METRIA_HUB_URL={hub_url}\n  -e METRIA_AGENT_TOKEN={token}\n  -e METRIA_CLAUDE_PATH=/sources/claude\n  -e METRIA_CODEX_PATH=/sources/codex\n  -e METRIA_OPENCODE_PATH=/sources/opencode\n  -v $HOME/.claude:/sources/claude:ro\n  -v $HOME/.codex:/sources/codex:ro\n  -v $HOME/.local/share/opencode:/sources/opencode:ro\n  -v metria-agent-data:/data\n  ghcr.io/supercxyz/metria:latest agent"
    )
}

/// 生成原生安装命令模板：下载 Hub 自供二进制 + 后台运行。
fn native_install_command(node_id: &str, hub_url: &str, token: &str) -> String {
    format!(
        "# 1) 下载 metria 二进制（需 Admin 会话；或直接 docker cp 自镜像提取）\ncurl -fsSL {hub_url}/api/v1/agent/download -o metria && chmod +x metria\n\n# 2) 后台运行 Agent\nMETRIA_NODE_ID={node_id} \\\n  METRIA_HUB_URL={hub_url} \\\n  METRIA_AGENT_TOKEN={token} \\\n  METRIA_CLAUDE_PATH=$HOME/.claude \\\n  METRIA_CODEX_PATH=$HOME/.codex \\\n  METRIA_OPENCODE_PATH=$HOME/.local/share/opencode \\\n  nohup ./metria agent >> metria-agent.log 2>&1 &\n\n# 生产建议：使用 systemd 服务管理（见 docs/deployment.md）"
    )
}

/// 获取安装命令：为新签发 token 生成 Docker 与原生命令。
pub(crate) async fn node_install(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
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
    // hub_url：优先使用创建/编辑节点时持久化的 hub_url；否则用节点 ip 拼接
    // （支持 Hub 在内网、公网 Agent 回连场景）；两者皆缺省用占位提示。
    let hub_url = node
        .get("hub_url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            node.get("ip")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|ip| format!("http://{ip}:8080"))
        })
        .unwrap_or_else(|| "http://<hub-host>:8080".to_string());
    Json(serde_json::json!({
        "node_id": id,
        "name": node.get("name"),
        "hub_url": hub_url,
        "token": token,
        "docker_command": docker_install_command(&id, &hub_url, &token),
        "native_command": native_install_command(&id, &hub_url, &token),
    }))
    .into_response()
}

/// 下载 Hub 自身二进制（metria 单二进制，含 agent 子命令）。
pub(crate) async fn agent_download(State(_st): State<AppState>) -> Response {
    match std::env::current_exe() {
        Ok(path) => match std::fs::read(&path) {
            Ok(bytes) => {
                let body = axum::body::Body::from(bytes);
                Response::builder()
                    .status(StatusCode::OK)
                    .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
                    .header(
                        axum::http::header::CONTENT_DISPOSITION,
                        "attachment; filename=\"metria\"",
                    )
                    .body(body)
                    .unwrap_or_else(|e| {
                        json_err(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "download_failed",
                            &e.to_string(),
                        )
                    })
            }
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

#[cfg(test)]
mod pricing_rule_validation_tests {
    use super::validate_pricing_rule;
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
}
