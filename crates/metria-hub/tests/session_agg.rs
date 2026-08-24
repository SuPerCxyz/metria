//! 会话聚合刷新：session 事件先到（0 计数）、call 后到（独立批次）。
//!
//! 验证 call 能关联到已存在的 session，且 insert_call 后 session 聚合字段被刷新。
//! 使用 HubDb 直连（无 HTTP 服务器），避免 tokio runtime 挂起。

use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use serde_json::json;

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

fn temp_db(tag: &str) -> HubDb {
    let dir = std::env::temp_dir().join(format!("sess-agg-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).unwrap();
    let db = HubDb::open(&test_cfg(&dir)).expect("open db");
    db.apply_migrations().expect("migrate");
    db
}

fn sess_payload(node: &str, sid: &str) -> serde_json::Value {
    json!({
        "id": sid, "source_session_id": sid, "node_id": node,
        "collector_id": "collector-agg-node", "source_id": "src-agg",
        "client_id": "codex", "started_at": "2026-08-10T08:00:00Z",
        "status": "active", "model_call_count": 0,
        "created_at": "2026-08-10T08:00:00Z"
    })
}

fn call_payload(node: &str, sid: &str, cid: &str, in_tok: i64, out_tok: i64) -> serde_json::Value {
    json!({
        "id": cid, "source_call_id": format!("codex:{sid}:1780000000001"),
        "node_id": node, "collector_id": "collector-agg-node",
        "source_id": "src-agg", "client_id": "codex",
        "session_id": sid, "model_raw": "gpt-5.6-sol",
        "started_at": "2026-08-10T08:10:00Z",
        "completed_at": "2026-08-10T08:11:00Z",
        "status": "success", "status_code": 200, "call_granularity": "call",
        "input_tokens": in_tok, "output_tokens": out_tok, "reasoning_tokens": 50
    })
}

fn usage_payload(node: &str, sid: &str, cid: &str, event_id: &str) -> serde_json::Value {
    json!({
        "event_id": event_id, "schema_version": 1, "node_id": node,
        "collector_id": "collector-agg-node", "source_id": "src-agg",
        "client_id": "codex", "adapter_id": "codex", "adapter_version": "0.1.0",
        "model_call_id": cid, "timestamp": "2026-08-10T08:10:30Z",
        "provider_normalized": "openai", "model_normalized": "gpt-5.6-sol",
        "usage": {"input": 10000, "output": 500, "cache_read": null, "cache_write": null, "reasoning": 50},
        "cost": {"reported_micro_usd": null, "calculated_micro_usd": 1234, "estimated_micro_usd": null},
        "quality": {"usage_source": "reported", "granularity": "call", "confidence": 1.0},
        "session_id": sid
    })
}

#[test]
fn session_agg_refreshes_on_cross_batch_call() {
    let db = temp_db("t1");
    let node = "agg-node";
    let sid = "agg-src-session-1";
    let ses_key = format!("{node}:{sid}");

    // 批次 1：仅 session 事件（聚合为 0）
    let is_new = db.upsert_session(&sess_payload(node, sid)).unwrap();
    assert!(is_new, "session 应为新插入");

    // 批次 2：同一 session 的 call 事件（跨批次，session_id = 源 session id）
    let inserted = db
        .insert_call(&call_payload(node, sid, "call-1", 10000, 500), &ses_key)
        .unwrap();
    assert!(inserted, "call 应为新插入");

    // call 的 session_id 应正确关联到规范键
    let call_ses: String = db
        .conn()
        .query_row(
            "SELECT session_id FROM model_calls WHERE id = 'call-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(call_ses, ses_key, "call 应关联到规范键");

    // session 聚合应被刷新
    let (count, in_tok, out_tok, cache_read, model, status) = db
        .conn()
        .query_row(
            "SELECT model_call_count, input_tokens, output_tokens, cache_read_tokens,
                    primary_model_raw, status FROM sessions WHERE id = ?1",
            [&ses_key],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(count, 1, "model_call_count 应刷新为 1");
    assert_eq!(in_tok, Some(10000), "input_tokens 应刷新");
    assert_eq!(out_tok, Some(500), "output_tokens 应刷新");
    assert_eq!(cache_read, None, "未知 cache token 必须保持 null");
    assert_eq!(model.as_deref(), Some("gpt-5.6-sol"), "主模型应刷新");
    assert_eq!(status, "active", "插入 call 不应擅自结束活跃会话");
}

#[test]
fn session_agg_refresh_recomputes_from_multiple_calls() {
    let db = temp_db("t2");
    let node = "agg-node-2";
    let sid = "agg-src-session-2";
    let ses_key = format!("{node}:{sid}");

    db.upsert_session(&sess_payload(node, sid)).unwrap();

    // 两条 call 分别插入（模拟增量扫描多批次）
    for (cid, in_tok, out_tok) in [("call-a", 100, 10), ("call-b", 200, 20)] {
        let inserted = db
            .insert_call(&call_payload(node, sid, cid, in_tok, out_tok), &ses_key)
            .unwrap();
        assert!(inserted);
    }

    let (count, in_tok, out_tok) = db
        .conn()
        .query_row(
            "SELECT model_call_count, input_tokens, output_tokens FROM sessions WHERE id = ?1",
            [&ses_key],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(count, 2, "model_call_count 应累加为 2");
    assert_eq!(in_tok, Some(300), "input_tokens 应累加");
    assert_eq!(out_tok, Some(30), "output_tokens 应累加");
}

#[test]
fn usage_and_call_costs_link_in_either_arrival_order() {
    for (index, usage_first) in [true, false].into_iter().enumerate() {
        let db = temp_db(&format!("link-{index}"));
        let node = format!("link-node-{index}");
        let sid = format!("link-session-{index}");
        let cid = format!("link-call-{index}");
        let event_id = format!("link-usage-{index}");
        let session_key = format!("{node}:{sid}");
        db.upsert_session(&sess_payload(&node, &sid)).unwrap();

        if usage_first {
            db.insert_usage(&usage_payload(&node, &sid, &cid, &event_id), &session_key)
                .unwrap();
            db.insert_call(&call_payload(&node, &sid, &cid, 10000, 500), &session_key)
                .unwrap();
        } else {
            db.insert_call(&call_payload(&node, &sid, &cid, 10000, 500), &session_key)
                .unwrap();
            db.insert_usage(&usage_payload(&node, &sid, &cid, &event_id), &session_key)
                .unwrap();
        }

        let (linked_event, cost): (Option<String>, Option<i64>) = db
            .conn()
            .query_row(
                "SELECT usage_event_id, calculated_cost_micro_usd FROM model_calls WHERE id = ?1",
                [&cid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(linked_event.as_deref(), Some(event_id.as_str()));
        assert_eq!(cost, Some(1234));
    }
}
