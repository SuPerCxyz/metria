//! C1：静态资源缓存头、ETag/304、HEAD 与压缩。

use std::io::Read;
use std::time::Duration;

use metria_hub::api::AppState;
use metria_hub::assets::Assets;
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_hub::http::with_compression;
use serde_json::json;
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
    let db = HubDb::open(&cfg).unwrap();
    db.apply_migrations().unwrap();
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
    let app = with_compression(
        metria_hub::api::app_router(state).fallback(metria_hub::http::static_fallback),
    );
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

fn hashed_js() -> String {
    Assets::iter()
        .map(|path| path.to_string())
        .find(|path| path.starts_with("assets/") && path.ends_with(".js"))
        .expect("hashed js asset")
}

fn admin_token(base: &str) -> String {
    let resp = ureq::post(&format!("{base}/api/v1/auth/login"))
        .send_json(json!({ "username": "admin", "password": "change-me-please" }))
        .unwrap();
    resp.into_json::<serde_json::Value>().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn static_assets_have_cache_validators_and_head_support() {
    let dir = tempfile::tempdir().unwrap();
    let base = spawn_hub(dir.path()).await;
    let js = hashed_js();

    let resp = ureq::get(&format!("{base}/static/{js}")).call().unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    let etag = resp.header("etag").expect("etag").to_string();
    assert!(etag.starts_with('"') && etag.ends_with('"'));

    let not_modified = ureq::get(&format!("{base}/static/{js}"))
        .set("If-None-Match", &etag)
        .call()
        .unwrap();
    assert_eq!(not_modified.status(), 304);
    assert_eq!(not_modified.header("etag"), Some(etag.as_str()));

    let head = ureq::head(&format!("{base}/static/{js}")).call().unwrap();
    assert_eq!(head.status(), 200);
    let mut body = Vec::new();
    head.into_reader().read_to_end(&mut body).unwrap();
    assert!(body.is_empty(), "HEAD 不得返回 body");

    let index = ureq::get(&format!("{base}/static/index.html"))
        .call()
        .unwrap();
    assert_eq!(index.status(), 200);
    assert_eq!(index.header("cache-control"), Some("no-cache"));
    assert!(index.header("etag").is_some());

    let spa = ureq::get(&format!("{base}/some/spa/route")).call().unwrap();
    assert_eq!(spa.status(), 200);
    assert_eq!(spa.header("cache-control"), Some("no-cache"));

    let font = ureq::get(&format!(
        "{base}/static/fonts/MapleMono-Medium-subset.woff2"
    ))
    .call()
    .unwrap();
    assert_eq!(font.status(), 200);
    assert_eq!(font.header("content-type"), Some("font/woff2"));
    assert_eq!(font.header("cache-control"), Some("public, max-age=86400"));
    let mut font_bytes = Vec::new();
    font.into_reader().read_to_end(&mut font_bytes).unwrap();
    assert!(font_bytes.len() < 20 * 1024, "font subset 必须 ≤20KB");

    // 旧 CSS 的 /static/static/fonts/* 兼容一个版本。
    let legacy = ureq::get(&format!(
        "{base}/static/static/fonts/MapleMono-Medium.woff2"
    ))
    .call()
    .unwrap();
    assert_eq!(legacy.status(), 200);
    assert_eq!(legacy.header("content-type"), Some("font/woff2"));

    let compressed_font = ureq::get(&format!(
        "{base}/static/fonts/MapleMono-Medium-subset.woff2"
    ))
    .set("Accept-Encoding", "gzip")
    .call()
    .unwrap();
    assert!(
        compressed_font.header("content-encoding").is_none(),
        "woff2 不得二次压缩"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn text_assets_are_compressed_and_sse_is_excluded() {
    let dir = tempfile::tempdir().unwrap();
    let base = spawn_hub(dir.path()).await;
    let js = hashed_js();

    for encoding in ["gzip", "br"] {
        let resp = ureq::get(&format!("{base}/static/{js}"))
            .set("Accept-Encoding", encoding)
            .call()
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.header("content-encoding"),
            Some(encoding),
            "缺少 {encoding} 压缩"
        );
        assert!(
            resp.header("vary")
                .is_some_and(|v| v.to_ascii_lowercase().contains("accept-encoding")),
            "缺少 Vary: Accept-Encoding"
        );
    }

    // SSE 通过认证 query token 访问；只读取响应头，不等待事件流结束。
    let token = admin_token(&base);
    let sse = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build()
        .get(&format!("{base}/api/v1/stream?token={token}"))
        .set("Accept-Encoding", "gzip")
        .call();
    if let Ok(resp) = sse {
        assert!(resp.header("content-encoding").is_none(), "SSE 不得压缩");
    }
}
