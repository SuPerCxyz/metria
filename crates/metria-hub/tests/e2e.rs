//! Hub 端到端集成测试：注册 → 上传（zstd/raw）→ 幂等 → rollup → 查询。
//!
//! 使用真实 HTTP 服务器 + 内存临时 SQLite，验证完整链路。

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
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = metria_hub::api::app_router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), state)
}

fn admin_token(base: &str) -> String {
    let resp = ureq::post(&format!("{base}/api/v1/auth/login"))
        .send_json(json!({ "username": "admin", "password": "metria-admin" }))
        .unwrap();
    resp.into_json::<Value>().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

fn build_batch() -> Value {
    let sid = "e2e-session-1";
    let node = "e2e-node";
    json!({
        "schema_version": 1,
        "batch_id": "e2e-batch-1",
        "node_id": node,
        "collector_id": "collector-e2e-node",
        "agent_version": "0.1.0",
        "events": [
            {
                "kind": "session",
                "event_id": "blake3:session1",
                "payload": {
                    "id": "sess-id-1",
                    "source_session_id": sid,
                    "node_id": node,
                    "collector_id": "collector-e2e-node",
                    "source_id": "src1",
                    "client_id": "claude-code",
                    "started_at": "2026-08-05T01:00:00Z",
                    "status": "ended",
                    "message_count": 4,
                    "tool_call_count": 1,
                    "model_call_count": 2,
                    "created_at": "2026-08-05T01:00:00Z"
                }
            },
            {
                "kind": "call",
                "event_id": "blake3:call1",
                "payload": {
                    "id": "call-1",
                    "source_call_id": "c1",
                    "node_id": node,
                    "collector_id": "collector-e2e-node",
                    "client_id": "claude-code",
                    "source_id": "src1",
                    "session_id": "sess-id-1",
                    "model_raw": "claude-sonnet-4-5",
                    "model_normalized": "claude-sonnet-4.5",
                    "provider_raw": "anthropic",
                    "provider_normalized": "anthropic",
                    "started_at": "2026-08-05T01:00:05Z",
                    "status": "success",
                    "call_granularity": "call",
                    "input_tokens": 1000,
                    "output_tokens": 500
                }
            },
            {
                "kind": "call",
                "event_id": "blake3:call2",
                "payload": {
                    "id": "call-2",
                    "source_call_id": "c2",
                    "node_id": node,
                    "collector_id": "collector-e2e-node",
                    "client_id": "claude-code",
                    "source_id": "src1",
                    "session_id": "sess-id-1",
                    "model_raw": "claude-sonnet-4-5",
                    "model_normalized": "claude-sonnet-4.5",
                    "provider_raw": "anthropic",
                    "provider_normalized": "anthropic",
                    "started_at": "2026-08-05T01:00:10Z",
                    "status": "success",
                    "call_granularity": "call",
                    "input_tokens": 2000,
                    "output_tokens": 300
                }
            },
            {
                "kind": "usage",
                "event_id": "blake3:usage1",
                "payload": {
                    "event_id": "blake3:usage1",
                    "schema_version": 1,
                    "node_id": node,
                    "collector_id": "collector-e2e-node",
                    "source_id": "src1",
                    "client_id": "claude-code",
                    "adapter_id": "claude-code",
                    "adapter_version": "0.1.0",
                    "session_id": sid,
                    "model_call_id": "call-1",
                    "timestamp": "2026-08-05T01:00:05Z",
                    "model_raw": "claude-sonnet-4-5",
                    "model_normalized": "claude-sonnet-4.5",
                    "usage": { "input": 1000, "output": 500, "cache_read": 100, "cache_write": 50, "reasoning": null },
                    "cost": { "reported_micro_usd": null, "calculated_micro_usd": 33218, "estimated_micro_usd": null, "pricing_rule_id": null, "pricing_snapshot_id": null },
                    "quality": { "usage_source": "reported", "granularity": "call", "confidence": 1.0 }
                }
            },
            {
                "kind": "usage",
                "event_id": "blake3:usage2",
                "payload": {
                    "event_id": "blake3:usage2",
                    "schema_version": 1,
                    "node_id": node,
                    "collector_id": "collector-e2e-node",
                    "source_id": "src1",
                    "client_id": "claude-code",
                    "adapter_id": "claude-code",
                    "adapter_version": "0.1.0",
                    "session_id": sid,
                    "model_call_id": "call-2",
                    "timestamp": "2026-08-05T01:00:10Z",
                    "model_raw": "claude-sonnet-4-5",
                    "model_normalized": "claude-sonnet-4.5",
                    "usage": { "input": 2000, "output": 300, "cache_read": 0, "cache_write": 0, "reasoning": null },
                    "cost": { "reported_micro_usd": null, "calculated_micro_usd": 11100, "estimated_micro_usd": null, "pricing_rule_id": null, "pricing_snapshot_id": null },
                    "quality": { "usage_source": "reported", "granularity": "call", "confidence": 1.0 }
                }
            },
            {
                "kind": "traffic",
                "event_id": "blake3:traffic1",
                "payload": {
                    "id": "t1",
                    "model_call_id": "call-1",
                    "node_id": node,
                    "client_id": "claude-code",
                    "estimated_request_wire_bytes": 5000,
                    "estimated_response_wire_bytes": 2000,
                    "estimated_total_wire_bytes": 7000,
                    "lower_bound_bytes": 6000,
                    "upper_bound_bytes": 9000,
                    "estimation_source": "partial_reconstruction",
                    "context_transport_mode": "full_context",
                    "cache_transport_behavior": "full_content_sent",
                    "request_reconstruction_quality": "partial",
                    "response_reconstruction_quality": "complete",
                    "confidence": 0.6,
                    "calculated_at": "2026-08-05T01:00:06Z"
                }
            },
            {
                "kind": "traffic",
                "event_id": "blake3:traffic2",
                "payload": {
                    "id": "t2",
                    "model_call_id": "call-2",
                    "node_id": node,
                    "client_id": "claude-code",
                    "estimated_request_wire_bytes": 3000,
                    "estimated_response_wire_bytes": 1000,
                    "estimated_total_wire_bytes": 4000,
                    "lower_bound_bytes": 3500,
                    "upper_bound_bytes": 5000,
                    "estimation_source": "partial_reconstruction",
                    "context_transport_mode": "full_context",
                    "cache_transport_behavior": "full_content_sent",
                    "request_reconstruction_quality": "partial",
                    "response_reconstruction_quality": "complete",
                    "confidence": 0.6,
                    "calculated_at": "2026-08-05T01:00:11Z"
                }
            }
        ]
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn full_ingest_rollup_query_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _state) = spawn_hub(dir.path()).await;

    // 健康检查
    let health: Value = ureq::get(&format!("{base}/healthz"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(health["status"], "ok");

    // 注册（需 collector token）
    let reg_resp = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({
            "schema_version": 1, "node_id": "e2e-node", "node_name": "e2e-node",
            "node_platform": "linux", "node_architecture": "x86_64",
            "agent_version": "0.1.0", "protocol_version": 1
        }))
        .unwrap();
    let reg: Value = reg_resp.into_json().unwrap();
    assert_eq!(reg["ok"], true);
    assert_eq!(reg["collector_id"], "collector-e2e-node");

    // 上传批次
    let batch = build_batch();
    let upload: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch.clone())
        .unwrap()
        .into_json()
        .unwrap();
    if upload["ok"] != true || upload["accepted"].as_array().unwrap().len() != 7 {
        panic!("upload 未全部接受: {upload}");
    }

    // 幂等：重复上传 → duplicate
    let upload2: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch.clone())
        .unwrap()
        .into_json()
        .unwrap();
    if upload2["duplicate"].as_array().unwrap().len() != 7 {
        panic!("第二次上传未全部去重: {upload2}");
    }
    if !upload2["accepted"].as_array().unwrap().is_empty() {
        panic!("第二次上传不应接受新事件: {upload2}");
    }

    // 查询：overview
    let token = admin_token(&base);
    let from = "2026-08-01T00:00:00Z";
    let to = "2026-08-06T00:00:00Z";
    let overview: Value = ureq::get(&format!("{base}/api/v1/overview?from={from}&to={to}"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(overview["model_calls"], 2);
    assert_eq!(overview["sessions"], 1);
    assert_eq!(overview["nodes"], 1);
    assert_eq!(overview["input_tokens"], 3000);
    assert_eq!(overview["output_tokens"], 800);
    assert_eq!(overview["cache_read_tokens"], 100);
    assert_eq!(overview["cache_write_tokens"], 50);
    assert_eq!(overview["calculated_cost_micro_usd"], 33218 + 11100);
    assert_eq!(overview["estimated_total_bytes"], 7000 + 4000);

    // sessions 列表
    let sessions: Value = ureq::get(&format!("{base}/api/v1/sessions?from={from}&to={to}"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(sessions["sessions"].as_array().unwrap().len(), 1);

    // 节点下客户端
    let clients: Value = ureq::get(&format!("{base}/api/v1/nodes/e2e-node/clients"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(clients["sources"].as_array().unwrap().len(), 0);

    // 未登录访问受保护端点 → 401
    let resp = ureq::get(&format!("{base}/api/v1/nodes"))
        .call()
        .unwrap_err();
    assert!(matches!(resp, ureq::Error::Status(401, _)));

    // rollup 直接校验
    let c = _state.db.conn();
    let calls: i64 = c
        .query_row(
            "SELECT SUM(model_call_count) FROM hourly_rollups",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(calls, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_batch_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    // 超出事件数上限
    let events: Vec<Value> = (0..300)
        .map(|i| json!({"kind": "usage", "event_id": format!("blake3:e{i}"), "payload": {}}))
        .collect();
    let batch = json!({
        "schema_version": 1, "batch_id": "big", "node_id": "n", "collector_id": "c",
        "agent_version": "0.1.0", "events": events
    });
    let resp = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch)
        .unwrap_err();
    assert!(matches!(resp, ureq::Error::Status(400, _)));
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_token_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    let resp = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer wrong-token")
        .send_json(json!({"schema_version": 1, "node_id": "n", "node_name": "n", "agent_version": "0.1.0", "protocol_version": 1}))
        .unwrap_err();
    assert!(matches!(resp, ureq::Error::Status(401, _)));
}

#[tokio::test(flavor = "multi_thread")]
async fn collector_token_has_seven_day_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    // 注册写入 token（有效期 7 天）
    let resp = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(
            json!({"schema_version": 1, "node_id": "tok-node", "node_name": "n",
            "agent_version": "0.1.0", "protocol_version": 1}),
        )
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 未过期 token 可鉴权
    let ok = state.db.verify_collector_token("testtok");
    assert!(ok.is_some(), "未过期 token 应有效");

    // 手工将 token 置为过期 → 鉴权失败
    {
        let c = state.db.conn();
        let now = chrono::Utc::now().to_rfc3339();
        c.execute(
            "UPDATE collector_tokens SET expires_at = ?1 WHERE status = 'active'",
            [&now],
        )
        .unwrap();
    }
    assert!(
        state.db.verify_collector_token("testtok").is_none(),
        "过期 token 应失效"
    );

    // 过期后重新注册 → 刷新有效期（upsert 语义）
    let resp2 = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(
            json!({"schema_version": 1, "node_id": "tok-node", "node_name": "n",
            "agent_version": "0.1.0", "protocol_version": 1}),
        )
        .unwrap();
    assert_eq!(resp2.status(), 200);
    assert!(
        state.db.verify_collector_token("testtok").is_some(),
        "重新注册应刷新有效期"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn incompatible_protocol_version_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    // 不兼容的协议版本 → 400，不静默注册
    let resp = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(
            json!({"schema_version": 1, "node_id": "proto-node", "node_name": "n",
            "agent_version": "0.1.0", "protocol_version": 999}),
        )
        .unwrap_err();
    assert!(matches!(resp, ureq::Error::Status(400, _)));
}

#[tokio::test(flavor = "multi_thread")]
async fn deep_nested_event_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    // 深度超过 32 的事件 → 400
    let mut deep = json!({"leaf": "x"});
    for _ in 0..40 {
        deep = json!({"wrap": deep});
    }
    let batch = json!({
        "schema_version": 1, "batch_id": "deep", "node_id": "n", "collector_id": "c",
        "agent_version": "0.1.0",
        "events": [{"kind": "usage", "event_id": "blake3:deep1", "payload": deep}]
    });
    let resp = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch)
        .unwrap_err();
    assert!(matches!(resp, ureq::Error::Status(400, _)));
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_single_event_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    // 单事件 payload 超过 2MiB → 400
    let big = "x".repeat(3 * 1024 * 1024);
    let batch = json!({
        "schema_version": 1, "batch_id": "bigevent", "node_id": "n", "collector_id": "c",
        "agent_version": "0.1.0",
        "events": [{"kind": "usage", "event_id": "blake3:big1", "payload": {"blob": big}}]
    });
    match ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch)
    {
        // 3MiB 未压缩请求体被 axum 默认 limit 拒绝（413）或解压后超限（400）
        Ok(r) => panic!("应被拒绝，实际成功: status {}", r.status()),
        Err(ureq::Error::Status(400, _)) => {}
        Err(ureq::Error::Status(413, _)) => {}
        Err(e) => panic!("应返回 400/413，实际: {e}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn zstd_bomb_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    // 高度可压缩的爆炸负载：解压后远超 8MiB 上限
    let payload = "compressme-".repeat(3 * 1024 * 1024);
    let batch = json!({
        "schema_version": 1, "batch_id": "bomb", "node_id": "n", "collector_id": "c",
        "agent_version": "0.1.0",
        "events": [{"kind": "usage", "event_id": "blake3:bomb1", "payload": {"blob": payload}}]
    });
    let raw = serde_json::to_vec(&batch).unwrap();
    let mut enc = zstd::stream::Encoder::new(Vec::new(), 3).unwrap();
    std::io::Write::write_all(&mut enc, &raw).unwrap();
    let compressed = enc.finish().unwrap();

    let resp = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .set("Content-Encoding", "zstd")
        .send_bytes(&compressed);
    match resp {
        Ok(r) => panic!("应被拒绝，实际成功: status {}", r.status()),
        Err(ureq::Error::Status(400, _)) => {}
        Err(ureq::Error::Status(413, _)) => {}
        Err(e) => panic!("应返回 400/413，实际: {e}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn partial_success_keeps_good_events() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    // 混合批次：2 个合法 + 1 个非法类型 → accepted=2, failed=1, 合法事件落库
    let batch = json!({
        "schema_version": 1, "batch_id": "partial", "node_id": "n", "collector_id": "c",
        "agent_version": "0.1.0",
        "events": [
            {"kind": "session", "event_id": "blake3:ok1", "payload": {
                "source_session_id": "ps-1", "started_at": "2026-08-06T01:00:00Z",
                "node_id": "n", "collector_id": "c", "client_id": "claude-code",
                "source_id": "s", "status": "active"
            }},
            {"kind": "call", "event_id": "blake3:ok2", "payload": {
                "started_at": "2026-08-06T01:01:00Z", "node_id": "n", "collector_id": "c",
                "client_id": "claude-code", "source_id": "s"
            }},
            {"kind": "no-such-kind", "event_id": "blake3:bad1", "payload": {}}
        ]
    });
    let resp: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch.clone())
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        resp["accepted"].as_array().unwrap().len(),
        2,
        "合法事件应接受: {resp}"
    );
    assert_eq!(
        resp["failed"].as_array().unwrap().len(),
        1,
        "非法事件应失败: {resp}"
    );
    assert_eq!(resp["failed"][0]["event_id"], "blake3:bad1");
    assert_eq!(resp["failed"][0]["retryable"], false);

    // 合法事件确实落库
    let token = admin_token(&base);
    let sessions: Value = ureq::get(&format!(
        "{base}/api/v1/sessions?from=2026-08-01T00:00:00Z&to=2026-08-06T23:59:59Z"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert!(!sessions["sessions"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_records_clock_skew() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    // 注册
    ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(
            json!({"schema_version": 1, "node_id": "skew-node", "node_name": "n",
            "agent_version": "0.1.0", "protocol_version": 1}),
        )
        .unwrap();
    // 心跳：agent 时钟比 Hub 慢 300 秒
    let agent_clock = (chrono::Utc::now() - chrono::Duration::seconds(300)).to_rfc3339();
    ureq::post(&format!("{base}/api/v1/collectors/heartbeat"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({"schema_version": 1, "node_id": "skew-node", "collector_id": "collector-skew-node",
            "spool_pending_events": 0, "spool_size_bytes": 0, "source_count": 0, "agent_clock": agent_clock}))
        .unwrap();
    let skew: i64 = state
        .db
        .conn()
        .query_row(
            "SELECT clock_skew_seconds FROM collectors WHERE id = 'collector-skew-node'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        (skew - 300).abs() <= 5,
        "clock_skew 应约 300 秒，实际 {skew}"
    );
}
