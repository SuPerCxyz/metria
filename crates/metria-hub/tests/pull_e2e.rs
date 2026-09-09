//! Pull 模式端到端测试：Hub 调度器 → Agent（pull 服务）→ ingest → ack → spool 清理。
//!
//! 拓扑：Hub 与 Agent 同进程内真实 TCP 通信（Agent 以 pull 模式运行，
//! Hub 按 agent_url 主动拉取），验证「Hub 主动访问 Agent」的完整闭环。

use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use metria_hub::api::AppState;
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use serde_json::{json, Value};
use tokio::net::TcpListener as AsyncTcpListener;

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

async fn spawn_hub(dir: &std::path::Path) -> (String, AppState) {
    let db = HubDb::open(&test_cfg(dir)).expect("open db");
    db.apply_migrations().expect("migrate");
    let state = AppState {
        db: db.clone(),
        cfg: test_cfg(dir),
        sse: metria_hub::api::SseHub::new(),
        sessions: Default::default(),
        collector_token: Some("testtok".into()),
        oidc: Default::default(),
    };
    // e2e 不走 serve()，需显式启动 pull 调度器（与生产一致）
    metria_hub::pull::spawn_pull_scheduler(state.clone());
    let listener = AsyncTcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = metria_hub::api::app_router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), state)
}

fn admin_token(base: &str) -> String {
    let resp = ureq::post(&format!("{base}/api/v1/auth/login"))
        .send_json(json!({ "username": "admin", "password": "change-me-please" }))
        .unwrap();
    resp.into_json::<Value>().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

/// 极简 HTTP GET（返回状态码与 JSON 体）。
fn get_json(url: &str, token: Option<&str>) -> (u16, Value) {
    let mut stream =
        TcpStream::connect(url.trim_start_matches("http://").split('/').next().unwrap()).unwrap();
    let host = url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let path = match url.split_once(host.as_str()) {
        Some((_, rest)) if !rest.is_empty() => rest.to_string(),
        _ => "/".to_string(),
    };
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{auth}Connection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    BufReader::new(stream).read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body_start = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(buf.len());
    let v: Value = serde_json::from_slice(&buf[body_start..]).unwrap_or(Value::Null);
    (status, v)
}

fn claude_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude")
}

#[tokio::test(flavor = "multi_thread")]
async fn pull_mode_end_to_end_hub_collects_from_agent() {
    let dir = tempfile::tempdir().unwrap();
    let (hub, _state) = spawn_hub(dir.path()).await;
    let admin = admin_token(&hub);
    let auth = format!("Bearer {admin}");

    // 1. 创建 pull 节点（agent_url 指向稍后启动的 Agent）
    let agent_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let agent_port = agent_listener.local_addr().unwrap().port();
    drop(agent_listener); // 释放端口给 Agent 复用

    let create: Value = ureq::post(&format!("{hub}/api/v1/nodes"))
        .set("Authorization", &auth)
        .send_json(json!({
            "name": "pull-node-e2e",
            "ip": "127.0.0.1",
            "agent_url": format!("http://127.0.0.1:{agent_port}"),
            "platform": "linux",
            "architecture": "amd64",
        }))
        .unwrap()
        .into_json()
        .unwrap();
    let node_id = create["node_id"].as_str().unwrap().to_string();
    let token = create["token"].as_str().unwrap().to_string();
    let agent_token = token.clone();
    assert!(token.starts_with("mct-"));

    // 2. 启动 Agent（pull 模式：仅 token + fixture 客户端路径，无 HUB_URL/NODE_ID）
    let agent_data = dir.path().join("agent-data");
    let agent_cfg = metria_agent::config::AgentConfig {
        node_id: String::new(),
        node_name: String::new(),
        hub_url: None,
        token: Some(token.clone()),
        listen_port: agent_port,
        claude_path: Some(claude_fixture_dir()),
        codex_path: None,
        opencode_path: None,
        content_mode: metria_core::ContentMode::Metadata,
        data_dir: agent_data.clone(),
        max_pending_events: 100_000,
        max_spool_bytes: 64 * 1024 * 1024,
        batch_max_events: 256,
        batch_max_bytes: 1024 * 1024,
        scan_interval_seconds: 2,
        reconcile_interval_seconds: 300,
        heartbeat_interval_seconds: 60,
        upload_interval_seconds: 15,
        poll_interval_seconds: 60,
        token_refresh_interval_seconds: 6 * 24 * 3600,
        log_filter: "error".into(),
    };
    let agent_stop = Arc::new(AtomicBool::new(false));
    {
        let cfg = agent_cfg.clone();
        let _stop = agent_stop.clone();
        std::thread::spawn(move || {
            let _ = metria_agent::runner::run_pull(cfg, Some(agent_token));
        });
    }
    // 等待 Agent 监听就绪（最多 10s，连不上则失败并保留诊断信息）
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut ok = false;
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", agent_port)).is_ok() {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        assert!(ok, "Agent pull 服务未在超时内监听端口 {agent_port}");
    }

    // 3. 等待 Hub 调度拉取（METRIA_PULL_INTERVAL 最小 5s，超时 40s）
    let deadline = Instant::now() + Duration::from_secs(40);
    let node = loop {
        let (status, v) = get_json(&format!("{hub}/api/v1/nodes/{node_id}"), Some(&admin));
        assert_eq!(status, 200);
        let n = v["node"].clone();
        if n["status"] == "online" && n["last_pull_at"] != Value::Null {
            break n;
        }
        if Instant::now() > deadline {
            panic!("Hub 未在超时内完成拉取：{n}");
        }
        std::thread::sleep(Duration::from_secs(1));
    };
    assert_eq!(node["last_pull_error"], Value::Null, "拉取不应有错误");

    // 4. 数据入库断言：fixture 的 session/call/usage 经 ingest 落库
    let (_, overview) = get_json(
        &format!(
            "{hub}/api/v1/usage/breakdown?dim=node&from=2020-01-01T00:00:00Z&to=2030-01-01T00:00:00Z"
        ),
        Some(&admin),
    );
    let by = overview["by"].as_array().cloned().unwrap_or_default();
    let row = by
        .iter()
        .find(|r| r["dimension"] == node_id.as_str())
        .unwrap_or_else(|| panic!("breakdown 应包含 pull 节点数据: {by:?}"));
    assert!(
        row["model_calls"].as_i64().unwrap_or(0) > 0,
        "pull 节点应有调用数据: {row}"
    );

    // 5. Agent 侧 spool 已被 ack 清空（/status pending_events == 0）
    let (status, s) = get_json(
        &format!("http://127.0.0.1:{agent_port}/status"),
        Some(&token),
    );
    assert_eq!(status, 200);
    assert_eq!(s["pending_events"], 0, "ack 后 spool 应清空: {s}");

    // 6. 错误 token 访问 Agent → 401
    let (status, _) = get_json(
        &format!("http://127.0.0.1:{agent_port}/collect"),
        Some("wrong-token"),
    );
    assert_eq!(status, 401);

    agent_stop.store(true, std::sync::atomic::Ordering::Relaxed);
}
