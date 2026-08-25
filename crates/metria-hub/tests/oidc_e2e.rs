//! OIDC 单用户登录端到端测试。
//!
//! 使用内置 mock IdP（discovery / token / userinfo）验证完整授权码流程：
//! status → login（302 IdP）→ callback（身份白名单 + 一次性交换码）
//! → exchange（会话 token）→ me；并覆盖重放拒绝、非白名单拒绝、密码登录禁用。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

use metria_hub::api::AppState;
use metria_hub::config::{HubConfig, OidcConfig};
use metria_hub::db::HubDb;
use serde_json::{json, Value};
use tokio::net::TcpListener as AsyncTcpListener;

const GOOD_CODE: &str = "good-code";
const OTHER_USER_CODE: &str = "other-user-code";
const OWNER_EMAIL: &str = "owner@example.com";
const OTHER_EMAIL: &str = "intruder@example.com";

fn oidc_config(idp_base: &str, disable_password: bool) -> OidcConfig {
    OidcConfig {
        issuer: idp_base.to_string(),
        client_id: "metria-hub-e2e".to_string(),
        client_secret: "e2e-secret".to_string(),
        allowed_email: Some(OWNER_EMAIL.to_string()),
        allowed_subject: None,
        redirect_url: None,
        disable_password_login: disable_password,
    }
}

fn test_cfg(dir: &std::path::Path, oidc: Option<OidcConfig>) -> HubConfig {
    HubConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir.to_path_buf(),
        database_url: format!("sqlite://{}/hub.db", dir.display()),
        content_mode: metria_core::ContentMode::Metadata,
        timezone: chrono_tz::Tz::UTC,
        log_filter: "error".into(),
        demo: false,
        oidc,
    }
}

async fn spawn_hub(dir: &std::path::Path, oidc: Option<OidcConfig>) -> (String, AppState) {
    let db = HubDb::open(&test_cfg(dir, None)).expect("open db");
    db.apply_migrations().expect("migrate");
    let state = AppState {
        db: db.clone(),
        cfg: test_cfg(dir, oidc),
        sse: metria_hub::api::SseHub::new(),
        sessions: Default::default(),
        collector_token: Some("testtok".into()),
        oidc: Default::default(),
    };
    let listener = AsyncTcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = metria_hub::api::app_router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), state)
}

/// 极简 mock IdP：仅支持本测试所需端点，每连接处理一个请求后关闭。
fn spawn_mock_idp() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let base = format!("http://{addr}");
            std::thread::spawn(move || handle_idp_conn(stream, &base));
        }
    });
    format!("http://{addr}")
}

fn handle_idp_conn(stream: std::net::TcpStream, base: &str) {
    let Ok(mut stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream.try_clone().unwrap());

    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let full_path = parts.next().unwrap_or("").to_string();

    let mut headers: HashMap<String, String> = HashMap::new();
    let mut content_length = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 {
            return;
        }
        let t = h.trim().to_string();
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            }
            headers.insert(key, val);
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).unwrap();
    }

    let (status, payload) = route_idp(&method, &full_path, &headers, &body, base);
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

fn route_idp(
    method: &str,
    full_path: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    base: &str,
) -> (&'static str, String) {
    let path = full_path.split('?').next().unwrap_or("");
    match (method, path) {
        ("GET", "/.well-known/openid-configuration") => (
            "200 OK",
            json!({
                "issuer": base,
                "authorization_endpoint": format!("{base}/oauth2/authorize"),
                "token_endpoint": format!("{base}/oauth2/token"),
                "userinfo_endpoint": format!("{base}/oauth2/userinfo"),
            })
            .to_string(),
        ),
        ("POST", "/oauth2/token") => {
            let form: HashMap<String, String> =
                url::form_urlencoded::parse(body).into_owned().collect();
            assert_eq!(
                form.get("grant_type").map(|s| s.as_str()),
                Some("authorization_code")
            );
            assert_eq!(
                form.get("client_id").map(|s| s.as_str()),
                Some("metria-hub-e2e")
            );
            assert_eq!(
                form.get("client_secret").map(|s| s.as_str()),
                Some("e2e-secret")
            );
            match form.get("code").map(|s| s.as_str()) {
                Some(GOOD_CODE) => (
                    "200 OK",
                    json!({ "access_token": "at-owner", "token_type": "Bearer", "expires_in": 300 })
                        .to_string(),
                ),
                Some(OTHER_USER_CODE) => (
                    "200 OK",
                    json!({ "access_token": "at-other", "token_type": "Bearer", "expires_in": 300 })
                        .to_string(),
                ),
                _ => ("400 Bad Request", json!({ "error": "invalid_grant" }).to_string()),
            }
        }
        ("GET", "/oauth2/userinfo") => {
            let auth = headers.get("authorization").cloned().unwrap_or_default();
            match auth.as_str() {
                "Bearer at-owner" => (
                    "200 OK",
                    json!({
                        "sub": "sub-owner",
                        "email": OWNER_EMAIL,
                        "email_verified": true,
                        "preferred_username": "owner",
                        "name": "Owner"
                    })
                    .to_string(),
                ),
                "Bearer at-other" => (
                    "200 OK",
                    json!({
                        "sub": "sub-intruder",
                        "email": OTHER_EMAIL,
                        "email_verified": true,
                        "preferred_username": "intruder"
                    })
                    .to_string(),
                ),
                _ => (
                    "401 Unauthorized",
                    json!({ "error": "invalid_token" }).to_string(),
                ),
            }
        }
        _ => ("404 Not Found", "{}".to_string()),
    }
}

// ============ HTTP 工具 ============

fn no_redirect_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).build()
}

/// 从 no-redirect 响应中提取 Location（ureq 对 redirects(0) 时 3xx 可能返回 Ok）。
fn location_of_result(res: Result<ureq::Response, ureq::Error>) -> String {
    let (status, header_loc) = match res {
        Ok(resp) => (
            resp.status(),
            resp.header("location").map(|s| s.to_string()),
        ),
        Err(ureq::Error::Status(code, resp)) => {
            (code, resp.header("location").map(|s| s.to_string()))
        }
        Err(other) => panic!("期望 3xx 重定向，得到 {other}"),
    };
    assert!(
        (300..400).contains(&status),
        "期望 3xx 重定向，得到 {status}"
    );
    header_loc.unwrap_or_default()
}

fn param_of(location: &str, key: &str) -> Option<String> {
    // Location 可能含 fragment：/#/oidc-callback?code=xxx
    let after_hash_query = location
        .split_once('#')
        .map(|(_, frag)| frag.split_once('?').map(|(_, q)| q))
        .unwrap_or(None);
    let query = after_hash_query.or_else(|| location.split_once('?').map(|(_, q)| q));
    let query = query?;
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

// ============ 测试 ============

#[tokio::test(flavor = "multi_thread")]
async fn oidc_status_reports_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let idp = spawn_mock_idp();

    let (base_off, _) = spawn_hub(dir.path().join("off").as_path(), None).await;
    let (base_on, _) = spawn_hub(
        dir.path().join("on").as_path(),
        Some(oidc_config(&idp, false)),
    )
    .await;

    let off: Value = ureq::get(&format!("{base_off}/api/v1/auth/oidc/status"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(off["oidc_enabled"], false);
    assert_eq!(off["password_login_enabled"], true);

    let on: Value = ureq::get(&format!("{base_on}/api/v1/auth/oidc/status"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(on["oidc_enabled"], true);
    assert_eq!(on["password_login_enabled"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn oidc_full_flow_issues_session_for_allowed_user() {
    let dir = tempfile::tempdir().unwrap();
    let idp = spawn_mock_idp();
    let (hub, _) = spawn_hub(dir.path(), Some(oidc_config(&idp, false))).await;

    // 1. login → 302 到 IdP authorize 端点
    let loc = location_of_result(
        no_redirect_agent()
            .get(&format!("{hub}/api/v1/auth/oidc/login"))
            .call(),
    );
    assert!(
        loc.starts_with(&format!("{idp}/oauth2/authorize")),
        "location={loc}"
    );
    assert!(loc.contains("response_type=code"));
    assert!(loc.contains("client_id=metria-hub-e2e"));
    assert!(
        loc.contains("scope=openid+email+profile")
            || loc.contains("scope=openid%20email%20profile")
    );
    let state = param_of(&loc, "state").expect("authorize URL 应包含 state");

    // 2. 模拟 IdP 回调（合法用户）→ 302 前端一次性交换码
    let loc = location_of_result(
        no_redirect_agent()
            .get(&format!(
                "{hub}/api/v1/auth/oidc/callback?code={GOOD_CODE}&state={state}"
            ))
            .call(),
    );
    assert!(loc.contains("/#/oidc-callback"), "location={loc}");
    let one_time = param_of(&loc, "code").expect("callback 应携带一次性交换码");

    // 3. exchange → 会话 token；me 返回白名单邮箱
    let ex: Value = ureq::post(&format!("{hub}/api/v1/auth/oidc/exchange"))
        .send_json(json!({ "code": one_time }))
        .unwrap()
        .into_json()
        .unwrap();
    let token = ex["token"].as_str().unwrap();
    assert_eq!(ex["username"], OWNER_EMAIL);

    let me: Value = ureq::get(&format!("{hub}/api/v1/auth/me"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(me["username"], OWNER_EMAIL);
    assert_eq!(me["ok"], true);

    // 4. 交换码一次性：重放拒绝
    let replay = ureq::post(&format!("{hub}/api/v1/auth/oidc/exchange"))
        .send_json(json!({ "code": one_time }));
    assert!(replay.is_err(), "重复使用交换码应失败");

    // 5. state 一次性：重放回调拒绝（错误页重定向）
    let loc = location_of_result(
        no_redirect_agent()
            .get(&format!(
                "{hub}/api/v1/auth/oidc/callback?code={GOOD_CODE}&state={state}"
            ))
            .call(),
    );
    assert!(
        loc.contains("/#/login"),
        "state 重放应回到登录页错误: {loc}"
    );
    assert!(param_of(&loc, "error").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn oidc_rejects_user_outside_allowlist_and_bad_state() {
    let dir = tempfile::tempdir().unwrap();
    let idp = spawn_mock_idp();
    let (hub, _) = spawn_hub(dir.path(), Some(oidc_config(&idp, false))).await;

    // 非白名单账号：IdP 正常发 token，但 userinfo 身份不匹配
    let loc = location_of_result(
        no_redirect_agent()
            .get(&format!("{hub}/api/v1/auth/oidc/login"))
            .call(),
    );
    let state = param_of(&loc, "state").unwrap();
    let loc = location_of_result(
        no_redirect_agent()
            .get(&format!(
                "{hub}/api/v1/auth/oidc/callback?code={OTHER_USER_CODE}&state={state}"
            ))
            .call(),
    );
    assert!(loc.contains("/#/login"), "location={loc}");
    let msg = param_of(&loc, "error").unwrap_or_default();
    assert!(msg.contains("未被授权"), "错误信息应说明未授权: {msg}");

    // 无效 state 直接拒绝
    let res = no_redirect_agent()
        .get(&format!(
            "{hub}/api/v1/auth/oidc/callback?code={GOOD_CODE}&state=forged-state"
        ))
        .call();
    assert!(location_of_result(res).contains("/#/login"));

    // 缺 state 拒绝
    let res = no_redirect_agent()
        .get(&format!("{hub}/api/v1/auth/oidc/callback?code={GOOD_CODE}"))
        .call();
    assert!(location_of_result(res).contains("/#/login"));
}

#[tokio::test(flavor = "multi_thread")]
async fn oidc_disable_password_login_blocks_local_login() {
    let dir = tempfile::tempdir().unwrap();
    let idp = spawn_mock_idp();
    let (hub, _) = spawn_hub(dir.path(), Some(oidc_config(&idp, true))).await;

    let status: Value = ureq::get(&format!("{hub}/api/v1/auth/oidc/status"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(status["password_login_enabled"], false);

    let denied = ureq::post(&format!("{hub}/api/v1/auth/login"))
        .send_json(json!({ "username": "admin", "password": "change-me-please" }));
    match denied {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 403),
        other => panic!("禁用密码登录后期望 403，得到 {other:?}"),
    }

    // OIDC 流程仍可正常登录
    let loc = location_of_result(
        no_redirect_agent()
            .get(&format!("{hub}/api/v1/auth/oidc/login"))
            .call(),
    );
    let state = param_of(&loc, "state").unwrap();
    let loc = location_of_result(
        no_redirect_agent()
            .get(&format!(
                "{hub}/api/v1/auth/oidc/callback?code={GOOD_CODE}&state={state}"
            ))
            .call(),
    );
    let one_time = param_of(&loc, "code").unwrap();
    let ex: Value = ureq::post(&format!("{hub}/api/v1/auth/oidc/exchange"))
        .send_json(json!({ "code": one_time }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(ex["username"], OWNER_EMAIL);
}

#[tokio::test(flavor = "multi_thread")]
async fn oidc_disabled_endpoints_return_404() {
    let dir = tempfile::tempdir().unwrap();
    let (hub, _) = spawn_hub(dir.path(), None).await;

    for path in ["/api/v1/auth/oidc/login", "/api/v1/auth/oidc/callback"] {
        let res = no_redirect_agent().get(&format!("{hub}{path}")).call();
        match res {
            Err(ureq::Error::Status(code, _)) => assert_eq!(code, 404, "path={path}"),
            Err(e) => panic!("path={path}: {e}"),
            Ok(_) => panic!("OIDC 未配置时期望 404: {path}"),
        }
    }
}
