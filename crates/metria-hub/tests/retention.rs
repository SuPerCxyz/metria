//! 运维表保留期清理与空间回收测试（`migrations/021_table_retention.sql` + `HubDb::prune_operational_tables`）。
//!
//! 覆盖 spec `hub-background-maintenance`：清理后每个 usage_event 仍至少一条计价记录、
//! 周期清理阻止继续增长、清理失败不影响服务、费用仍可追溯，以及 `compact_if_needed`
//! 只在空闲页占比过高时才做一次性 VACUUM。

use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_storage::rusqlite::params;

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

fn test_db(tag: &str) -> (std::path::PathBuf, HubDb) {
    let dir = std::env::temp_dir().join(format!("metria-retention-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = HubDb::open(&test_cfg(&dir)).unwrap();
    db.apply_migrations().unwrap();
    (dir, db)
}

/// 写入一条用量行，供“每个 usage_event 至少一条计价记录”断言使用。
fn insert_usage(db: &HubDb, event_id: &str, calculated_micro: i64) {
    let now = chrono::Utc::now().to_rfc3339();
    let c = db.conn();
    c.execute(
        "INSERT INTO usage_events (event_id, schema_version, node_id, collector_id, source_id,
             client_id, adapter_id, adapter_version, timestamp, usage_source, usage_granularity,
             calculated_cost_micro_usd)
         VALUES (?1, 1, 'n1', 'c1', 's1', 'claude-code', 'claude', '0.1.0', ?2, 'reported', 'call', ?3)",
        params![event_id, now, calculated_micro],
    )
    .unwrap();
}

/// 写入一条计价匹配（同一 usage_event 可有多条版本，`calculated_at` 决定新旧）。
fn insert_match(db: &HubDb, id: &str, usage_event_id: &str, calculated_at: &str, total: i64) {
    let c = db.conn();
    c.execute(
        "INSERT INTO pricing_matches (id, usage_event_id, match_type, calculated_at, total_cost)
         VALUES (?1, ?2, 'reprice', ?3, ?4)",
        params![id, usage_event_id, calculated_at, total],
    )
    .unwrap();
}

fn insert_batch(db: &HubDb, batch_id: &str, received_at: &str) {
    let c = db.conn();
    c.execute(
        "INSERT INTO upload_batches (batch_id, node_id, collector_id, received_at, status, event_count, bytes)
         VALUES (?1, 'n1', 'c1', ?2, 'accepted', 1, 10)",
        params![batch_id, received_at],
    )
    .unwrap();
}

fn count(db: &HubDb, sql: &str) -> i64 {
    db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn prune_keeps_latest_match_per_usage_event() {
    let (_dir, db) = test_db("latest-match");
    let recent = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let older = (chrono::Utc::now() - chrono::Duration::days(3)).to_rfc3339();
    let oldest = (chrono::Utc::now() - chrono::Duration::days(9)).to_rfc3339();

    insert_usage(&db, "ev-a", 500);
    insert_usage(&db, "ev-b", 700);
    // ev-a 三个版本（重计价两次），ev-b 仅一个版本
    insert_match(&db, "m-a1", "ev-a", &oldest, 100);
    insert_match(&db, "m-a2", "ev-a", &older, 300);
    insert_match(&db, "m-a3", "ev-a", &recent, 500);
    insert_match(&db, "m-b1", "ev-b", &older, 700);

    let report = db.prune_operational_tables().unwrap();
    assert_eq!(
        (
            report.upload_batches_deleted,
            report.pricing_matches_deleted
        ),
        (0, 2),
        "应只删掉被覆盖的两个旧版本: {report:?}"
    );
    assert!(report.finished, "清理应跑到没有可删行: {report:?}");

    let left = count(
        &db,
        "SELECT COUNT(*) FROM pricing_matches WHERE usage_event_id = 'ev-a'",
    );
    assert_eq!(left, 1, "同一 usage_event 只应保留最新一条");
    let newest: String = db
        .conn()
        .query_row(
            "SELECT id FROM pricing_matches WHERE usage_event_id = 'ev-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(newest, "m-a3", "保留的必须是最新版本，否则费用口径会回退");

    // 费用仍可追溯：清理后每个已有计价记录的 usage_event 至少还有一条
    let orphans = count(
        &db,
        "SELECT COUNT(*) FROM usage_events u
         WHERE u.calculated_cost_micro_usd IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM pricing_matches p WHERE p.usage_event_id = u.event_id)",
    );
    assert_eq!(orphans, 0, "有计算费用的用量行不得失去计价记录");
}

#[test]
fn prune_removes_expired_batches_and_keeps_recent() {
    let (_dir, db) = test_db("expired-batches");
    let expired = (chrono::Utc::now() - chrono::Duration::hours(48)).to_rfc3339();
    let fresh = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
    insert_batch(&db, "b-old-1", &expired);
    insert_batch(&db, "b-old-2", &expired);
    insert_batch(&db, "b-new-1", &fresh);

    let report = db.prune_operational_tables().unwrap();
    assert_eq!(
        report.upload_batches_deleted, 2,
        "超期批次应被删除: {report:?}"
    );

    let remaining: String = db
        .conn()
        .query_row("SELECT batch_id FROM upload_batches", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, "b-new-1", "保留期内的批次必须留下");
}

#[test]
fn repeated_prune_is_idempotent() {
    let (_dir, db) = test_db("idempotent");
    let expired = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    for i in 0..5 {
        insert_batch(&db, &format!("b-{i}"), &expired);
    }

    let first = db.prune_operational_tables().unwrap();
    assert_eq!(first.deleted(), 5);
    assert!(first.finished);

    // 再跑多轮：不再删任何行，说明周期清理能阻止继续增长而不是每轮空转
    let second = db.prune_operational_tables().unwrap();
    assert_eq!(second.deleted(), 0, "重复清理不应再删行: {second:?}");
    assert!(second.finished);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM upload_batches"), 0);
}

#[test]
fn retention_policy_falls_back_on_missing_or_invalid_settings() {
    let (_dir, db) = test_db("policy");

    // 迁移已写入默认值
    let policy = db.retention_policy().unwrap();
    assert_eq!(policy.upload_batch_retention_hours, 24);
    assert_eq!(policy.pricing_match_history_days, 0);

    // 键缺失 → 回落默认
    db.conn()
        .execute(
            "DELETE FROM settings WHERE key = 'upload_batch_retention_hours'",
            [],
        )
        .unwrap();
    let policy = db.retention_policy().unwrap();
    assert_eq!(policy.upload_batch_retention_hours, 24);

    // 非法值 → 回落默认，不 panic
    db.setting_set("upload_batch_retention_hours", "not-a-number")
        .unwrap();
    let policy = db.retention_policy().unwrap();
    assert_eq!(policy.upload_batch_retention_hours, 24);

    // 负数同样回落
    db.setting_set("upload_batch_retention_hours", "-3")
        .unwrap();
    let policy = db.retention_policy().unwrap();
    assert_eq!(policy.upload_batch_retention_hours, 24);
}

#[test]
fn prune_records_stats_for_system_info() {
    let (_dir, db) = test_db("stats");
    let expired = (chrono::Utc::now() - chrono::Duration::hours(30)).to_rfc3339();
    insert_batch(&db, "b-1", &expired);
    insert_batch(&db, "b-2", &expired);

    let report = db.prune_operational_tables().unwrap();
    assert_eq!(report.deleted(), 2);

    let deleted: i64 = db
        .setting_get(metria_hub::db::KEY_LAST_CLEANUP_DELETED)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(deleted, 2, "最近一次清理行数应可查询");
    assert!(
        db.setting_get(metria_hub::db::KEY_LAST_CLEANUP_AT)
            .unwrap()
            .is_some(),
        "最近清理时间应可查询"
    );
    let total: i64 = db
        .setting_get(metria_hub::db::KEY_TOTAL_CLEANUP_DELETED)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert!(total >= 2, "累计清理行数应只增不减: {total}");
}

#[test]
fn prune_failure_returns_error_without_poisoning_db() {
    let (_dir, db) = test_db("failure");
    insert_batch(
        &db,
        "b-1",
        &(chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339(),
    );

    // 人为制造失败：表不存在 → 必须是 Err 而不是 panic，且连接仍可继续服务
    db.conn()
        .execute(
            "ALTER TABLE upload_batches RENAME TO upload_batches_gone",
            [],
        )
        .unwrap();

    let result = db.prune_operational_tables();
    assert!(result.is_err(), "清理失败应返回错误交由维护任务告警");

    // 服务不受影响：其他查询照常执行
    assert!(db.setting_get("upload_batch_retention_hours").is_ok());
    assert!(count(&db, "SELECT COUNT(*) FROM pricing_matches") == 0);
}

#[test]
fn compact_skips_clean_database_and_reclaims_after_delete() {
    let (_dir, db) = test_db("compact");
    // 新库建库时已设置 INCREMENTAL 且无空闲页 → 跳过，不浪费启动时间
    assert_eq!(
        db.compact_if_needed().unwrap(),
        None,
        "干净库不应触发 VACUUM"
    );

    // 造出大量空闲页（删除远多于存量的行）
    let expired = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
    {
        let c = db.conn();
        c.execute_batch("BEGIN").unwrap();
        for i in 0..20_000 {
            c.execute(
                "INSERT INTO upload_batches (batch_id, node_id, collector_id, received_at, status, event_count, bytes)
                 VALUES (?1, 'n1', 'c1', ?2, 'accepted', 1, 10)",
                params![format!("bulk-{i}"), expired],
            )
            .unwrap();
        }
        c.execute_batch("COMMIT").unwrap();
        c.execute("DELETE FROM upload_batches", []).unwrap();
    }

    let freed = db.compact_if_needed().unwrap();
    assert!(freed.is_some(), "空闲页占比过高时应触发一次 VACUUM");

    let auto: i64 = db
        .conn()
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(auto, 2, "VACUUM 后应处于 INCREMENTAL 模式");
    let free: i64 = db
        .conn()
        .query_row("PRAGMA freelist_count", [], |r| r.get(0))
        .unwrap();
    assert_eq!(free, 0, "VACUUM 后空闲页应被回收");

    // incremental_vacuum 此后才是真正生效的调用
    db.incremental_vacuum().unwrap();
    assert_eq!(
        db.compact_if_needed().unwrap(),
        None,
        "回收后再次启动应跳过"
    );
}
