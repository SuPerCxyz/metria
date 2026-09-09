//! 无状态轮询端到端：扫描 → 上传 → 推游标 →（Hub 离线期间追加数据）→ 恢复补齐 → 重复扫描幂等去重。
//!
//! 直接驱动 `run_cycle`（与 polling_loop 单轮逻辑一致），Hub 为真实 HTTP 服务。

use std::collections::HashMap;
use std::path::PathBuf;

use metria_adapter_api::ScanIdentity;
use metria_agent::config::AgentConfig;
use metria_agent::scanner::Scanner;
use metria_agent::stateless::run_cycle;
use metria_agent::wire::HubClient;
use metria_core::model::SourceCursor;
use metria_hub::api::AppState;
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_protocol::CursorEntry;
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

fn agent_cfg(root: &std::path::Path, data_dir: &std::path::Path) -> AgentConfig {
    AgentConfig {
        node_id: String::new(),
        node_name: "stateless-e2e".into(),
        hub_url: None,
        token: None,
        listen_port: 0,
        claude_path: Some(root.to_path_buf()),
        codex_path: None,
        opencode_path: None,
        content_mode: metria_core::ContentMode::Metadata,
        data_dir: data_dir.to_path_buf(),
        max_pending_events: 1000,
        max_spool_bytes: 10_000_000,
        batch_max_events: 256,
        batch_max_bytes: 1024 * 1024,
        scan_interval_seconds: 10,
        reconcile_interval_seconds: 300,
        heartbeat_interval_seconds: 60,
        upload_interval_seconds: 15,
        poll_interval_seconds: 60,
        token_refresh_interval_seconds: 6 * 24 * 3600,
        log_filter: "error".into(),
    }
}

fn to_map(entries: Vec<CursorEntry>) -> HashMap<String, SourceCursor> {
    entries
        .into_iter()
        .filter_map(|e| {
            serde_json::from_str(&e.cursor_json)
                .ok()
                .map(|c| (e.source_id, c))
        })
        .collect()
}

fn overview_input_tokens(base: &str, admin: &str) -> i64 {
    let resp = ureq::get(&format!(
        "{base}/api/v1/overview?from=2026-08-05T00:00:00Z&to=2026-08-05T23:59:59Z"
    ))
    .set("Authorization", &format!("Bearer {admin}"))
    .call()
    .unwrap();
    let body: Value = resp.into_json().unwrap();
    body["input_tokens"].as_i64().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn stateless_poll_backfills_after_hub_gap_without_duplicates() {
    // 客户端 fixture：golden 会话文件
    let fixture_root = tempfile::tempdir().unwrap();
    let golden = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude/golden_full.jsonl"),
    )
    .unwrap();
    let session = fixture_root.path().join("session.jsonl");
    std::fs::write(&session, &golden).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let (hub, _state) = spawn_hub(dir.path()).await;
    let admin = admin_token(&hub);

    // 创建节点 + 注册
    let created: Value = ureq::post(&format!("{hub}/api/v1/nodes"))
        .set("Authorization", &format!("Bearer {admin}"))
        .send_json(json!({ "name": "stateless-node", "ip": "203.0.113.7" }))
        .unwrap()
        .into_json()
        .unwrap();
    let node_id = created["node_id"].as_str().unwrap().to_string();
    let node_token = created["token"].as_str().unwrap().to_string();
    let reg: Value = ureq::post(&format!("{hub}/api/v1/collectors/register"))
        .set("Authorization", &format!("Bearer {node_token}"))
        .send_json(json!({
            "schema_version": 1, "node_id": node_id, "node_name": "stateless-node",
            "agent_version": "0.4.0", "protocol_version": 1
        }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(reg["ok"], true);

    let client = HubClient::new(&hub, Some(node_token));
    let identity = ScanIdentity {
        node_id: node_id.clone(),
        collector_id: reg["collector_id"].as_str().unwrap().to_string(),
    };
    let cfg = agent_cfg(fixture_root.path(), dir.path());
    let scanner = Scanner::new(cfg.clone(), identity.clone());

    // ---- 周期 1（Hub 可达）：全量扫描 + 上传 + 推游标
    let empty = client.fetch_cursors().unwrap().unwrap();
    assert!(empty.is_empty());
    let stats1 = run_cycle(&scanner, &cfg, &client, &identity, &HashMap::new()).unwrap();
    assert!(stats1.events > 0, "周期 1 应产生事件");

    // Hub 游标已推进
    let after = client.fetch_cursors().unwrap().unwrap();
    assert_eq!(after.len(), 1, "应有一条来源游标");

    let input1 = overview_input_tokens(&hub, &admin);
    assert!(input1 > 0);

    // ---- Hub 离线：客户端继续产生数据（追加一条 assistant usage 行）
    let mut append = String::new();
    for line in golden.lines() {
        if let Ok(mut v) = serde_json::from_str::<Value>(line) {
            if v["type"] == "assistant" && v["message"]["usage"]["input_tokens"].is_number() {
                v["uuid"] = json!("entry-appended-1");
                v["message"]["id"] = json!("msg-appended-1");
                v["timestamp"] = json!("2026-08-05T02:00:00.000Z");
                append = v.to_string();
                break;
            }
        }
    }
    assert!(!append.is_empty(), "fixture 中应有 assistant usage 行");
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&session)
            .unwrap();
        use std::io::Write;
        writeln!(f, "{append}").unwrap();
    }

    // ---- Hub 恢复：拉最新游标补齐（周期 2）
    let cursors2 = to_map(client.fetch_cursors().unwrap().unwrap());
    let stats2 = run_cycle(&scanner, &cfg, &client, &identity, &cursors2).unwrap();
    assert!(stats2.events > 0, "补齐周期应产生增量事件");

    let input2 = overview_input_tokens(&hub, &admin);
    assert_eq!(
        input2 - input1,
        32000,
        "补齐应精确累加追加的 usage（32000 input tokens）"
    );

    // ---- 模拟崩溃前重复扫描（同一旧游标再跑一轮）：全部 duplicate，数据不变
    let cursors2b = to_map(client.fetch_cursors().unwrap().unwrap());
    let _stats3 = run_cycle(&scanner, &cfg, &client, &identity, &cursors2b).unwrap();
    let input3 = overview_input_tokens(&hub, &admin);
    assert_eq!(input3, input2, "重复扫描应被 Hub event_id 幂等去重");
}
