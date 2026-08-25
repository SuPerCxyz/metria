//! Pull 服务集成测试：真实 TCP 监听 + HTTP 请求，覆盖认证与 collect/ack 状态机。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use metria_agent::config::AgentConfig;
use metria_agent::spool::Spool;

fn pull_cfg(dir: &std::path::Path) -> AgentConfig {
    AgentConfig {
        node_id: String::new(),
        node_name: "test".into(),
        hub_url: None,
        token: Some("tok-123".into()),
        listen_port: 0, // 由测试直接绑定端口后忽略（serve 内部绑定）
        claude_path: None,
        codex_path: None,
        opencode_path: None,
        content_mode: metria_core::ContentMode::Metadata,
        data_dir: dir.to_path_buf(),
        max_pending_events: 1000,
        max_spool_bytes: 10_000_000,
        batch_max_events: 256,
        batch_max_bytes: 1024 * 1024,
        scan_interval_seconds: 10,
        reconcile_interval_seconds: 300,
        heartbeat_interval_seconds: 60,
        upload_interval_seconds: 15,
        token_refresh_interval_seconds: 6 * 24 * 3600,
        log_filter: "error".into(),
    }
}

/// 在固定端口启动 pull 服务（测试专用端口段，避免随机端口与配置不一致）。
fn spawn_server(dir: &std::path::Path, port: u16) -> Arc<AtomicBool> {
    let cfg = AgentConfig {
        listen_port: port,
        ..pull_cfg(dir)
    };
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    std::thread::spawn(move || {
        let _ = metria_agent::pullserver::serve(cfg, "tok-123".into(), stop_thread);
    });
    stop
}

fn http(
    port: u16,
    method: &str,
    path: &str,
    auth: Option<&str>,
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let auth_line = auth
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth_line}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    if !body.is_empty() {
        stream.write_all(body).unwrap();
    }
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let batch_id = text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("x-batch-id:"))
        .map(|l| l.split_once(':').unwrap().1.trim().to_string());
    let body_start = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(buf.len());
    (
        status,
        batch_id.unwrap_or_default(),
        buf[body_start..].to_vec(),
    )
}

fn seed_events(dir: &std::path::Path, ids: &[&str]) {
    let mut spool = Spool::open(&dir.join("spool.db"), 1000, 10_000_000).unwrap();
    let events: Vec<metria_agent::spool::PendingEvent> = ids
        .iter()
        .map(|id| metria_agent::spool::PendingEvent {
            event_id: id.to_string(),
            kind: "usage".into(),
            payload: serde_json::json!({"n": 1}),
        })
        .collect();
    spool.insert_batch(&events, &[]).unwrap();
}

#[test]
fn pull_server_auth_collect_and_ack_flow() {
    let dir = std::env::temp_dir().join(format!("pullserver-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    seed_events(&dir, &["e1", "e2", "e3"]);

    // 固定测试端口（并发测试少，冲突概率低；占用则换端口重试一次）
    let port = pick_port();
    let stop = spawn_server(&dir, port);
    std::thread::sleep(Duration::from_millis(300));

    // 无 token → 401
    let (status, _, _) = http(port, "GET", "/collect", None, b"");
    assert_eq!(status, 401);
    // 错误 token → 401
    let (status, _, _) = http(port, "GET", "/collect", Some("wrong"), b"");
    assert_eq!(status, 401);
    // 未知路径 → 404
    let (status, _, _) = http(port, "GET", "/other", Some("tok-123"), b"");
    assert_eq!(status, 404);

    // 正确 token → 200 + zstd 批次
    let (status, batch_id, body) = http(port, "GET", "/collect", Some("tok-123"), b"");
    assert_eq!(status, 200);
    assert!(!batch_id.is_empty());
    let json = zstd::stream::decode_all(&body[..]).unwrap();
    let batch: metria_protocol::UploadBatch = serde_json::from_slice(&json).unwrap();
    assert_eq!(batch.events.len(), 3);
    assert_eq!(batch.node_id, "pull");

    // 幂等重拉：同 batch_id
    let (status, batch_id2, _) = http(port, "GET", "/collect", Some("tok-123"), b"");
    assert_eq!(status, 200);
    assert_eq!(batch_id2, batch_id);

    // 未拉取批次 ack → 404
    let (status, _, _) = http(
        port,
        "POST",
        "/ack",
        Some("tok-123"),
        br#"{"batch_id":"batch-nope"}"#,
    );
    assert_eq!(status, 404);

    // 正确 ack → 200，事件删除
    let (status, _, _) = http(
        port,
        "POST",
        "/ack",
        Some("tok-123"),
        format!(r#"{{"batch_id":"{batch_id}"}}"#).as_bytes(),
    );
    assert_eq!(status, 200);
    // 重复 ack → 200 幂等
    let (status, _, _) = http(
        port,
        "POST",
        "/ack",
        Some("tok-123"),
        format!(r#"{{"batch_id":"{batch_id}"}}"#).as_bytes(),
    );
    assert_eq!(status, 200);

    // ack 后拉取 → 204（无待传）
    let (status, _, _) = http(port, "GET", "/collect", Some("tok-123"), b"");
    assert_eq!(status, 204);

    // status 端点
    let (status, _, body) = http(port, "GET", "/status", Some("tok-123"), b"");
    assert_eq!(status, 200);
    let s: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(s["mode"], "pull");
    assert_eq!(s["pending_events"], 0);

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = std::fs::remove_dir_all(&dir);
}

fn pick_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[allow(dead_code)]
fn unused_dir_type() -> PathBuf {
    PathBuf::new()
}
