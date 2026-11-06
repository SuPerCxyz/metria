//! OIDC 单用户登录（Authorization Code Flow）。
//!
//! 设计约束：项目仅支持单一用户，通过白名单 email / subject 匹配；
//! 身份校验使用 IdP userinfo 端点（Hub↔issuer 必须走 HTTPS），避免本地 JWKS 验签实现。
//! 会话复用既有 HMAC 签名 token 机制；回调向前端传递一次性短时交换码，
//! 避免 session token 进入 URL / 浏览器历史。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use super::{json_err, AppState};
use crate::config::OidcConfig;

/// 授权 state 有效期。
const PENDING_TTL: Duration = Duration::from_secs(10 * 60);
/// 一次性交换码有效期。
const CODE_TTL: Duration = Duration::from_secs(60);
/// discovery / token / userinfo 请求超时。
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// discovery 默认缓存时间。
const DISCOVERY_DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);
/// 尊重 Cache-Control 时的最小/最大缓存时间。
const DISCOVERY_MIN_TTL: Duration = Duration::from_secs(60);
const DISCOVERY_MAX_TTL: Duration = Duration::from_secs(60 * 60);
/// 刷新失败时允许使用 stale discovery 的最长时间。
const DISCOVERY_STALE_MAX: Duration = Duration::from_secs(24 * 60 * 60);

/// OIDC 运行时状态：待验证授权请求与一次性交换码。
#[derive(Debug, Default)]
pub struct OidcRuntime {
    pending: Mutex<HashMap<String, Pending>>,
    codes: Mutex<HashMap<String, CodeGrant>>,
    discovery: DiscoveryCache,
}

#[derive(Debug, Clone)]
struct CachedDiscovery {
    discovery: Discovery,
    fetched_at: Instant,
    ttl: Duration,
    issuer: String,
}

#[derive(Debug, Default)]
struct DiscoveryCache {
    entry: Mutex<Option<CachedDiscovery>>,
    fetch_lock: Mutex<()>,
}

impl DiscoveryCache {
    fn fresh(&self, issuer: &str) -> Option<Discovery> {
        let entry = self.entry.lock().ok()?;
        let cached = entry.as_ref()?;
        (cached.issuer == issuer && cached.fetched_at.elapsed() < cached.ttl)
            .then(|| cached.discovery.clone())
    }

    /// 缓存命中直接返回；过期时单飞刷新，失败最多回退 24h 的 stale 值。
    fn get_or_fetch<F>(&self, cfg: &OidcConfig, fetch: F) -> Result<Discovery, String>
    where
        F: FnOnce() -> Result<(Discovery, Duration), String>,
    {
        let issuer = cfg.issuer.trim_end_matches('/').to_string();
        if let Some(discovery) = self.fresh(&issuer) {
            return Ok(discovery);
        }
        let _guard = self
            .fetch_lock
            .lock()
            .map_err(|_| "OIDC discovery 缓存锁不可用".to_string())?;
        // 等待期间其他请求可能已完成刷新。
        if let Some(discovery) = self.fresh(&issuer) {
            return Ok(discovery);
        }
        match fetch() {
            Ok((discovery, ttl)) => {
                let ttl = ttl.clamp(DISCOVERY_MIN_TTL, DISCOVERY_MAX_TTL);
                if let Ok(mut entry) = self.entry.lock() {
                    *entry = Some(CachedDiscovery {
                        discovery: discovery.clone(),
                        fetched_at: Instant::now(),
                        ttl,
                        issuer,
                    });
                }
                Ok(discovery)
            }
            Err(error) => {
                let stale = self.entry.lock().ok().and_then(|entry| {
                    entry.as_ref().and_then(|cached| {
                        (cached.issuer == issuer
                            && cached.fetched_at.elapsed() <= DISCOVERY_STALE_MAX)
                            .then(|| cached.discovery.clone())
                    })
                });
                if let Some(discovery) = stale {
                    tracing::warn!(%error, "OIDC discovery 刷新失败，使用最近一次有效缓存");
                    Ok(discovery)
                } else {
                    Err(error)
                }
            }
        }
    }
}

impl OidcRuntime {
    fn discovery(&self, cfg: &OidcConfig) -> Result<Discovery, String> {
        self.discovery.get_or_fetch(cfg, || fetch_discovery(cfg))
    }
}

#[derive(Debug)]
struct Pending {
    created: Instant,
}

#[derive(Debug)]
struct CodeGrant {
    username: String,
    created: Instant,
}

impl OidcRuntime {
    pub fn new() -> Self {
        Self::default()
    }
}

/// 密码登录是否可用（未启用 OIDC 或未显式禁用时可用）。
pub(crate) fn password_login_enabled(cfg: &crate::config::HubConfig) -> bool {
    match &cfg.oidc {
        Some(o) => !o.disable_password_login,
        None => true,
    }
}

// ============ Discovery / Token / Userinfo ============

#[derive(Debug, Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    userinfo_endpoint: Option<String>,
}

fn fetch_discovery(cfg: &OidcConfig) -> Result<(Discovery, Duration), String> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        cfg.issuer.trim_end_matches('/')
    );
    let resp = ureq::get(&url)
        .timeout(HTTP_TIMEOUT)
        .call()
        .map_err(|e| format!("获取 OIDC discovery 失败: {e}"))?;
    let ttl = resp
        .header("cache-control")
        .and_then(cache_control_max_age)
        .map(Duration::from_secs)
        .unwrap_or(DISCOVERY_DEFAULT_TTL);
    let disc: Discovery = resp
        .into_json()
        .map_err(|e| format!("解析 OIDC discovery 失败: {e}"))?;
    // 防混叠：discovery 声明的 issuer 必须与配置一致
    if disc.issuer.trim_end_matches('/') != cfg.issuer.trim_end_matches('/') {
        return Err(format!(
            "OIDC issuer 不匹配：discovery 返回 `{}`，配置为 `{}`",
            disc.issuer, cfg.issuer
        ));
    }
    Ok((disc, ttl))
}

fn cache_control_max_age(value: &str) -> Option<u64> {
    value.split(',').find_map(|part| {
        part.trim()
            .strip_prefix("max-age=")
            .and_then(|v| v.trim().parse::<u64>().ok())
    })
}

fn exchange_code(
    cfg: &OidcConfig,
    token_endpoint: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<String, String> {
    // 优先 client_secret_post；部分 IdP 仅接受 basic auth 时自动降级重试
    let post = |with_secret: bool, basic: bool| -> Result<String, String> {
        let mut form: Vec<(&str, String)> = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", redirect_uri.to_string()),
            ("client_id", cfg.client_id.clone()),
        ];
        if with_secret {
            form.push(("client_secret", cfg.client_secret.clone()));
        }
        let form_ref: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let mut req = ureq::post(token_endpoint).timeout(HTTP_TIMEOUT);
        if basic {
            let cred = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", cfg.client_id, cfg.client_secret));
            req = req.set("Authorization", &format!("Basic {cred}"));
        }
        let resp = req
            .send_form(&form_ref)
            .map_err(|e| format!("OIDC code 兑换失败: {e}"))?;
        let body: serde_json::Value = resp
            .into_json()
            .map_err(|e| format!("解析 OIDC token 响应失败: {e}"))?;
        body.get("access_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "OIDC token 响应缺少 access_token".to_string())
    };
    match post(true, false) {
        Ok(tok) => Ok(tok),
        Err(first) => post(false, true).map_err(|second| {
            format!("OIDC code 兑换失败（client_secret_post 与 basic 均被拒绝）: {first}; {second}")
        }),
    }
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    picture: Option<String>,
}

fn safe_picture_url(picture: Option<&str>) -> Option<String> {
    let url = Url::parse(picture?.trim()).ok()?;
    (url.scheme() == "https" && url.host_str().is_some()).then(|| url.to_string())
}

fn fetch_userinfo(userinfo_endpoint: &str, access_token: &str) -> Result<UserInfo, String> {
    let resp = ureq::get(userinfo_endpoint)
        .timeout(HTTP_TIMEOUT)
        .set("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("OIDC userinfo 请求失败: {e}"))?;
    resp.into_json()
        .map_err(|e| format!("解析 OIDC userinfo 失败: {e}"))
}

/// 单用户白名单匹配：任一已配置标识命中即放行。
///
/// 使用 email 匹配时要求 email_verified ≠ false（防止未验证邮箱冒充）。
fn identity_allowed(cfg: &OidcConfig, ui: &UserInfo) -> bool {
    if let Some(want) = &cfg.allowed_email {
        if let Some(email) = &ui.email {
            if ui.email_verified != Some(false) && email.eq_ignore_ascii_case(want) {
                return true;
            }
        }
    }
    if let Some(want) = &cfg.allowed_subject {
        if ui.sub == *want {
            return true;
        }
    }
    false
}

// ============ 工具 ============

fn random_token() -> String {
    use argon2::password_hash::rand_core::{OsRng, RngCore};
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// 回调 URI：显式配置优先，否则从 Host / X-Forwarded-* 推导。
fn redirect_uri(cfg: &OidcConfig, headers: &HeaderMap) -> String {
    if let Some(u) = &cfg.redirect_url {
        return u.clone();
    }
    let first = |name: &str| -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let host = first("x-forwarded-host")
        .or_else(|| first(header::HOST.as_str()))
        .unwrap_or_else(|| "localhost:8080".to_string());
    let proto = first("x-forwarded-proto").unwrap_or_else(|| "http".to_string());
    format!("{proto}://{host}/api/v1/auth/oidc/callback")
}

/// 出错时重定向回前端登录页并携带错误信息。
fn redirect_login_error(message: &str) -> Response {
    let msg = urlencoding_encode(message);
    Redirect::temporary(&format!("/#/login?error={msg}")).into_response()
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("v", s)
        .finish()
        .trim_start_matches("v=")
        .to_string()
}

fn take_expired<V>(map: &mut HashMap<String, V>, key: &str, ttl: Duration) -> Option<V>
where
    V: HasCreated,
{
    let entry = map.remove(key)?;
    if entry.created().elapsed() > ttl {
        return None;
    }
    Some(entry)
}

trait HasCreated {
    fn created(&self) -> Instant;
}

impl HasCreated for Pending {
    fn created(&self) -> Instant {
        self.created
    }
}

impl HasCreated for CodeGrant {
    fn created(&self) -> Instant {
        self.created
    }
}

// ============ Handlers ============

/// GET /api/v1/auth/oidc/status —— 登录页渲染依据（公开端点）。
pub async fn oidc_status(State(st): State<AppState>) -> Response {
    (
        StatusCode::OK,
        axum::Json(json!({
            "oidc_enabled": st.cfg.oidc.is_some(),
            "password_login_enabled": password_login_enabled(&st.cfg),
        })),
    )
        .into_response()
}

/// GET /api/v1/auth/oidc/login —— 发起授权码流程，302 到 IdP。
pub async fn oidc_login(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let Some(cfg) = st.cfg.oidc.clone() else {
        return json_err(StatusCode::NOT_FOUND, "oidc_disabled", "OIDC 登录未启用");
    };
    let runtime = st.oidc.clone();
    let discovery_cfg = cfg.clone();
    let disc = match tokio::task::spawn_blocking(move || runtime.discovery(&discovery_cfg)).await {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => return redirect_login_error(&e),
        Err(e) => return redirect_login_error(&format!("OIDC discovery 任务失败: {e}")),
    };
    if disc.authorization_endpoint.is_empty() {
        return redirect_login_error("OIDC discovery 缺少 authorization 端点");
    }
    if disc
        .userinfo_endpoint
        .as_deref()
        .unwrap_or_default()
        .is_empty()
    {
        return redirect_login_error("OIDC discovery 缺少 userinfo 端点");
    }

    let state = random_token();
    // nonce 随授权请求传给 IdP；因本实现不进行本地 ID Token 验签（以 userinfo 校验替代），
    // 服务端不保存 nonce 做回程比对。
    let nonce = random_token();
    st.oidc.pending.lock().unwrap().insert(
        state.clone(),
        Pending {
            created: Instant::now(),
        },
    );

    let redirect = redirect_uri(&cfg, &headers);
    let mut u = match Url::parse(&disc.authorization_endpoint) {
        Ok(u) => u,
        Err(e) => return redirect_login_error(&format!("authorization endpoint 非法: {e}")),
    };
    {
        let mut q = u.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", &cfg.client_id);
        q.append_pair("redirect_uri", &redirect);
        q.append_pair("scope", "openid email profile");
        q.append_pair("state", &state);
        q.append_pair("nonce", &nonce);
    }
    Redirect::temporary(u.as_str()).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

/// GET /api/v1/auth/oidc/callback —— IdP 回调，校验身份后签发一次性交换码。
pub async fn oidc_callback(
    State(st): State<AppState>,
    Query(q): Query<CallbackQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(cfg) = st.cfg.oidc.clone() else {
        return json_err(StatusCode::NOT_FOUND, "oidc_disabled", "OIDC 登录未启用");
    };
    if let Some(err) = &q.error {
        let desc = q.error_description.as_deref().unwrap_or("");
        return redirect_login_error(&format!("IdP 拒绝授权: {err} {desc}"));
    }

    // state 一次性消费 + 过期检查（防 CSRF / 重放）
    let Some(state) = q.state.as_deref().filter(|s| !s.is_empty()) else {
        return redirect_login_error("缺少 OIDC state");
    };
    let pending = take_expired(&mut st.oidc.pending.lock().unwrap(), state, PENDING_TTL);
    if pending.is_none() {
        return redirect_login_error("OIDC state 无效或已过期，请重新登录");
    }

    let Some(code) = q.code.as_deref().filter(|c| !c.is_empty()) else {
        return redirect_login_error("缺少 OIDC authorization code");
    };

    let runtime = st.oidc.clone();
    let discovery_cfg = cfg.clone();
    let callback_headers = headers.clone();
    let callback_code = code.to_string();
    let flow = tokio::task::spawn_blocking(move || {
        let disc = runtime.discovery(&discovery_cfg)?;
        let userinfo_endpoint = disc
            .userinfo_endpoint
            .clone()
            .ok_or("discovery 缺少 userinfo 端点")?;
        let redirect = redirect_uri(&discovery_cfg, &callback_headers);
        let access_token = exchange_code(
            &discovery_cfg,
            &disc.token_endpoint,
            &callback_code,
            &redirect,
        )?;
        let ui = fetch_userinfo(&userinfo_endpoint, &access_token)?;
        if !identity_allowed(&discovery_cfg, &ui) {
            // 记录实际收到的身份便于排查 IdP 侧账号/claim 配置错误；
            // email 为自托管部署者本人可见的日志，sub 仅输出前缀
            let email_desc = match (&ui.email, ui.email_verified) {
                (Some(e), Some(false)) => format!("{e}（未验证）"),
                (Some(e), _) => e.clone(),
                (None, _) => "<IdP 未返回 email>".to_string(),
            };
            let sub_prefix: String = ui.sub.chars().take(8).collect();
            tracing::warn!(
                email = %email_desc,
                sub_prefix = %sub_prefix,
                "OIDC 登录被拒：身份不在白名单（检查 METRIA_OIDC_ALLOWED_EMAIL/ALLOWED_SUBJECT）"
            );
            return Err("该账号未被授权访问本系统（单用户模式）".to_string());
        }
        let username = ui.email.clone().unwrap_or_else(|| ui.sub.clone());
        let display_name = ui.name.or(ui.preferred_username);
        let avatar_url = safe_picture_url(ui.picture.as_deref());
        Ok::<(String, Option<String>, Option<String>), String>((username, display_name, avatar_url))
    })
    .await;
    let (username, display_name, avatar_url) = match flow {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "OIDC 登录失败");
            return redirect_login_error(&e);
        }
        Err(e) => {
            tracing::warn!(error = %e, "OIDC 回调任务失败");
            return redirect_login_error(&format!("OIDC 回调任务失败: {e}"));
        }
    };

    // 建立 users 记录（无本地密码），复用现有 profile / 会话机制
    if let Err(e) =
        st.db
            .ensure_oidc_user(&username, display_name.as_deref(), avatar_url.as_deref())
    {
        return json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "db_error",
            &e.to_string(),
        );
    }

    let one_time = random_token();
    st.oidc.codes.lock().unwrap().insert(
        one_time.clone(),
        CodeGrant {
            username,
            created: Instant::now(),
        },
    );
    Redirect::temporary(&format!("/#/oidc-callback?code={one_time}")).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ExchangeRequest {
    pub code: String,
}

/// POST /api/v1/auth/oidc/exchange —— 一次性交换码换会话 token。
pub async fn oidc_exchange(
    State(st): State<AppState>,
    axum::Json(req): axum::Json<ExchangeRequest>,
) -> Response {
    if req.code.is_empty() {
        return json_err(StatusCode::BAD_REQUEST, "invalid_code", "交换码为空");
    }
    let grant = take_expired(&mut st.oidc.codes.lock().unwrap(), &req.code, CODE_TTL);
    let Some(grant) = grant else {
        return json_err(
            StatusCode::BAD_REQUEST,
            "invalid_code",
            "交换码无效或已过期",
        );
    };
    let token = super::sign_session(&grant.username);
    st.sessions
        .lock()
        .unwrap()
        .insert(token.clone(), grant.username.clone());
    axum::Json(json!({ "token": token, "username": grant.username })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(email: Option<&str>, subject: Option<&str>) -> OidcConfig {
        OidcConfig {
            issuer: "https://idp.example.com/realms/x".into(),
            client_id: "metria".into(),
            client_secret: "secret".into(),
            allowed_email: email.map(|s| s.to_string()),
            allowed_subject: subject.map(|s| s.to_string()),
            redirect_url: None,
            disable_password_login: false,
        }
    }

    fn ui(sub: &str, email: Option<&str>, verified: Option<bool>) -> UserInfo {
        UserInfo {
            sub: sub.into(),
            email: email.map(|s| s.into()),
            email_verified: verified,
            preferred_username: None,
            name: None,
            picture: None,
        }
    }

    #[test]
    fn identity_allowed_matches_email_case_insensitively() {
        let c = cfg(Some("Owner@Example.com"), None);
        assert!(identity_allowed(
            &c,
            &ui("sub-1", Some("owner@example.com"), Some(true))
        ));
        // 未验证邮箱不得通过 email 匹配
        assert!(!identity_allowed(
            &c,
            &ui("sub-1", Some("owner@example.com"), Some(false))
        ));
        assert!(!identity_allowed(
            &c,
            &ui("sub-1", Some("other@example.com"), Some(true))
        ));
        assert!(!identity_allowed(&c, &ui("sub-1", None, None)));
    }

    #[test]
    fn identity_allowed_matches_subject_and_any_of() {
        let c = cfg(None, Some("sub-42"));
        assert!(identity_allowed(&c, &ui("sub-42", None, None)));
        assert!(!identity_allowed(&c, &ui("sub-43", None, None)));

        // 任一命中即放行
        let both = cfg(Some("a@example.com"), Some("sub-42"));
        assert!(identity_allowed(&both, &ui("sub-42", None, None)));
        assert!(identity_allowed(
            &both,
            &ui("sub-99", Some("A@example.com"), None)
        ));

        // 两者都未配置时拒绝（配置层已拦截，此处兜底）
        let none = cfg(None, None);
        assert!(!identity_allowed(
            &none,
            &ui("sub-1", Some("a@example.com"), Some(true))
        ));
    }

    #[test]
    fn picture_requires_https_with_host() {
        assert_eq!(
            safe_picture_url(Some("https://idp.example.com/avatar.png")).as_deref(),
            Some("https://idp.example.com/avatar.png")
        );
        assert!(safe_picture_url(Some("http://idp.example.com/avatar.png")).is_none());
        assert!(safe_picture_url(Some("data:image/png;base64,abc")).is_none());
        assert!(safe_picture_url(Some("not-a-url")).is_none());
        assert!(safe_picture_url(None).is_none());
    }

    #[test]
    fn pending_state_is_single_use_and_expires() {
        let rt = OidcRuntime::new();
        rt.pending.lock().unwrap().insert(
            "st".into(),
            Pending {
                created: Instant::now(),
            },
        );
        assert!(take_expired(&mut rt.pending.lock().unwrap(), "st", PENDING_TTL).is_some());
        // 一次性：第二次取不到
        assert!(take_expired(&mut rt.pending.lock().unwrap(), "st", PENDING_TTL).is_none());
        // 过期：TTL=0 时视为失效
        rt.pending.lock().unwrap().insert(
            "old".into(),
            Pending {
                created: Instant::now(),
            },
        );
        assert!(take_expired(&mut rt.pending.lock().unwrap(), "old", Duration::ZERO).is_none());
    }

    #[test]
    fn exchange_code_is_single_use() {
        let rt = OidcRuntime::new();
        rt.codes.lock().unwrap().insert(
            "code1".into(),
            CodeGrant {
                username: "u@example.com".into(),
                created: Instant::now(),
            },
        );
        assert!(take_expired(&mut rt.codes.lock().unwrap(), "code1", CODE_TTL).is_some());
        assert!(take_expired(&mut rt.codes.lock().unwrap(), "code1", CODE_TTL).is_none());
    }

    fn discovery(issuer: &str) -> Discovery {
        Discovery {
            issuer: issuer.to_string(),
            authorization_endpoint: format!("{issuer}/authorize"),
            token_endpoint: format!("{issuer}/token"),
            userinfo_endpoint: Some(format!("{issuer}/userinfo")),
        }
    }

    #[test]
    fn cache_control_max_age_is_parsed() {
        assert_eq!(cache_control_max_age("public, max-age=120"), Some(120));
        assert_eq!(cache_control_max_age("max-age=0"), Some(0));
        assert_eq!(cache_control_max_age("no-store"), None);
    }

    #[test]
    fn discovery_cache_reuses_and_clamps_ttl() {
        let cfg = cfg(None, Some("s"));
        let cache = DiscoveryCache::default();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let fetch = || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok((discovery(&cfg.issuer), Duration::from_secs(5)))
        };
        let first = cache.get_or_fetch(&cfg, fetch).unwrap();
        let second = cache
            .get_or_fetch(&cfg, || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok((discovery(&cfg.issuer), DISCOVERY_DEFAULT_TTL))
            })
            .unwrap();
        assert_eq!(first.issuer, second.issuer);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let entry = cache.entry.lock().unwrap();
        assert_eq!(entry.as_ref().unwrap().ttl, DISCOVERY_MIN_TTL);
    }

    #[test]
    fn discovery_cache_uses_stale_after_refresh_failure_but_not_after_24h() {
        let cfg = cfg(None, Some("s"));
        let cache = DiscoveryCache::default();
        {
            let mut entry = cache.entry.lock().unwrap();
            *entry = Some(CachedDiscovery {
                discovery: discovery(&cfg.issuer),
                fetched_at: Instant::now() - Duration::from_secs(2 * 3600),
                ttl: DISCOVERY_DEFAULT_TTL,
                issuer: cfg.issuer.clone(),
            });
        }
        let stale = cache
            .get_or_fetch(&cfg, || Err("IdP down".into()))
            .expect("24h 内应允许 stale fallback");
        assert_eq!(stale.issuer, cfg.issuer);

        {
            let mut entry = cache.entry.lock().unwrap();
            entry.as_mut().unwrap().fetched_at = Instant::now() - Duration::from_secs(25 * 3600);
        }
        assert!(cache.get_or_fetch(&cfg, || Err("IdP down".into())).is_err());
    }

    #[test]
    fn discovery_cache_rejects_other_issuer_and_single_flights() {
        let cfg = cfg(None, Some("s"));
        let cache = DiscoveryCache::default();
        {
            let mut entry = cache.entry.lock().unwrap();
            *entry = Some(CachedDiscovery {
                discovery: discovery("https://other.example.com"),
                fetched_at: Instant::now(),
                ttl: DISCOVERY_DEFAULT_TTL,
                issuer: "https://other.example.com".into(),
            });
        }
        let refreshed = cache
            .get_or_fetch(&cfg, || Ok((discovery(&cfg.issuer), DISCOVERY_DEFAULT_TTL)))
            .unwrap();
        assert_eq!(refreshed.issuer, cfg.issuer);

        let cache = std::sync::Arc::new(DiscoveryCache::default());
        let cfg = std::sync::Arc::new(cfg);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            for _ in 0..2 {
                let cache = cache.clone();
                let cfg = cfg.clone();
                let calls = calls.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    cache
                        .get_or_fetch(&cfg, || {
                            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(50));
                            Ok((discovery(&cfg.issuer), DISCOVERY_DEFAULT_TTL))
                        })
                        .unwrap();
                });
            }
        });
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "并发登录应共享一次 discovery"
        );
    }

    #[test]
    fn redirect_uri_prefers_explicit_config_then_forwarded_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "hub.internal:8080".parse().unwrap());

        let mut explicit = cfg(None, Some("s"));
        explicit.redirect_url = Some("https://metria.example.com/api/v1/auth/oidc/callback".into());
        assert_eq!(
            redirect_uri(&explicit, &headers),
            "https://metria.example.com/api/v1/auth/oidc/callback"
        );

        let derived = cfg(None, Some("s"));
        assert_eq!(
            redirect_uri(&derived, &headers),
            "http://hub.internal:8080/api/v1/auth/oidc/callback"
        );

        let mut proxied = HeaderMap::new();
        proxied.insert("x-forwarded-host", "metria.example.com".parse().unwrap());
        proxied.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(
            redirect_uri(&derived, &proxied),
            "https://metria.example.com/api/v1/auth/oidc/callback"
        );
    }
}
