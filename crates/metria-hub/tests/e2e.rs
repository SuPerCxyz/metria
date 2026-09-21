//! Hub 端到端集成测试：注册 → 上传（zstd/raw）→ 幂等 → rollup → 查询。
//!
//! 使用真实 HTTP 服务器 + 内存临时 SQLite，验证完整链路。
#![recursion_limit = "256"]

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

async fn spawn_hub(dir: &std::path::Path) -> (String, AppState) {
    let db = HubDb::open(&test_cfg(dir)).expect("open db");
    db.apply_migrations().expect("migrate");
    let state = AppState {
        db: db.clone(),
        cfg: test_cfg(dir),
        sse: metria_hub::api::SseHub::new(),
        sessions: Default::default(),
        oidc: Default::default(),
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
                    "project_id": "demo-project",
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
                    "project_id": "demo-project",
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
                    "project_id": "demo-project",
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
                    "usage": { "input": 1000, "output": 500, "cache_read": 100, "cache_write": 50, "reasoning": 10 },
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
    if upload["ok"] != true || upload["accepted"].as_array().unwrap().len() != 5 {
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
    assert_eq!(overview["reasoning_tokens"], 10);
    assert_eq!(overview["projects"], 1);
    assert_eq!(overview["calculated_cost_micro_usd"], 33218 + 11100);
    assert!(overview.get("estimated_total_bytes").is_none());
    assert_eq!(overview["pricing_coverage"]["priced_calls"], 2);
    assert_eq!(overview["pricing_coverage"]["total_calls"], 2);
    assert_eq!(overview["token_calls"], 2);
    assert!(overview.get("traffic_coverage").is_none());
    // 消息/工具按明细统计：该夹具没有明细事件，因此为 0（不再沿用会话级计数）。
    assert_eq!(overview["message_count"], 0);
    assert_eq!(overview["tool_call_count"], 0);
    assert_eq!(overview["user_message_count"], 0);
    assert_eq!(
        overview["session_duration_ms"], 10_000,
        "会话跨度 = 开始(01:00:00) 到最后活动(01:00:10)"
    );
    assert!(overview["active_duration_ms"].is_null());

    let filter_options: Value = ureq::get(&format!(
        "{base}/api/v1/usage/filter-options?from={from}&to={to}"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert!(filter_options["agents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == "claude-code"));
    assert!(filter_options["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == "claude-sonnet-4.5"));
    assert!(filter_options["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == "e2e-node"));

    // 时间范围外：Agent 与模型候选为空，节点仍全部列出（节点不按范围收窄）。
    let out_of_range_options: Value = ureq::get(&format!(
        "{base}/api/v1/usage/filter-options?from=2026-08-10T00:00:00Z&to=2026-08-11T00:00:00Z"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert!(out_of_range_options["agents"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(out_of_range_options["models"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(out_of_range_options["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == "e2e-node"));

    let project_overview: Value = ureq::get(&format!(
        "{base}/api/v1/overview?from={from}&to={to}&project_id=demo-project"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(project_overview["model_calls"], 2);

    let project_series: Value = ureq::get(&format!(
        "{base}/api/v1/usage/timeseries?from=2026-08-05T01:00:00Z&to=2026-08-05T01:01:00Z&project_id=demo-project&dim=model"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(
        project_series["series"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["model_calls"].as_i64().unwrap())
            .sum::<i64>(),
        2
    );

    let heatmap: Value = ureq::get(&format!(
        "{base}/api/v1/usage/heatmap?from={from}&to={to}&timezone=Asia%2FShanghai"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(heatmap["cells"].as_array().unwrap().len(), 168);
    assert_eq!(heatmap["timezone"], "Asia/Shanghai");

    let daily: Value = ureq::get(&format!(
        "{base}/api/v1/usage/daily?from={from}&to={to}&timezone=UTC"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(daily["series"].as_array().unwrap().len(), 5);
    // 总 Token 含缓存读写：3000 + 800 + 10 + 100 + 50
    assert_eq!(daily["series"][4]["tokens"], 3960);

    let excluded_project: Value = ureq::get(&format!(
        "{base}/api/v1/overview?from={from}&to={to}&project_id=missing"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(excluded_project["model_calls"], 0);

    // 图表图例隐藏维度后，汇总与时间序列都应排除对应 Agent/模型。
    let excluded_overview: Value = ureq::get(&format!(
        "{base}/api/v1/overview?from={from}&to={to}&exclude_client_ids=claude-code"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(excluded_overview["model_calls"], 0);
    assert_eq!(excluded_overview["input_tokens"], 0);
    assert_eq!(excluded_overview["agent_tools"], 0);
    assert_eq!(excluded_overview["models"], 0);
    assert_eq!(excluded_overview["failed_calls"], 0);
    assert!(excluded_overview["duration_p50_ms"].is_null());
    assert_eq!(excluded_overview["cache_savings_micro_usd"], 0);
    for field in ["nodes", "collectors", "collectors_online"] {
        assert_eq!(excluded_overview[field], overview[field]);
    }
    assert_eq!(excluded_overview["projects"], 0);

    let series: Value = ureq::get(&format!(
        "{base}/api/v1/usage/timeseries?from={from}&to={to}&dim=model"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    let model_point = series["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|point| point["dimension"] == "claude-sonnet-4.5")
        .unwrap();
    assert_eq!(model_point["reasoning_tokens"], 10);

    let breakdown: Value = ureq::get(&format!(
        "{base}/api/v1/usage/breakdown?from={from}&to={to}&dim=model"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(breakdown["by"][0]["reasoning_tokens"], 10);
    assert_eq!(breakdown["by"][0]["cost_micro_usd"], 44318);

    let client_detail: Value = ureq::get(&format!(
        "{base}/api/v1/clients/claude-code?from={from}&to={to}"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(client_detail["by_node"][0]["cost_micro_usd"], 44318);
    assert_eq!(client_detail["by_project"][0]["cost_micro_usd"], 44318);

    let client_models: Value = ureq::get(&format!("{base}/api/v1/clients/claude-code/models"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(client_models["models"][0]["cost_micro_usd"], 44318);

    let node_detail: Value =
        ureq::get(&format!("{base}/api/v1/nodes/e2e-node?from={from}&to={to}"))
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
    assert_eq!(node_detail["by_model"][0]["cost_micro_usd"], 44318);
    assert_eq!(node_detail["by_project"][0]["cost_micro_usd"], 44318);

    let excluded_series: Value = ureq::get(&format!(
        "{base}/api/v1/usage/timeseries?from={from}&to={to}&dim=model&exclude_models=claude-sonnet-4.5"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert!(excluded_series["series"]
        .as_array()
        .unwrap()
        .iter()
        .all(|point| point["model_calls"] == 0));

    // sessions 列表
    let sessions: Value = ureq::get(&format!("{base}/api/v1/sessions?from={from}&to={to}"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(sessions["sessions"].as_array().unwrap().len(), 1);

    let filtered_sessions: Value = ureq::get(&format!(
        "{base}/api/v1/sessions?from={from}&to={to}&client_id=claude-code&model=claude-sonnet-4.5&project_id=demo-project"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(filtered_sessions["sessions"].as_array().unwrap().len(), 1);

    let missing_sessions: Value = ureq::get(&format!(
        "{base}/api/v1/sessions?from={from}&to={to}&project_id=missing"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert!(missing_sessions["sessions"].as_array().unwrap().is_empty());

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
async fn unpriced_dimension_cost_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;

    ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({
            "schema_version": 1, "node_id": "e2e-node", "node_name": "e2e-node",
            "node_platform": "linux", "node_architecture": "x86_64",
            "agent_version": "0.1.0", "protocol_version": 1
        }))
        .unwrap();

    let mut batch = build_batch();
    batch["batch_id"] = json!("unpriced-batch");
    for event in batch["events"].as_array_mut().unwrap() {
        if event["kind"] == "call" || event["kind"] == "usage" {
            event["payload"]["model_raw"] = json!("unpriced-model");
            event["payload"]["model_normalized"] = json!("unpriced-model");
        }
        if event["kind"] == "usage" {
            event["payload"]["cost"]["reported_micro_usd"] = Value::Null;
            event["payload"]["cost"]["calculated_micro_usd"] = Value::Null;
            event["payload"]["cost"]["estimated_micro_usd"] = Value::Null;
        }
    }
    let upload: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch)
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(upload["accepted"].as_array().unwrap().len(), 5);

    let token = admin_token(&base);
    let from = "2026-08-01T00:00:00Z";
    let to = "2026-08-06T00:00:00Z";
    let breakdown: Value = ureq::get(&format!(
        "{base}/api/v1/usage/breakdown?from={from}&to={to}&dim=model"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert!(breakdown["by"][0]["cost_micro_usd"].is_null());

    let client_detail: Value = ureq::get(&format!(
        "{base}/api/v1/clients/claude-code?from={from}&to={to}"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert!(client_detail["by_node"][0]["cost_micro_usd"].is_null());

    let client_models: Value = ureq::get(&format!("{base}/api/v1/clients/claude-code/models"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert!(client_models["models"][0]["cost_micro_usd"].is_null());

    let node_detail: Value =
        ureq::get(&format!("{base}/api/v1/nodes/e2e-node?from={from}&to={to}"))
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
    assert!(node_detail["by_model"][0]["cost_micro_usd"].is_null());
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
            "labels": ["prod", "app"],
            "platform": "linux",
            "architecture": "amd64"
        }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(created["ok"], true);
    assert_eq!(created["ip"], "203.0.113.10");
    assert_eq!(created["platform"], "linux");
    assert_eq!(created["architecture"], "amd64");
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
    assert!(docker_cmd.contains("METRIA_HUB_URL='http://203.0.113.10:8080'"));
    assert!(native_cmd.contains(&format!("/api/v1/nodes/{node_id}/agent/download")));
    assert!(!native_cmd.contains("Authorization"));
    assert!(native_cmd.contains("systemctl enable --now metria-agent.service"));
    assert!(native_cmd.contains("EnvironmentFile=/etc/metria/metria-agent.env"));
    assert!(!native_cmd.contains("nohup"));
    assert!(inst["token"].as_str().unwrap().starts_with("mct-"));

    // 前端环境地址可覆盖历史节点配置，下载命令不包含 token。
    let dynamic: Value = ureq::get(&format!(
        "{base}/api/v1/nodes/{node_id}/install?hub_url=http%3A%2F%2Ffrontend.example%3A8080"
    ))
    .set("Authorization", &auth)
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(dynamic["hub_url"], "http://frontend.example:8080");
    assert!(dynamic["native_command"]
        .as_str()
        .unwrap()
        .contains("http://frontend.example:8080"));

    // 节点专属下载地址公开可读，按节点架构选择当前测试二进制。
    let resp = ureq::get(&format!("{base}/api/v1/nodes/{node_id}/agent/download"))
        .call()
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.header("content-type"),
        Some("application/octet-stream")
    );

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
            .contains("METRIA_HUB_URL='http://hub.internal:9090'"),
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

    // Windows 仅支持 amd64 → arm64 明确拒绝
    let err = ureq::post(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .send_json(json!({
            "name": "windows-arm64-node",
            "ip": "203.0.113.2",
            "platform": "windows",
            "architecture": "arm64"
        }))
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
async fn node_install_preserves_windows_target() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);
    let auth = format!("Bearer {token}");

    let created: Value = ureq::post(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &auth)
        .send_json(json!({
            "name": "windows-node",
            "ip": "198.51.100.22",
            "platform": "windows",
            "architecture": "amd64"
        }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(created["platform"], "windows");
    assert_eq!(created["architecture"], "amd64");

    let node_id = created["node_id"].as_str().unwrap();
    let install: Value = ureq::get(&format!("{base}/api/v1/nodes/{node_id}/install"))
        .set("Authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(install["platform"], "windows");
    assert_eq!(install["architecture"], "amd64");
    assert_eq!(install["agent_asset"], "metria-windows-amd64.exe");
    assert!(install["native_command"]
        .as_str()
        .unwrap()
        .contains("Invoke-WebRequest"));
    assert!(install["native_command"]
        .as_str()
        .unwrap()
        .contains(&format!("/api/v1/nodes/{node_id}/agent/download")));
    assert!(install["native_command"]
        .as_str()
        .unwrap()
        .contains("Register-ScheduledTask"));
    assert!(install["native_command"]
        .as_str()
        .unwrap()
        .contains("$installDir = Join-Path $env:LOCALAPPDATA 'Metria'"));
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_download_returns_binary() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _) = spawn_hub(dir.path()).await;
    let resp = ureq::get(&format!("{base}/api/v1/agent/download"))
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
    let (base, state) = spawn_hub(dir.path()).await;

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
                    "first_byte_at": "2026-08-05T01:00:05.100Z",
                    "first_token_at": "2026-08-05T01:00:05.200Z",
                    "first_response_at": "2026-08-05T01:00:05.200Z",
                    "last_output_at": "2026-08-05T01:00:05.700Z",
                    "completed_at": "2026-08-05T01:00:05.800Z",
                    "first_byte_latency_ms": 100,
                    "ttft_ms": 200,
                    "generation_duration_ms": 500,
                    "output_tokens_per_second_milli": 83333,
                    "observability_source": "runtime_http",
                    "observability_quality": "observed",
                    "observed_request_payload_bytes": 1200,
                    "observed_response_payload_bytes": 2400,
                    "observed_request_wire_bytes": 1300,
                    "observed_response_wire_bytes": 2500,
                    "timing_source": "test_event_timestamps",
                    "timing_quality": "observed",
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
                    "first_response_at": "2026-08-05T01:00:45.300Z",
                    "completed_at": "2026-08-05T01:00:46.200Z",
                    "timing_source": "test_event_timestamps",
                    "timing_quality": "observed",
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

    let performance: Value = ureq::get(&format!(
        "{base}/api/v1/usage/performance?from={from}&to={to}"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(performance["total_calls"], 3);
    assert_eq!(performance["ttft"]["count"], 2);
    assert_eq!(performance["ttft"]["avg_ms"], 250);
    // 生成耗时与输出速度只来自运行时观测字段：仅 call-1 带显式字段。
    assert_eq!(performance["generation"]["count"], 1);
    assert_eq!(performance["generation"]["avg_ms"], 500);
    assert_eq!(performance["output_speed"]["count"], 1);
    let avg_speed = performance["output_speed"]["avg_tokens_per_second"]
        .as_f64()
        .unwrap();
    assert!(
        (avg_speed - 83.333).abs() < 0.1,
        "日志条目时间戳不得推导速度；avg_speed={avg_speed}"
    );

    // 每指标来源分布：运行时观测与日志推导分别统计
    let ttft_sources = performance["ttft"]["sources"].as_array().unwrap();
    assert_eq!(ttft_sources.len(), 2, "两类来源应分别统计");
    let count_of = |name: &str| {
        ttft_sources
            .iter()
            .find(|item| item["source"] == name)
            .and_then(|item| item["count"].as_i64())
            .unwrap_or(0)
    };
    assert_eq!(count_of("runtime_http"), 1);
    assert_eq!(count_of("test_event_timestamps"), 1);
    assert_eq!(
        performance["output_speed"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "输出速度只统计运行时观测样本"
    );
    assert!(
        performance["first_byte"]["sources"]
            .as_array()
            .unwrap()
            .len()
            == 1,
        "首字节延迟只有运行时观测来源"
    );

    let performance_series: Value = ureq::get(&format!(
        "{base}/api/v1/usage/performance/timeseries?from={from}&to={to}"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    let first_bucket = &performance_series["series"][0];
    assert_eq!(first_bucket["ttft_avg_ms"], 200);
    assert_eq!(first_bucket["first_byte_avg_ms"], 100);
    assert_eq!(first_bucket["generation_avg_ms"], 500);
    assert_eq!(state.db.count("traffic_estimates"), 0);
}

/// 游标同步闭环：注册 → 读空 → 推进 → 读回一致 → 幂等 → 未认证 401 → 删节点清理。
#[tokio::test(flavor = "multi_thread")]
async fn cursor_sync_roundtrip_and_lifecycle() {
    let dir = tempdir("e2e-cursors");
    let (base, _state) = spawn_hub(&dir).await;
    let admin = admin_token(&base);

    // 创建节点，获取一次性专属 token
    let created: Value = ureq::post(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &format!("Bearer {admin}"))
        .send_json(json!({
            "name": "cursor-node",
            "ip": "203.0.113.9",
            "poll_interval_seconds": 120
        }))
        .unwrap()
        .into_json()
        .unwrap();
    let node_id = created["node_id"].as_str().unwrap().to_string();
    let node_token = format!("Bearer {}", created["token"].as_str().unwrap());

    // Agent 注册（使用节点专属 token）
    let reg: Value = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", &node_token)
        .send_json(json!({
            "schema_version": 1, "node_id": node_id, "node_name": "cursor-node",
            "agent_version": "0.4.0", "protocol_version": 1
        }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(reg["ok"], true);

    // 初始为空
    let cursors: Value = ureq::get(&format!("{base}/api/v1/collectors/cursors"))
        .set("Authorization", &node_token)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(cursors["cursors"].as_array().unwrap().len(), 0);

    // 推进游标
    let push: Value = ureq::post(&format!("{base}/api/v1/collectors/cursors"))
        .set("Authorization", &node_token)
        .send_json(json!({
            "schema_version": 1,
            "cursors": [{"source_id": "s1", "cursor_json": "{\"off\":10}", "updated_at": null}]
        }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(push["ok"], true);

    // 读回一致
    let cursors: Value = ureq::get(&format!("{base}/api/v1/collectors/cursors"))
        .set("Authorization", &node_token)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let arr = cursors["cursors"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["source_id"], "s1");
    assert_eq!(arr[0]["cursor_json"], "{\"off\":10}");
    assert!(!arr[0]["updated_at"].as_str().unwrap_or("").is_empty());

    // 幂等重复提交
    ureq::post(&format!("{base}/api/v1/collectors/cursors"))
        .set("Authorization", &node_token)
        .send_json(json!({
            "schema_version": 1,
            "cursors": [{"source_id": "s1", "cursor_json": "{\"off\":10}", "updated_at": null}]
        }))
        .unwrap();

    // 未认证 → 401；env bootstrap token（无注册身份）→ 400
    let status = ureq::get(&format!("{base}/api/v1/collectors/cursors"))
        .set("Authorization", "Bearer wrong")
        .call();
    assert!(status.is_err());
    let err = ureq::get(&format!("{base}/api/v1/collectors/cursors"))
        .set("Authorization", "Bearer testtok")
        .call()
        .unwrap_err();
    assert!(matches!(err, ureq::Error::Status(400, _)));

    // 删除节点 → 游标随 collector 级联清理（token 同时失效 → 401）
    ureq::delete(&format!("{base}/api/v1/nodes/{node_id}"))
        .set("Authorization", &format!("Bearer {admin}"))
        .call()
        .unwrap();
    let status = ureq::get(&format!("{base}/api/v1/collectors/cursors"))
        .set("Authorization", &node_token)
        .call();
    assert!(status.is_err(), "删除节点后 collector token 应失效");
}

#[test]
fn pricing_alias_persists_metadata_and_effective_time() {
    let dir = tempdir("metria-pricing-alias");
    let db = HubDb::open(&test_cfg(&dir)).unwrap();
    db.apply_migrations().unwrap();
    let id = db
        .insert_pricing_rule(&json!({
            "model_pattern": "my-custom-model",
            "price_equivalent_to": "gpt-5",
            "price_equivalent_missing_as_free": true,
            "effective_from": "2026-09-14T00:00:00Z"
        }))
        .unwrap();
    let rule = db
        .load_all_rules()
        .into_iter()
        .find(|rule| rule.id.as_str() == id)
        .expect("alias rule");
    assert_eq!(rule.metadata["price_equivalent_to"], "gpt-5");
    assert_eq!(rule.metadata["price_equivalent_missing_as_free"], true);
    assert_eq!(
        rule.effective_from.unwrap().to_rfc3339(),
        "2026-09-14T00:00:00+00:00"
    );
}

#[test]
fn pricing_options_use_platform_models_and_catalog_rules() {
    let dir = tempdir("metria-pricing-options");
    let db = HubDb::open(&test_cfg(&dir)).unwrap();
    db.apply_migrations().unwrap();
    {
        let c = db.conn();
        c.execute(
            "INSERT INTO model_calls
                (id, node_id, collector_id, client_id, source_id, session_id,
                 model_normalized, started_at, status, call_granularity,
                 created_at, updated_at)
             VALUES (?1, 'node', 'collector', 'client', 'source', 'session',
                     ?2, '2026-09-14T00:00:00Z', 'success', 'call',
                     '2026-09-14T00:00:00Z', '2026-09-14T00:00:00Z')",
            metria_storage::rusqlite::params!["call-options", "custom-model"],
        )
        .unwrap();
        c.execute(
            "INSERT INTO pricing_rules
                (id, source, channel, provider_pattern, model_pattern,
                 input_price, priority, enabled, metadata, created_at, updated_at)
             VALUES ('catalog-option', 'openrouter_catalog', 'openrouter',
                     'openai', 'gpt-5', 1000000, 0, 1, '{}',
                     '2026-09-14T00:00:00Z', '2026-09-14T00:00:00Z')",
            [],
        )
        .unwrap();
    }
    let options = db.pricing_model_options();
    assert_eq!(options["used_models"][0]["model"], "custom-model");
    assert_eq!(options["catalog_models"][0]["model_pattern"], "gpt-5");
    assert_eq!(options["catalog_models"][0]["price_available"], true);
}

#[test]
fn catalog_sync_keeps_only_latest_snapshot_and_rules() {
    use metria_hub::catalog::RuleInput;

    let dir = tempdir("metria-pricing-retention");
    let db = HubDb::open(&test_cfg(&dir)).unwrap();
    db.apply_migrations().unwrap();
    {
        let c = db.conn();
        c.execute(
            "INSERT INTO pricing_catalogs (id, name, kind, enabled, base_url, priority, created_at, updated_at)
             VALUES ('catalog-test', 'Test', 'openrouter', 1, 'http://example.test/models', 0, '2026-09-14T00:00:00Z', '2026-09-14T00:00:00Z')",
            [],
        )
        .unwrap();
    }
    let rule = |model: &str| RuleInput {
        provider: "openai".into(),
        model: model.into(),
        input: Some(1_000_000),
        output: Some(2_000_000),
        cache_read: None,
        cache_write: None,
        reasoning: None,
        request: None,
    };
    db.upsert_snapshot_and_rules(
        "catalog-test",
        "openrouter",
        None,
        "hash-a".into(),
        &[rule("gpt-5")],
    )
    .unwrap();
    // 第二次同步：旧快照与旧规则应被删除，只保留最新
    db.upsert_snapshot_and_rules(
        "catalog-test",
        "openrouter",
        None,
        "hash-b".into(),
        &[rule("gpt-5"), rule("gpt-5-mini")],
    )
    .unwrap();

    let (snapshots, rules): (i64, i64) = {
        let c = db.conn();
        (
            c.query_row(
                "SELECT COUNT(*) FROM pricing_snapshots WHERE catalog_id = 'catalog-test'",
                [],
                |r| r.get(0),
            )
            .unwrap(),
            c.query_row(
                "SELECT COUNT(*) FROM pricing_rules WHERE source = 'openrouter_catalog'",
                [],
                |r| r.get(0),
            )
            .unwrap(),
        )
    };
    assert_eq!(snapshots, 1, "旧快照应被删除");
    assert_eq!(rules, 2, "只保留最新快照规则，旧规则不累积");

    // 手工插入一条停用的目录规则与一条用户规则
    {
        let c = db.conn();
        c.execute(
            "INSERT INTO pricing_rules (id, source, channel, provider_pattern, model_pattern, input_price, priority, enabled, metadata, created_at, updated_at)
             VALUES ('legacy-disabled', 'openrouter_catalog', 'openrouter', '*', 'legacy-model', 1000, 0, 0, '{}', '2026-09-14T00:00:00Z', '2026-09-14T00:00:00Z')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO pricing_rules (id, source, channel, provider_pattern, model_pattern, priority, enabled, metadata, created_at, updated_at)
             VALUES ('user-link', 'user_override', 'vendor_direct', '*', 'my-model', 0, 1, '{\"price_equivalent_to\":\"gpt-5\"}', '2026-09-14T00:00:00Z', '2026-09-14T00:00:00Z')",
            [],
        )
        .unwrap();
    }
    let listed = db.list_pricing_rules();
    let models: Vec<String> = listed
        .iter()
        .filter_map(|r| {
            r.get("model_pattern")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect();
    assert!(models.contains(&"gpt-5".to_string()), "生效目录规则应返回");
    assert!(models.contains(&"my-model".to_string()), "用户规则应返回");
    assert!(
        !models.contains(&"legacy-model".to_string()),
        "停用目录规则不应返回"
    );
}

/// 概览活动/消息统计按窗口内明细与重叠时长计算，而不是按会话开始时间归属。
#[tokio::test(flavor = "multi_thread")]
async fn overview_activity_uses_window_and_detail() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _state) = spawn_hub(dir.path()).await;

    ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({
            "schema_version": 1, "node_id": "act-node", "node_name": "act-node",
            "node_platform": "linux", "node_architecture": "x86_64",
            "agent_version": "0.3.0", "protocol_version": 1
        }))
        .unwrap();

    let node = "act-node";
    let collector = "collector-act-node";
    let batch = json!({
        "schema_version": 1,
        "batch_id": "act-batch-1",
        "node_id": node,
        "collector_id": collector,
        "agent_version": "0.3.0",
        "events": [
            {"kind": "session", "event_id": "blake3:act-session", "payload": {
                "id": "act-sess-id",
                "source_session_id": "act-src-session",
                "node_id": node,
                "collector_id": collector,
                "source_id": "src-act",
                "client_id": "codex",
                "started_at": "2026-09-18T23:00:00Z",
                "last_activity_at": "2026-09-19T02:00:00Z",
                "status": "active",
                "message_count": 99,
                "tool_call_count": 88,
                "model_call_count": 0,
                "created_at": "2026-09-18T23:00:00Z"
            }},
            {"kind": "message", "event_id": "blake3:act-message", "payload": {
                "id": "act-msg-1", "turn_id": null, "session_id": "act-sess-id",
                "source_message_id": null, "sequence": 1, "role": "user", "content_type": "text",
                "content": "hi", "content_hash": "h1", "content_length": 2, "utf8_bytes": 2,
                "created_at": "2026-09-19T01:00:00Z", "redacted": false
            }},
            {"kind": "tool", "event_id": "blake3:act-tool", "payload": {
                "id": "act-tool-1", "session_id": "act-sess-id", "model_call_id": null, "turn_id": null,
                "source_tool_id": "t1", "name": "Bash", "tool_type": "command", "status": "success",
                "input_length": 1, "output_length": 1, "started_at": "2026-09-19T01:30:00Z",
                "completed_at": "2026-09-19T01:30:01Z", "duration_ms": 1000, "error": false,
                "created_at": "2026-09-19T01:30:01Z"
            }}
        ]
    });
    let upload: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch)
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(upload["accepted"].as_array().unwrap().len(), 3, "{upload}");

    let token = admin_token(&base);
    let overview: Value = ureq::get(&format!(
        "{base}/api/v1/overview?from=2026-09-19T00:00:00Z&to=2026-09-19T03:00:00Z"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(overview["sessions"], 0, "窗口内没有新建会话");
    assert_eq!(
        overview["active_sessions"], 1,
        "窗口内仍在活动的会话应计入活跃会话"
    );
    assert_eq!(overview["message_count"], 1, "按消息时间统计");
    assert_eq!(overview["user_message_count"], 1);
    assert_eq!(overview["tool_call_count"], 1, "按工具时间统计");
    assert_eq!(
        overview["session_duration_ms"], 7_200_000,
        "会话跨度应裁剪到窗口（23:00 开始、02:00 最后活动 → 00:00-02:00 共 2 小时）"
    );
}

/// Agent 上报来源集合后，消失的来源标记 missing，新鲜度不再被历史来源拉低。
#[tokio::test(flavor = "multi_thread")]
async fn source_sync_marks_missing_sources() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _state) = spawn_hub(dir.path()).await;

    ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({
            "schema_version": 1, "node_id": "sync-node", "node_name": "sync-node",
            "node_platform": "linux", "node_architecture": "x86_64",
            "agent_version": "0.3.0", "protocol_version": 1
        }))
        .unwrap();

    let node = "sync-node";
    let collector = "collector-sync-node";
    let source_event = |id: &str| {
        json!({"kind": "source", "event_id": format!("blake3:src-{id}"), "payload": {
            "id": id, "node_id": node, "collector_id": collector, "client_id": "codex",
            "adapter_id": "codex", "adapter_version": "0.3.0",
            "source_fingerprint": id, "source_path_hash": id, "capabilities": [], "status": "active"
        }})
    };
    let upload = |events: Value, batch_id: &str| -> Value {
        ureq::post(&format!("{base}/api/v1/events/batch"))
            .set("Authorization", "Bearer testtok")
            .send_json(json!({
                "schema_version": 1, "batch_id": batch_id,
                "node_id": node, "collector_id": collector, "agent_version": "0.3.0",
                "events": events
            }))
            .unwrap()
            .into_json()
            .unwrap()
    };

    let resp = upload(
        json!([source_event("s1"), source_event("s2")]),
        "sync-batch-1",
    );
    assert_eq!(resp["accepted"].as_array().unwrap().len(), 2, "{resp}");

    let resp = upload(
        json!([{"kind": "source_sync", "event_id": "blake3:sync-1", "payload": {
            "node_id": node, "collector_id": collector, "source_ids": ["s1"]
        }}]),
        "sync-batch-2",
    );
    assert_eq!(resp["accepted"].as_array().unwrap().len(), 1, "{resp}");

    let token = admin_token(&base);
    let overview: Value = ureq::get(&format!(
        "{base}/api/v1/overview?from=2026-09-19T00:00:00Z&to=2026-09-19T03:00:00Z"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    let freshness = &overview["freshness"];
    assert_eq!(
        freshness["source_total"], 1,
        "消失的来源不计入覆盖率分母：{freshness}"
    );
    assert_eq!(freshness["source_missing"], 1);
    assert_eq!(freshness["source_healthy"], 1);
    assert!(
        freshness["last_scan_at"].is_string(),
        "同步即记录扫描时间：{freshness}"
    );
    assert_eq!(freshness["coverage"], 1.0);
    assert_eq!(freshness["status"], "fresh");
}

fn tempdir(prefix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 会话明细（message/tool）事件应通过协议白名单并落库，可在时间线/工具接口读取。
#[tokio::test(flavor = "multi_thread")]
async fn session_detail_events_are_ingested() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _state) = spawn_hub(dir.path()).await;

    ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({
            "schema_version": 1, "node_id": "detail-node", "node_name": "detail-node",
            "node_platform": "linux", "node_architecture": "x86_64",
            "agent_version": "0.3.0", "protocol_version": 1
        }))
        .unwrap();

    let node = "detail-node";
    let collector = "collector-detail-node";
    let sid = "detail-session-source";
    let batch = json!({
        "schema_version": 1,
        "batch_id": "detail-batch-1",
        "node_id": node,
        "collector_id": collector,
        "agent_version": "0.3.0",
        "events": [
            {"kind": "session", "event_id": "blake3:detail-session", "payload": {
                "id": "detail-sess-id",
                "source_session_id": sid,
                "node_id": node,
                "collector_id": collector,
                "source_id": "src-detail",
                "client_id": "codex",
                "started_at": "2026-09-19T01:00:00Z",
                "status": "ended",
                "message_count": 1,
                "tool_call_count": 1,
                "model_call_count": 1,
                "created_at": "2026-09-19T01:00:00Z"
            }},
            {"kind": "message", "event_id": "blake3:detail-message", "payload": {
                "id": "detail-msg-1",
                "turn_id": null,
                "session_id": "detail-sess-id",
                "source_message_id": null,
                "sequence": 1,
                "role": "user",
                "content_type": "text",
                "content": "你好",
                "content_hash": "hash-detail",
                "content_length": 2,
                "utf8_bytes": 6,
                "created_at": "2026-09-19T01:00:01Z",
                "redacted": false
            }},
            {"kind": "tool", "event_id": "blake3:detail-tool", "payload": {
                "id": "detail-tool-1",
                "session_id": "detail-sess-id",
                "model_call_id": null,
                "turn_id": null,
                "source_tool_id": "call_detail",
                "name": "Bash",
                "tool_type": "command",
                "status": "success",
                "input_length": 10,
                "output_length": 20,
                "started_at": "2026-09-19T01:00:02Z",
                "completed_at": "2026-09-19T01:00:03Z",
                "duration_ms": 1000,
                "error": false,
                "created_at": "2026-09-19T01:00:03Z"
            }}
        ]
    });
    let upload: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch)
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        upload["accepted"].as_array().unwrap().len(),
        3,
        "message 与 tool 应被接受：{upload}"
    );
    assert!(upload["failed"].as_array().unwrap().is_empty());

    let token = admin_token(&base);
    let sessions: Value = ureq::get(&format!(
        "{base}/api/v1/sessions?from=2026-09-19T00:00:00Z&to=2026-09-19T02:00:00Z"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    let session_id = sessions["sessions"][0]["id"].as_str().unwrap().to_string();

    let timeline: Value = ureq::get(&format!("{base}/api/v1/sessions/{session_id}/timeline"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let messages = timeline["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "时间线应包含上传的消息");
    assert_eq!(messages[0]["role"], "user");

    let tools: Value = ureq::get(&format!("{base}/api/v1/sessions/{session_id}/tools"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        tools["tools"].as_array().unwrap().len(),
        1,
        "会话工具列表应包含上传的工具事件"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn data_quality_reports_source_freshness_and_usage_sources() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    // 注册 node/collector：sources 有外键，source 事件需先有 collector 行
    let reg: Value = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({
            "schema_version": 1, "node_id": "n-dq", "node_name": "n-dq",
            "node_platform": "linux", "node_architecture": "x86_64",
            "agent_version": "0.1.0", "protocol_version": 1
        }))
        .unwrap()
        .into_json()
        .unwrap();
    let collector = reg["collector_id"].as_str().unwrap().to_string();
    // 事件 source_id 是 pseudo_id(path_hash)，与 sources.id（原始 path_hash）不同：
    // ingest 必须用同批次 source 事件把它们关联起来回写 last_event_at。
    let pseudo = metria_core::privacy::hash_path("dq-src")
        .as_str()
        .to_string();
    let batch = json!({
        "schema_version": 1, "batch_id": "dq-1", "node_id": "n-dq", "collector_id": collector,
        "agent_version": "0.1.0",
        "events": [
            {"kind": "source", "event_id": "blake3:dq-src", "payload": {
                "id": "dq-src", "node_id": "n-dq", "collector_id": collector,
                "client_id": "codex", "adapter_id": "codex", "adapter_version": "0.3.0",
                "source_fingerprint": "fp", "source_path_hash": "dq-src",
                "capabilities": [], "status": "active"
            }},
            {"kind": "session", "event_id": "blake3:dq-session", "payload": {
                "id": "dq-session", "source_session_id": "dq-session",
                "node_id": "n-dq", "collector_id": collector, "client_id": "codex",
                "source_id": pseudo, "started_at": "2026-08-07T01:00:00Z",
                "last_activity_at": "2026-08-07T01:00:05Z", "status": "active",
                "message_count": 0, "tool_call_count": 0, "model_call_count": 1
            }},
            {"kind": "call", "event_id": "blake3:dq-call", "payload": {
                "id": "dq-call", "node_id": "n-dq", "collector_id": collector,
                "client_id": "codex", "source_id": pseudo, "session_id": "dq-session",
                "model_raw": "gpt-5", "model_normalized": "gpt-5",
                "provider_raw": "openai", "provider_normalized": "openai",
                "started_at": "2026-08-07T01:00:10Z", "completed_at": "2026-08-07T01:00:15Z",
                "status": "success", "call_granularity": "call"
            }},
            {"kind": "usage", "event_id": "blake3:dq-usage", "payload": {
                "event_id": "blake3:dq-usage", "schema_version": 1,
                "node_id": "n-dq", "collector_id": collector, "source_id": pseudo,
                "client_id": "codex", "adapter_id": "codex", "adapter_version": "0.3.0",
                "session_id": "dq-session", "model_call_id": "dq-call",
                "timestamp": "2026-08-07T01:00:20Z",
                "model_raw": "gpt-5", "model_normalized": "gpt-5",
                "usage": {"input": 1000, "output": 100, "cache_read": 50, "cache_write": 10, "reasoning": 5},
                "cost": {"reported_micro_usd": null, "calculated_micro_usd": 1234,
                         "estimated_micro_usd": null, "pricing_rule_id": null, "pricing_snapshot_id": null},
                "quality": {"usage_source": "reported", "granularity": "call", "confidence": 1.0}
            }}
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
        4,
        "全部事件应接受: {resp}"
    );

    // sources.last_event_at 取批次内该来源最新事件时间（usage 01:00:20Z）
    let last_event: Option<String> = state
        .db
        .conn()
        .query_row(
            "SELECT last_event_at FROM sources WHERE id = 'dq-src'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        last_event.as_deref(),
        Some("2026-08-07T01:00:20+00:00"),
        "ingest 应把 pseudo source_id 关联回 sources.id 并回写最新事件时间"
    );

    let token = admin_token(&base);
    let quality: Value = ureq::get(&format!(
        "{base}/api/v1/data-quality?from=2026-08-07T00:00:00Z&to=2026-08-08T00:00:00Z"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    let cursor = quality["cursor_status"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source_id"] == "dq-src")
        .expect("cursor_status 应包含该来源");
    assert_eq!(cursor["last_event_at"], "2026-08-07T01:00:20+00:00");

    // 用量来源分布只统计 Usage 行：不含空来源，且带完整 Token 构成
    let dist = quality["usage_distribution"].as_array().unwrap();
    assert!(
        dist.iter().all(|row| row["usage_source"] != ""),
        "不应再返回空 usage_source 行: {dist:?}"
    );
    let reported = dist
        .iter()
        .find(|row| row["usage_source"] == "reported")
        .expect("应有 reported 行");
    assert_eq!(reported["tokens"], 1000);
    assert_eq!(reported["output_tokens"], 100);
    assert_eq!(reported["cache_read_tokens"], 50);
    assert_eq!(reported["cache_write_tokens"], 10);
    assert_eq!(reported["reasoning_tokens"], 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn subagent_sessions_are_excluded_and_visible_in_parent_detail() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    let reg: Value = ureq::post(&format!("{base}/api/v1/collectors/register"))
        .set("Authorization", "Bearer testtok")
        .send_json(json!({
            "schema_version": 1, "node_id": "n-sa", "node_name": "n-sa",
            "node_platform": "linux", "node_architecture": "x86_64",
            "agent_version": "0.1.0", "protocol_version": 1
        }))
        .unwrap()
        .into_json()
        .unwrap();
    let collector = reg["collector_id"].as_str().unwrap().to_string();
    let pseudo = metria_core::privacy::hash_path("sa-src")
        .as_str()
        .to_string();
    let call = |id: &str, session: &str, input: i64| {
        json!({"kind": "call", "event_id": format!("blake3:call-{id}"), "payload": {
            "id": id, "node_id": "n-sa", "collector_id": collector, "client_id": "opencode",
            "source_id": pseudo, "session_id": session,
            "model_raw": "gpt-5", "model_normalized": "gpt-5",
            "started_at": "2026-08-08T01:00:10Z", "status": "success",
            "call_granularity": "call", "input_tokens": input, "output_tokens": 10
        }})
    };
    let usage = |id: &str, session: &str, input: i64| {
        json!({"kind": "usage", "event_id": format!("blake3:usage-{id}"), "payload": {
            "event_id": format!("blake3:usage-{id}"), "schema_version": 1,
            "node_id": "n-sa", "collector_id": collector, "source_id": pseudo,
            "client_id": "opencode", "adapter_id": "opencode", "adapter_version": "0.3.0",
            "session_id": session, "model_call_id": id,
            "timestamp": "2026-08-08T01:00:10Z",
            "model_raw": "gpt-5", "model_normalized": "gpt-5",
            "usage": {"input": input, "output": 10},
            "cost": {"calculated_micro_usd": 100},
            "quality": {"usage_source": "reported", "granularity": "call", "confidence": 1.0}
        }})
    };
    let batch = json!({
        "schema_version": 1, "batch_id": "sa-1", "node_id": "n-sa", "collector_id": collector,
        "agent_version": "0.1.0",
        "events": [
            {"kind": "source", "event_id": "blake3:sa-src", "payload": {
                "id": "sa-src", "node_id": "n-sa", "collector_id": collector,
                "client_id": "opencode", "adapter_id": "opencode", "adapter_version": "0.3.0",
                "source_fingerprint": "fp", "source_path_hash": "sa-src",
                "capabilities": [], "status": "active"
            }},
            {"kind": "session", "event_id": "blake3:sa-parent", "payload": {
                "id": "sa-parent", "source_session_id": "sa-parent", "node_id": "n-sa",
                "collector_id": collector, "client_id": "opencode", "source_id": pseudo,
                "started_at": "2026-08-08T01:00:00Z", "last_activity_at": "2026-08-08T01:00:20Z",
                "status": "ended", "message_count": 0, "tool_call_count": 0,
                "model_call_count": 1, "subagent_count": 1
            }},
            {"kind": "session", "event_id": "blake3:sa-child", "payload": {
                "id": "sa-child", "source_session_id": "sa-child", "node_id": "n-sa",
                "collector_id": collector, "client_id": "opencode", "source_id": pseudo,
                "parent_session_id": "sa-parent",
                "started_at": "2026-08-08T01:00:05Z", "last_activity_at": "2026-08-08T01:00:15Z",
                "status": "ended", "message_count": 0, "tool_call_count": 0,
                "model_call_count": 1
            }},
            call("sa-call-parent", "sa-parent", 100),
            usage("sa-call-parent", "sa-parent", 100),
            call("sa-call-child", "sa-child", 500),
            usage("sa-call-child", "sa-child", 500),
            {"kind": "subagent", "event_id": "blake3:sa-rel", "payload": {
                "id": "sa-rel", "session_id": "sa-parent", "child_session_id": "sa-child",
                "relation": "subagent", "created_at": "2026-08-08T01:00:06Z"
            }}
        ]
    });
    let resp: Value = ureq::post(&format!("{base}/api/v1/events/batch"))
        .set("Authorization", "Bearer testtok")
        .send_json(batch)
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        resp["accepted"].as_array().unwrap().len(),
        8,
        "全部事件应接受: {resp}"
    );

    let token = admin_token(&base);
    let get = |path: &str| -> Value {
        ureq::get(&format!("{base}{path}"))
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .unwrap()
            .into_json()
            .unwrap()
    };

    // 概览会话数只含主会话；Token 仍包含子会话用量
    let overview = get("/api/v1/overview?from=2026-08-08T00:00:00Z&to=2026-08-09T00:00:00Z");
    assert_eq!(overview["sessions"], 1, "会话数应只含主会话");
    assert_eq!(overview["input_tokens"], 600, "子会话 Token 不应丢失");

    // 列表默认不含子会话；显式包含时返回 2 条
    let list = get("/api/v1/sessions?from=2026-08-08T00:00:00Z&to=2026-08-09T00:00:00Z");
    assert_eq!(
        list["sessions"].as_array().unwrap().len(),
        1,
        "默认只列主会话"
    );
    let list_all = get(
        "/api/v1/sessions?from=2026-08-08T00:00:00Z&to=2026-08-09T00:00:00Z&include_subagents=true",
    );
    assert_eq!(
        list_all["sessions"].as_array().unwrap().len(),
        2,
        "include_subagents=true 应返回全部会话"
    );

    // 子会话详情：父级已解析为规范键
    let parent_key = "n-sa:sa-parent";
    let child_key = "n-sa:sa-child";
    let child = get(&format!("/api/v1/sessions/{child_key}"));
    assert_eq!(child["session"]["parent_session_id"], parent_key);
    let parent = get(&format!("/api/v1/sessions/{parent_key}"));
    assert!(parent["session"]["parent_session_id"].is_null());

    // 主会话详情：子 Agent 明细与合计
    let subs = get(&format!("/api/v1/sessions/{parent_key}/subagents"));
    let children = subs["children"].as_array().unwrap();
    assert_eq!(children.len(), 1, "应返回子会话摘要");
    assert_eq!(children[0]["id"], child_key);
    assert_eq!(children[0]["input_tokens"], 500);
    assert_eq!(subs["totals"]["children"], 1);
    assert_eq!(subs["totals"]["input_tokens"], 500);
    assert_eq!(subs["totals"]["model_calls"], 1);
    assert_eq!(subs["totals"]["cost_micro_usd"], 100);

    // 回填幂等：父级已写入时不重复回填
    assert_eq!(state.db.backfill_subagent_parents().unwrap(), 0);

    // 历史重复关系清理：手工插入同父子会话的第二条关系后应被去重
    state
        .db
        .insert_subagent(&json!({
            "id": "sa-rel-dup", "session_id": parent_key, "child_session_id": "sa-child",
            "relation": "subagent", "created_at": "2026-08-08T01:00:07Z"
        }))
        .unwrap();
    assert_eq!(state.db.dedupe_subagent_relations().unwrap(), 1);
    let subs = get(&format!("/api/v1/sessions/{parent_key}/subagents"));
    assert_eq!(
        subs["relations"].as_array().unwrap().len(),
        1,
        "去重后关系列表不应包含重复子会话"
    );
}
