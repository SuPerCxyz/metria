//! A 阶段：重计价增量传播与索引行为。

use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_storage::rusqlite::OptionalExtension;
use serde_json::json;
use std::sync::{Arc, Barrier};

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

fn test_db(tag: &str) -> HubDb {
    let dir = std::env::temp_dir().join(format!("metria-reprice-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = HubDb::open(&test_cfg(&dir)).unwrap();
    db.apply_migrations().unwrap();
    db
}

fn seed_rule(db: &HubDb) {
    db.insert_pricing_rule(&json!({
        "model_pattern": "gpt-reprice",
        "provider_pattern": "openai",
        "input_price": 1_000_000,
        "output_price": 2_000_000,
    }))
    .unwrap();
}

fn seed_call(db: &HubDb, id: &str, session: &str) {
    let ts = "2026-10-01T10:15:00+00:00";
    assert!(db
        .insert_call(
            &json!({
                "id": id,
                "source_call_id": id,
                "node_id": "n1", "collector_id": "c1", "client_id": "codex",
                "source_id": "s1", "session_id": session,
                "model_raw": "gpt-reprice", "model_normalized": "gpt-reprice",
                "provider_normalized": "openai",
                "started_at": ts, "completed_at": ts,
                "status": "success", "call_granularity": "call",
            }),
            &format!("n1:{session}"),
        )
        .unwrap());
}

fn seed_usage(db: &HubDb, event_id: &str, call_id: &str, session: &str, reported: Option<i64>) {
    let mut cost = json!({});
    if let Some(value) = reported {
        cost["reported_micro_usd"] = json!(value);
    }
    assert!(db
        .insert_usage(
            &json!({
                "event_id": event_id, "schema_version": 1,
                "node_id": "n1", "collector_id": "c1", "source_id": "s1", "client_id": "codex",
                "adapter_id": "codex", "adapter_version": "0.1.0",
                "model_call_id": call_id, "session_id": session,
                "timestamp": "2026-10-01T10:15:00+00:00",
                "provider_normalized": "openai", "model_normalized": "gpt-reprice",
                "usage": {"input": 1_000_000, "output": 100_000},
                "cost": cost,
                "quality": {"usage_source": "reported", "granularity": "call", "confidence": 1.0},
            }),
            &format!("n1:{session}"),
        )
        .unwrap());
}

fn call_cost(db: &HubDb, call_id: &str) -> (Option<i64>, Option<i64>, Option<i64>) {
    db.conn()
        .query_row(
            "SELECT reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd
             FROM model_calls WHERE id = ?1",
            [call_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

#[test]
fn zero_repriced_skips_propagation_and_matches() {
    let db = test_db("zero");
    seed_rule(&db);
    seed_call(&db, "call-zero", "sess-zero");
    seed_usage(&db, "usage-zero", "call-zero", "sess-zero", None);

    let engine = metria_hub::catalog::pricing_engine(&db);
    assert_eq!(db.reprice_all(&engine, false).unwrap(), 1);
    let (_, calculated, _) = call_cost(&db, "call-zero");
    assert!(
        calculated.unwrap() > 0,
        "首次重计价必须写入 calculated 费用"
    );
    let matches: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM pricing_matches", [], |r| r.get(0))
        .unwrap();
    assert_eq!(matches, 1);

    // 人为写入哨兵值：第二轮无变化时必须完全跳过传播。
    db.conn()
        .execute(
            "UPDATE model_calls SET calculated_cost_micro_usd = 999 WHERE id = 'call-zero'",
            [],
        )
        .unwrap();
    assert_eq!(db.reprice_all(&engine, false).unwrap(), 0);
    assert_eq!(call_cost(&db, "call-zero").1, Some(999), "0 变更不得传播");
    let matches_after: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM pricing_matches", [], |r| r.get(0))
        .unwrap();
    assert_eq!(matches_after, matches, "0 变更不得写 pricing_matches");
}

#[test]
fn only_changed_usage_updates_its_call_and_matches() {
    let db = test_db("partial");
    seed_rule(&db);
    for (call, usage, session) in [
        ("call-a", "usage-a", "sess-a"),
        ("call-b", "usage-b", "sess-b"),
    ] {
        seed_call(&db, call, session);
        seed_usage(&db, usage, call, session, None);
    }
    let engine = metria_hub::catalog::pricing_engine(&db);
    assert_eq!(db.reprice_all(&engine, false).unwrap(), 2);
    let before_a = call_cost(&db, "call-a");
    let before_b = call_cost(&db, "call-b");

    db.conn()
        .execute(
            "UPDATE usage_events SET input_tokens = 2_000_000 WHERE event_id = 'usage-a'",
            [],
        )
        .unwrap();
    assert_eq!(db.reprice_all(&engine, false).unwrap(), 1);
    let after_b = call_cost(&db, "call-b");
    assert_eq!(before_b, after_b, "未变化调用的费用不得被改写");
    let (_, calculated_a, _) = call_cost(&db, "call-a");
    assert!(
        calculated_a != before_a.1,
        "变化调用应重新计算: {before_a:?} -> {calculated_a:?}"
    );
}

#[test]
fn reported_cost_keeps_priority() {
    let db = test_db("reported");
    seed_rule(&db);
    seed_call(&db, "call-reported", "sess-reported");
    seed_usage(
        &db,
        "usage-reported",
        "call-reported",
        "sess-reported",
        Some(12_345),
    );

    let engine = metria_hub::catalog::pricing_engine(&db);
    // reported 已由入库关联写入，重计价不会产生新的派生费用，因此 0 变更。
    assert_eq!(db.reprice_all(&engine, false).unwrap(), 0);
    let (reported, calculated, estimated) = call_cost(&db, "call-reported");
    assert_eq!(reported, Some(12_345), "reported 必须优先且保留");
    assert_eq!(calculated, None, "reported 优先时不得伪造 calculated");
    assert_eq!(estimated, None);
}

#[test]
fn full_reprice_matches_engine_and_removes_stale_derived_cost() {
    let db = test_db("full");
    seed_rule(&db);
    seed_call(&db, "call-full", "sess-full");
    seed_usage(&db, "usage-full", "call-full", "sess-full", None);
    let engine = metria_hub::catalog::pricing_engine(&db);
    db.reprice_all(&engine, false).unwrap();
    let first = call_cost(&db, "call-full");
    assert!(first.1.unwrap() > 0);

    // 删除规则后全量重算：派生费用必须恢复为不可用，reported 不受影响。
    db.conn().execute("DELETE FROM pricing_rules", []).unwrap();
    let empty_engine = metria_hub::catalog::pricing_engine(&db);
    db.reprice_all(&empty_engine, false).unwrap();
    let after = call_cost(&db, "call-full");
    assert_eq!(after, (None, None, None), "无规则时必须清空派生费用");

    // 重建规则后全量重算应与引擎直接计算结果一致。
    seed_rule(&db);
    let engine = metria_hub::catalog::pricing_engine(&db);
    db.reprice_all(&engine, false).unwrap();
    let (reported, calculated, estimated) = call_cost(&db, "call-full");
    assert_eq!(reported, None);
    assert_eq!(
        calculated,
        engine
            .compute(
                &metria_core::model::Usage {
                    input: Some(1_000_000),
                    output: Some(100_000),
                    ..Default::default()
                },
                Some("gpt-reprice"),
                Some("openai"),
                chrono::DateTime::parse_from_rfc3339("2026-10-01T10:15:00+00:00")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                None,
            )
            .unwrap()
            .calculated_micro_usd
    );
    assert_eq!(estimated, None);
}

#[test]
fn partial_index_is_used_for_unpriced_scan() {
    let db = test_db("partial-index");
    let calls_index: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_calls_usage_event'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        calls_index, 1,
        "迁移 025 必须创建 model_calls(usage_event_id) 索引"
    );
    let plan: String = db
        .conn()
        .query_row(
            "EXPLAIN QUERY PLAN
             SELECT event_id FROM usage_events
             WHERE event_id > '' AND reported_cost_micro_usd IS NULL
               AND calculated_cost_micro_usd IS NULL AND estimated_cost_micro_usd IS NULL
             ORDER BY event_id LIMIT 10",
            [],
            |r| r.get(3),
        )
        .unwrap();
    assert!(
        plan.contains("idx_usage_unpriced"),
        "未计价扫描应使用部分索引，实际计划: {plan}"
    );
}

#[test]
fn reprice_does_not_rebuild_below_archive_watermark() {
    let db = test_db("watermark");
    seed_rule(&db);
    seed_call(&db, "call-wm", "sess-wm");
    seed_usage(&db, "usage-wm", "call-wm", "sess-wm", None);
    let engine = metria_hub::catalog::pricing_engine(&db);
    db.reprice_all(&engine, false).unwrap();
    db.rebuild_rollups(36_500).unwrap();
    let bucket = "2026-10-01T10:00:00+00:00";
    let before: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(SUM(calculated_cost), 0) FROM hourly_rollups WHERE bucket = ?1",
            [bucket],
            |r| r.get(0),
        )
        .unwrap();
    assert!(before > 0, "前置 rollup 应有费用: {before}");
    // 模拟水位之前的明细已被归档删除：重建不得把这些 rollup 清成 0/空。
    db.conn()
        .execute("DELETE FROM usage_events WHERE event_id = 'usage-wm'", [])
        .unwrap();
    db.conn()
        .execute("DELETE FROM model_calls WHERE id = 'call-wm'", [])
        .unwrap();
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        "2026-10-01T11:00:00+00:00",
    )
    .unwrap();

    db.rebuild_rollups(36_500).unwrap();
    let after: Option<i64> = db
        .conn()
        .query_row(
            "SELECT SUM(calculated_cost) FROM hourly_rollups WHERE bucket = ?1",
            [bucket],
            |r| r.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(after, Some(before), "水位之前的 rollup 不得被重建成 0/空");
}

#[test]
fn concurrent_reprice_calls_are_serialized() {
    let db = test_db("concurrent");
    seed_rule(&db);
    let count = 200i64;
    for index in 0..count {
        let call = format!("call-c{index}");
        let session = format!("sess-c{index}");
        seed_call(&db, &call, &session);
        seed_usage(&db, &format!("usage-c{index}"), &call, &session, None);
    }

    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let db = db.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let engine = metria_hub::catalog::pricing_engine(&db);
            barrier.wait();
            db.reprice_all(&engine, false)
        }));
    }
    let total: i64 = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .sum();
    assert_eq!(
        total, count,
        "两次并发重计价合计只应处理 {count} 条，TEMP 变化集不得互相覆盖"
    );
}
