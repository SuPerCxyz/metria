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
        .send_json(json!({ "username": "admin", "password": "change-me-please" }))
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

    let session_id = sessions["sessions"][0]["id"].as_str().unwrap();
    let session_detail: Value = ureq::get(&format!("{base}/api/v1/sessions/{session_id}"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert!(session_detail["session"]["startup_command"].is_null());

    let timeline: Value = ureq::get(&format!("{base}/api/v1/sessions/{session_id}/timeline"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert!(timeline["messages"].is_array());

    // 模型详情最近会话应保留会话模型与 Token 汇总，不能退化为 —/0。
    let model_detail: Value = ureq::get(&format!(
        "{base}/api/v1/models/claude-sonnet-4.5?from={from}&to={to}"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    let recent = &model_detail["recent_sessions"][0];
    assert_eq!(recent["model"], "claude-sonnet-4.5");
    assert_eq!(recent["input_tokens"], 3000);
    assert_eq!(recent["output_tokens"], 800);

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

#[tokio::test(flavor = "multi_thread")]
async fn collector_token_rotate_and_revoke() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    // 注册（testtok bootstrap）
    ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(
            json!({"schema_version": 1, "node_id": "rot-node", "node_name": "n",
            "agent_version": "0.1.0", "protocol_version": 1}),
        )
        .unwrap();

    // Admin 登录
    let token = admin_token(&base);

    // 轮换 → 返回新 token，且旧 token 立即失效
    let rot: Value = ureq::post(&format!(
        "{base}/api/v1/collectors/collector-rot-node/tokens"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .send_json(json!({}))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(rot["ok"], true);
    let new_tok = rot["token"].as_str().unwrap().to_string();

    // env bootstrap token 作为注册凭据始终可用（设计如此）；
    // 轮换后 DB 中的旧 token 记录被吊销。新 token 可鉴权（注册成功）。
    let ok: Value = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", &format!("Bearer {new_tok}"))
        .send_json(
            json!({"schema_version": 1, "node_id": "rot-node", "node_name": "n",
            "agent_version": "0.1.0", "protocol_version": 1}),
        )
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(ok["ok"], true);

    // 吊销 → 全部失效
    let rev: Value = ureq::post(&format!(
        "{base}/api/v1/collectors/collector-rot-node/tokens/revoke"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .send_json(json!({}))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(rev["ok"], true);
    let resp2 = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", &format!("Bearer {new_tok}"))
        .send_json(
            json!({"schema_version": 1, "node_id": "rot-node", "node_name": "n",
            "agent_version": "0.1.0", "protocol_version": 1}),
        )
        .unwrap_err();
    assert!(
        matches!(resp2, ureq::Error::Status(401, _)),
        "吊销后新 token 也应失效"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ingest_enforces_node_collector_identity() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    // 注册（testtok bootstrap）→ 拿到 collector
    ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(
            json!({"schema_version": 1, "node_id": "id-node", "node_name": "n",
            "agent_version": "0.1.0", "protocol_version": 1}),
        )
        .unwrap();

    // Admin 轮换 → 生成持久化 token（带身份）
    let token = admin_token(&base);
    let rot: Value = ureq::post(&format!(
        "{base}/api/v1/collectors/collector-id-node/tokens"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .send_json(json!({}))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(rot["ok"], true);
    let new_tok = rot["token"].as_str().unwrap().to_string();

    // 匹配身份：node=id-node, collector=collector-id-node → 通过
    let good = json!({
        "schema_version": 1, "batch_id": "id-ok", "node_id": "id-node",
        "collector_id": "collector-id-node", "agent_version": "0.1.0",
        "events": [{
            "kind": "usage", "event_id": "blake3:idok1",
            "payload": {"node_id": "id-node", "collector_id": "collector-id-node",
                        "client_id": "claude-code", "model_normalized": "claude-sonnet-4.5",
                        "timestamp": "2026-08-05T01:00:00Z", "input_tokens": 100}
        }]
    });
    let ok: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", &format!("Bearer {new_tok}"))
        .send_json(good)
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(ok["ok"], true, "身份匹配应通过: {ok}");

    // 身份不匹配：batch 声明其他 node → 403
    let bad = json!({
        "schema_version": 1, "batch_id": "id-bad", "node_id": "other-node",
        "collector_id": "collector-id-node", "agent_version": "0.1.0",
        "events": []
    });
    let err = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", &format!("Bearer {new_tok}"))
        .send_json(bad)
        .unwrap_err();
    assert!(
        matches!(err, ureq::Error::Status(403, _)),
        "身份不匹配应被拒绝 403，实际: {err}"
    );

    // 身份不匹配：batch 声明其他 collector → 403
    let bad2 = json!({
        "schema_version": 1, "batch_id": "id-bad2", "node_id": "id-node",
        "collector_id": "collector-other", "agent_version": "0.1.0",
        "events": []
    });
    let err2 = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", &format!("Bearer {new_tok}"))
        .send_json(bad2)
        .unwrap_err();
    assert!(
        matches!(err2, ureq::Error::Status(403, _)),
        "collector 不匹配应被拒绝 403，实际: {err2}"
    );
}

// ============ 节点管理（前端创建 / 安装命令 / 编辑 / 删除） ============

#[tokio::test(flavor = "multi_thread")]
async fn node_management_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);
    let auth = format!("Bearer {token}");

    // 1) 创建节点 → 返回一次性 token
    let created: Value = ureq::post(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .send_json(json!({
            "name": "web-node",
            "ip": "203.0.113.10",
            "description": "web server",
            "labels": ["prod", "app"]
        }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(created["ok"], true);
    assert_eq!(created["ip"], "203.0.113.10");
    let node_id = created["node_id"].as_str().unwrap().to_string();
    let plain_tok = created["token"].as_str().unwrap().to_string();
    assert!(
        plain_tok.starts_with("mct-"),
        "token 前缀 mct-，实际 {plain_tok}"
    );
    assert!(node_id.starts_with("node-"), "node_id 前缀 node-");

    // 2) 列表包含新节点且不泄露明文 token
    let list: Value = ureq::get(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let node = list["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == node_id)
        .expect("列表应包含新建节点");
    assert_eq!(node["name"], "web-node");
    assert_eq!(node["description"], "web server");
    assert_eq!(node["status"], "pending");
    assert_eq!(node["ip"], "203.0.113.10");
    assert!(node.get("token").is_none(), "列表不应返回明文 token");

    // 3) 安装命令 → 生成 Docker/原生两种命令，内嵌 node_id、有效 token，
    //    且 hub_url 使用节点 IP 拼接（未显式填 hub_url 时）
    let inst: Value = ureq::get(&format!("{base}/api/v1/nodes/{node_id}/install"))
        .set("Authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let docker_cmd = inst["docker_command"].as_str().unwrap();
    let native_cmd = inst["native_command"].as_str().unwrap();
    assert!(docker_cmd.contains(node_id.as_str()));
    assert!(docker_cmd.contains("METRIA_AGENT_TOKEN="));
    assert!(docker_cmd.contains("ghcr.io/supercxyz/metria"));
    assert!(docker_cmd.contains("METRIA_HUB_URL=http://203.0.113.10:8080"));
    assert!(native_cmd.contains("api/v1/agent/download"));
    assert!(inst["token"].as_str().unwrap().starts_with("mct-"));

    // 4) 用专属 token 注册 → 节点在线且名称保持为创建时设置值
    let reg: Value = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", &format!("Bearer {plain_tok}"))
        .send_json(json!({
            "schema_version": 1, "node_id": node_id, "node_name": "agent-self-name",
            "agent_version": "0.1.0", "protocol_version": 1
        }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(reg["ok"], true);
    let list2: Value = ureq::get(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let node2 = list2["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == node_id)
        .expect("注册后节点仍在列表");
    assert_eq!(node2["status"], "online");
    assert_eq!(node2["name"], "web-node", "注册不应覆盖用户设置的名称");

    // 5) 编辑节点 → 名称与 IP 生效
    let upd: Value = ureq::put(&format!("{base}/api/v1/nodes/{node_id}"))
        .set("Authorization", &auth)
        .send_json(json!({
            "name": "web-node-renamed",
            "ip": "198.51.100.20",
            "description": "updated desc",
            "labels": ["prod"],
            "hub_url": "http://hub.internal:9090"
        }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(upd["ok"], true);
    let list3: Value = ureq::get(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let node3 = list3["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == node_id)
        .expect("编辑后节点仍在列表");
    assert_eq!(node3["name"], "web-node-renamed");
    assert_eq!(node3["ip"], "198.51.100.20");

    // 5.1) 显式 hub_url 优先于 ip：编辑后 install 命令使用 hub_url
    let inst2: Value = ureq::get(&format!("{base}/api/v1/nodes/{node_id}/install"))
        .set("Authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        inst2["docker_command"]
            .as_str()
            .unwrap()
            .contains("METRIA_HUB_URL=http://hub.internal:9090"),
        "显式 hub_url 应优先于节点 IP"
    );

    // 5.5) 上传一批事件（创建 sources，验证删除时外键级联清理）
    let batch = json!({
        "schema_version": 1,
        "batch_id": "nm-batch-1",
        "node_id": node_id,
        "collector_id": format!("collector-{node_id}"),
        "agent_version": "0.1.0",
        "events": [
            {
                "kind": "source",
                "event_id": "blake3:nmsrc",
                "payload": {
                    "id": "nmsrc-1",
                    "source_id": "nmsrc-1",
                    "node_id": node_id,
                    "collector_id": format!("collector-{node_id}"),
                    "client_id": "claude-code",
                    "adapter_id": "claude",
                    "source_fingerprint": "nmsrc-fp",
                    "source_path_hash": "abc",
                    "status": "healthy",
                    "created_at": "2026-08-05T01:00:00Z"
                }
            }
        ]
    });
    let up: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", &format!("Bearer {plain_tok}"))
        .send_json(batch)
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        up["accepted"].as_array().unwrap().len(),
        1,
        "sources 应被接受，响应: {up}"
    );

    // 6) 删除节点（含 sources）→ 列表消失
    let del: Value = ureq::delete(&format!("{base}/api/v1/nodes/{node_id}"))
        .set("Authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(del["ok"], true);
    let list4: Value = ureq::get(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        list4["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["id"] != node_id),
        "删除后节点不应在列表"
    );

    // 7) 未登录创建节点 → 401
    let err = ureq::post(&format!("{base}/api/v1/nodes"))
        .send_json(json!({"name": "x"}))
        .unwrap_err();
    assert!(matches!(err, ureq::Error::Status(401, _)));
}

#[tokio::test(flavor = "multi_thread")]
async fn node_management_errors() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);
    let auth = format!("Bearer {token}");

    // 空名称 → 400
    let err = ureq::post(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .send_json(json!({"name": "  ", "ip": "203.0.113.1"}))
        .unwrap_err();
    assert!(matches!(err, ureq::Error::Status(400, _)));

    // 缺 ip → 400
    let err = ureq::post(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .send_json(json!({"name": "no-ip-node"}))
        .unwrap_err();
    assert!(matches!(err, ureq::Error::Status(400, _)));

    // 非法 ip → 400
    let err = ureq::post(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .send_json(json!({"name": "bad-ip-node", "ip": "not an ip!"}))
        .unwrap_err();
    assert!(matches!(err, ureq::Error::Status(400, _)));

    // 更新/删除不存在的节点 → 404
    let err = ureq::put(&format!("{base}/api/v1/nodes/nonexistent"))
        .set("Authorization", &auth)
        .send_json(json!({"name": "x", "ip": "203.0.113.1"}))
        .unwrap_err();
    assert!(matches!(err, ureq::Error::Status(404, _)));
    let err = ureq::delete(&format!("{base}/api/v1/nodes/nonexistent"))
        .set("Authorization", &auth)
        .call()
        .unwrap_err();
    assert!(matches!(err, ureq::Error::Status(404, _)));
    let err = ureq::get(&format!("{base}/api/v1/nodes/nonexistent/install"))
        .set("Authorization", &auth)
        .call()
        .unwrap_err();
    assert!(matches!(err, ureq::Error::Status(404, _)));
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_download_returns_binary() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);
    let resp = ureq::get(&format!("{base}/api/v1/agent/download"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .header("content-type")
        .map(|v| v.to_string())
        .unwrap_or_default();
    assert_eq!(ct, "application/octet-stream");
    let mut body = Vec::new();
    resp.into_reader().read_to_end(&mut body).expect("读 body");
    assert!(body.len() > 1000, "二进制应非空");
}

#[tokio::test(flavor = "multi_thread")]
async fn latency_timeseries_buckets_aggregates_and_gapfills() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _state) = spawn_hub(dir.path()).await;

    let reg_resp = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({
            "schema_version": 1, "node_id": "lat-node", "node_name": "lat-node",
            "node_platform": "linux", "node_architecture": "x86_64",
            "agent_version": "0.1.0", "protocol_version": 1
        }))
        .unwrap();
    let reg: Value = reg_resp.into_json().unwrap();
    assert_eq!(reg["ok"], true);

    // 构造带 duration 的 call 事件：01:00 桶两个样本（800ms/1200ms），
    // 01:05 桶一个样本（2000ms），01:30 桶留空以验证缺口补齐。
    let batch = json!({
        "schema_version": 1,
        "batch_id": "latency-batch-1",
        "node_id": "lat-node",
        "collector_id": "collector-lat-node",
        "agent_version": "0.1.0",
        "events": [
            {
                "kind": "call",
                "event_id": "blake3:lat-call-1",
                "payload": {
                    "id": "lat-call-1",
                    "source_call_id": "lc1",
                    "node_id": "lat-node",
                    "collector_id": "collector-lat-node",
                    "client_id": "claude-code",
                    "source_id": "src1",
                    "session_id": "lat-sess",
                    "model_raw": "claude-sonnet-4-5",
                    "model_normalized": "claude-sonnet-4.5",
                    "provider_raw": "anthropic",
                    "provider_normalized": "anthropic",
                    "started_at": "2026-08-05T01:00:05Z",
                    "status": "success",
                    "call_granularity": "call",
                    "duration_ms": 800,
                    "input_tokens": 100,
                    "output_tokens": 50
                }
            },
            {
                "kind": "call",
                "event_id": "blake3:lat-call-2",
                "payload": {
                    "id": "lat-call-2",
                    "source_call_id": "lc2",
                    "node_id": "lat-node",
                    "collector_id": "collector-lat-node",
                    "client_id": "claude-code",
                    "source_id": "src1",
                    "session_id": "lat-sess",
                    "model_raw": "claude-sonnet-4-5",
                    "model_normalized": "claude-sonnet-4.5",
                    "provider_raw": "anthropic",
                    "provider_normalized": "anthropic",
                    "started_at": "2026-08-05T01:00:45Z",
                    "status": "success",
                    "call_granularity": "call",
                    "duration_ms": 1200,
                    "input_tokens": 100,
                    "output_tokens": 50
                }
            },
            {
                "kind": "call",
                "event_id": "blake3:lat-call-3",
                "payload": {
                    "id": "lat-call-3",
                    "source_call_id": "lc3",
                    "node_id": "lat-node",
                    "collector_id": "collector-lat-node",
                    "client_id": "claude-code",
                    "source_id": "src1",
                    "session_id": "lat-sess",
                    "model_raw": "claude-sonnet-4-5",
                    "model_normalized": "claude-sonnet-4.5",
                    "provider_raw": "anthropic",
                    "provider_normalized": "anthropic",
                    "started_at": "2026-08-05T01:05:10Z",
                    "status": "success",
                    "call_granularity": "call",
                    "duration_ms": 2000,
                    "input_tokens": 100,
                    "output_tokens": 50
                }
            }
        ]
    });
    let upload: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch)
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(upload["accepted"].as_array().unwrap().len(), 3);

    let token = admin_token(&base);
    let from = "2026-08-05T01:00:00Z";
    let to = "2026-08-05T01:35:00Z";
    let resp = ureq::get(&format!(
        "{base}/api/v1/usage/latency/timeseries?from={from}&to={to}"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap();
    let body: Value = resp.into_json().unwrap();
    let series = body["series"].as_array().unwrap();

    // 5 分钟粒度，01:00-01:35 共 8 个桶（含补齐），每个 5 分钟
    assert_eq!(series.len(), 8, "应补齐为连续 8 个 5 分钟桶");

    let b0 = &series[0];
    assert_eq!(b0["count"], 2);
    assert_eq!(b0["avg_ms"], 1000);
    assert_eq!(b0["p50_ms"], 800);
    assert_eq!(b0["p95_ms"], 1200);
    assert_eq!(b0["p99_ms"], 1200);

    let b1 = &series[1];
    assert_eq!(b1["count"], 1);
    assert_eq!(b1["avg_ms"], 2000);
    assert_eq!(b1["p50_ms"], 2000);

    // 空桶：count=0，统计字段为 null
    let b2 = &series[2];
    assert_eq!(b2["count"], 0);
    assert!(b2["avg_ms"].is_null());
    assert!(b2["p50_ms"].is_null());
    assert!(b2["p95_ms"].is_null());
    assert!(b2["p99_ms"].is_null());

    // 与 /usage/latency 样本数一致（范围内 3 个 duration 样本）
    let overall: Value = ureq::get(&format!("{base}/api/v1/usage/latency?from={from}&to={to}"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(overall["count"], 3);
}
