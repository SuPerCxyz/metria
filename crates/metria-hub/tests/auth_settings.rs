//! 账户资料、密码和系统设置 API 回归测试。

use metria_hub::api::AppState;
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use serde_json::{json, Value};
use tokio::net::TcpListener;

fn test_cfg(dir: &std::path::Path) -> HubConfig {
    HubConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir.to_path_buf(),
        database_url: format!("sqlite://{}/hub.db", dir.display()),
        content_mode: metria_core::ContentMode::Metadata,
        timezone: chrono_tz::Tz::UTC,
        log_filter: "error".into(),
        demo: false,
        oidc: None,
    }
}

async fn spawn_hub(dir: &std::path::Path) -> String {
    let cfg = test_cfg(dir);
    let db = HubDb::open(&cfg).expect("open db");
    db.apply_migrations().expect("migrate");
    let state = AppState {
        db,
        cfg,
        sse: metria_hub::api::SseHub::new(),
        sessions: Default::default(),
        oidc: Default::default(),
        collector_token: Some("testtok".into()),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = metria_hub::api::app_router(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

fn login(base: &str, password: &str) -> Result<String, Box<dyn std::error::Error>> {
    let response = ureq::post(&format!("{base}/api/v1/auth/login"))
        .send_json(json!({ "username": "admin", "password": password }))
        .map_err(Box::new)?;
    let body = response.into_json::<Value>()?;
    Ok(body["token"].as_str().unwrap().to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_password_and_system_info_form_a_real_loop() {
    let dir = tempfile::tempdir().unwrap();
    let base = spawn_hub(dir.path()).await;
    let token = login(&base, "change-me-please").unwrap();

    let profile = ureq::get(&format!("{base}/api/v1/auth/me"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json::<Value>()
        .unwrap();
    assert_eq!(profile["username"], "admin");
    assert_eq!(profile["avatar_color"], "indigo");

    let updated = ureq::put(&format!("{base}/api/v1/auth/profile"))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(json!({
            "display_name": "运维管理员",
            "avatar_text": "运维",
            "avatar_color": "emerald"
        }))
        .unwrap()
        .into_json::<Value>()
        .unwrap();
    assert_eq!(updated["display_name"], "运维管理员");
    assert_eq!(updated["avatar_text"], "运维");
    assert_eq!(updated["avatar_color"], "emerald");

    let wrong_old = ureq::post(&format!("{base}/api/v1/auth/change-password"))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(json!({ "old_password": "wrong-password", "new_password": "new-password-123" }));
    assert!(wrong_old.is_err());

    ureq::post(&format!("{base}/api/v1/auth/change-password"))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(
            json!({ "old_password": "change-me-please", "new_password": "new-password-123" }),
        )
        .unwrap();
    assert!(login(&base, "change-me-please").is_err());
    let new_token = login(&base, "new-password-123").unwrap();

    let info = ureq::get(&format!("{base}/api/v1/system/info"))
        .set("Authorization", &format!("Bearer {new_token}"))
        .call()
        .unwrap()
        .into_json::<Value>()
        .unwrap();
    assert_eq!(info["content_mode"], "metadata");
    assert_eq!(info["retention"]["automatic_cleanup"], false);
}
