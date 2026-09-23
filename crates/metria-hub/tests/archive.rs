//! 明细归档行为测试（spec `data-archival`）。
//!
//! 覆盖：默认关闭不删数据、删除前强制备份且失败即中止、聚合数据完整保留、
//! 归档可重复执行、非法保留天数被拒、孤儿计价记录一并回收、水位可查询。

use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use metria_hub::archive::{self, ArchiveReport};
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
        oidc: None,
    }
}

fn test_db(tag: &str) -> (PathBuf, HubDb) {
    let dir = std::env::temp_dir().join(format!("metria-archive-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = HubDb::open(&test_cfg(&dir)).unwrap();
    db.apply_migrations().unwrap();
    (dir, db)
}

/// 插入 `count` 条调用，`day_offset` 为相对当前时刻的天数偏移（负数表示过去）。
fn seed_calls(db: &HubDb, tag: &str, day_offset: i64, count: i64) -> Vec<String> {
    let base = Utc::now() + Duration::days(day_offset);
    let mut ids = Vec::new();
    for i in 0..count {
        let started = base - Duration::minutes(i * 17);
        let id = format!("call-{tag}-{i}");
        let call = json!({
            "id": id.clone(),
            "source_call_id": format!("src-{tag}-{i}"),
            "node_id": "n1", "collector_id": "c1", "client_id": "codex",
            "source_id": "s1", "session_id": format!("sess-{tag}"),
            "model_normalized": "gpt-5.6-sol", "provider_normalized": "openai",
            "started_at": started.to_rfc3339(),
            "completed_at": (started + Duration::seconds(30)).to_rfc3339(),
            "duration_ms": 30_000, "status": "success", "status_code": 200,
            "call_granularity": "call",
            "input_tokens": 1_000, "output_tokens": 200,
            "calculated_cost_micro_usd": 1_000,
        });
        db.insert_call(&call, &format!("n1:sess-{tag}")).unwrap();
        let usage = json!({
            "event_id": format!("ue-{tag}-{i}"), "schema_version": 1,
            "node_id": "n1", "collector_id": "c1", "source_id": "s1", "client_id": "codex",
            "adapter_id": "codex", "adapter_version": "0.1.0",
            "model_call_id": id.clone(),
            "timestamp": started.to_rfc3339(),
            "provider_normalized": "openai", "model_normalized": "gpt-5.6-sol",
            "usage": {"input": 1_000, "output": 200},
            "cost": {"calculated_micro_usd": 1_000},
            "quality": {"usage_source": "reported", "granularity": "call", "confidence": 1.0},
        });
        db.insert_usage(&usage, &format!("n1:sess-{tag}")).unwrap();
        // 注：insert_usage 会同时写入一条 ingest 计价匹配，这里不再手插，避免重复计数
        ids.push(id);
    }
    ids
}

fn count(db: &HubDb, sql: &str) -> i64 {
    db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn rollup_calls(db: &HubDb) -> i64 {
    count(
        db,
        "SELECT COALESCE(SUM(model_call_count), 0) FROM hourly_rollups",
    )
}

#[test]
fn archive_disabled_by_default_deletes_nothing() {
    let (_dir, db) = test_db("disabled");
    seed_calls(&db, "old", -30, 12);
    let before_calls = count(&db, "SELECT COUNT(*) FROM model_calls");
    let before_usage = count(&db, "SELECT COUNT(*) FROM usage_events");
    let before_matches = count(&db, "SELECT COUNT(*) FROM pricing_matches");

    let policy = archive::policy(&db);
    assert!(!policy.enabled, "归档必须默认关闭");
    assert_eq!(policy.retention_days, archive::DEFAULT_RETENTION_DAYS);

    let report = archive::run(&db).unwrap();
    assert!(report.skipped, "未启用时应跳过执行");
    assert_eq!(report.deleted, 0, "未启用时不得删除任何行");
    assert_eq!(count(&db, "SELECT COUNT(*) FROM model_calls"), before_calls);
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM usage_events"),
        before_usage
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM pricing_matches"),
        before_matches
    );
    assert!(archive::watermark(&db).is_none(), "从未归档时水位应为空");
}

#[test]
fn set_policy_rejects_unsafe_retention_days() {
    let (_dir, db) = test_db("policy-guard");
    assert!(
        archive::set_policy(&db, true, 0).is_err(),
        "保留 0 天会立刻删光明细，必须拒绝"
    );
    assert!(archive::set_policy(&db, true, -7).is_err());
    assert!(!archive::policy(&db).enabled, "非法配置不得改变开关状态");
    let ok = archive::set_policy(&db, true, 30).unwrap();
    assert_eq!(ok.retention_days, 30);
}

#[test]
fn archive_removes_expired_detail_but_keeps_rollups() {
    let (_dir, db) = test_db("archive-keeps-rollups");
    let old_ids = seed_calls(&db, "old", -40, 8);
    let recent_ids = seed_calls(&db, "new", -1, 5);
    db.rebuild_rollups(90).unwrap();
    let rollup_calls_before = rollup_calls(&db);
    let rollup_tokens_before = count(
        &db,
        "SELECT COALESCE(SUM(output_tokens), 0) FROM hourly_rollups",
    );
    assert!(rollup_calls_before >= 13, "重建应覆盖全部调用");
    let _ = &old_ids;

    archive::set_policy(&db, true, 30).unwrap();
    let report = archive::run(&db).unwrap();
    assert!(!report.skipped, "存在超期明细时不应跳过");
    assert_eq!(
        report.deleted, 24,
        "8 条超期调用 + 8 条用量 + 8 条随之失去归属的计价匹配"
    );

    // 明细：超期的没了，保留期内的还在
    let remaining: Vec<String> = {
        let c = db.conn();
        let mut stmt = c.prepare("SELECT id FROM model_calls ORDER BY id").unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        rows
    };
    for id in &old_ids {
        assert!(!remaining.contains(id), "超期调用 {id} 应被归档删除");
    }
    for id in &recent_ids {
        assert!(remaining.contains(id), "保留期内的调用 {id} 必须保留");
    }

    // 聚合：调用数与 Token 完全不变（总览/趋势依赖它们）
    assert_eq!(
        rollup_calls(&db),
        rollup_calls_before,
        "rollup 调用数不得被改写"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COALESCE(SUM(output_tokens), 0) FROM hourly_rollups"
        ),
        rollup_tokens_before,
        "rollup Token 不得被改写"
    );

    // 水位与状态
    assert!(archive::watermark(&db).is_some(), "归档后必须写入水位");
    let status = archive::status(&db);
    assert_eq!(status["enabled"], json!(true));
    // 小数据量下可能一页都没腾空（SQLite 只回收整页），freed_bytes=0 属诚实计量；
    // 真实释放空间由 freed_bytes_reports_reclaimed_pages 单独验证。
    assert!(
        status["freed_bytes"].as_i64().is_some(),
        "释放空间字段必须存在"
    );
    assert!(
        status["total_deleted"].as_i64().unwrap_or(0) > 0,
        "累计删除行数应可查询"
    );
    // 备份文件确实生成
    let backup = status["backup_path"].as_str().expect("应记录备份路径");
    assert!(
        Path::new(backup).exists(),
        "归档前备份必须真实存在: {backup}"
    );
    assert!(
        backup.ends_with(".db.zst"),
        "备份应为压缩格式，避免占用与库同量级的磁盘"
    );
}

#[test]
fn archive_is_idempotent_and_resumable() {
    let (_dir, db) = test_db("idempotent");
    seed_calls(&db, "old", -60, 4);
    archive::set_policy(&db, true, 7).unwrap();

    let first: ArchiveReport = archive::run(&db).unwrap();
    assert!(!first.skipped);
    assert!(first.deleted > 0);

    // 第二次执行：没有超期数据，应跳过且不重复生成备份
    let backup_after_first = archive::status(&db)["backup_path"].clone();
    let second = archive::run(&db).unwrap();
    assert!(second.skipped, "没有超期明细时应跳过");
    assert_eq!(second.deleted, 0);
    assert_eq!(
        archive::status(&db)["backup_path"],
        backup_after_first,
        "跳过时不应重复备份"
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM model_calls"),
        0,
        "重复执行不得出错或产生额外删除"
    );
}

#[test]
fn backup_failure_aborts_before_any_deletion() {
    let (dir, db) = test_db("backup-fail");
    seed_calls(&db, "old", -90, 6);
    archive::set_policy(&db, true, 7).unwrap();

    // 让备份无法写入快照文件（非 root 下 chmod 生效；root 下无法模拟，跳过该断言）
    let readonly = dir.join("readonly-probe");
    std::fs::create_dir_all(&readonly).unwrap();
    let mut perms = std::fs::metadata(&dir).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o555);
    std::fs::set_permissions(&dir, perms.clone()).unwrap();
    let can_block = std::fs::File::create(dir.join(".probe")).is_err();

    let result = archive::run(&db);

    perms.set_mode(0o755);
    std::fs::set_permissions(&dir, perms).unwrap();
    let _ = std::fs::remove_dir_all(&readonly);

    if !can_block {
        eprintln!("当前用户无法制造只读目录（可能以 root 运行），跳过备份失败断言");
        return;
    }
    assert!(result.is_err(), "备份失败必须返回错误");
    assert!(
        count(&db, "SELECT COUNT(*) FROM model_calls") == 6,
        "备份失败时一行明细都不能删"
    );
    assert!(
        archive::watermark(&db).is_none(),
        "备份失败时不得写入水位，否则界面会谎称已归档"
    );
}

#[test]
fn archive_reclaims_orphan_pricing_matches() {
    let (_dir, db) = test_db("orphan-matches");
    seed_calls(&db, "old", -50, 3);
    let recent = seed_calls(&db, "new", -2, 2);
    archive::set_policy(&db, true, 30).unwrap();
    let report = archive::run(&db).unwrap();
    assert!(!report.skipped);

    // 孤儿匹配被回收
    let orphans = count(
        &db,
        "SELECT COUNT(*) FROM pricing_matches p
         WHERE NOT EXISTS (SELECT 1 FROM usage_events u WHERE u.event_id = p.usage_event_id)",
    );
    assert_eq!(orphans, 0, "归档后不应留下孤儿计价记录");

    // 保留期内的用量行仍各有一条计价记录，费用口径不失真
    for id in &recent {
        let call_num = id.rsplit('-').next().unwrap();
        let has_match = count(
            &db,
            &format!(
                "SELECT COUNT(*) FROM pricing_matches p
                 WHERE p.usage_event_id = 'ue-new-{call_num}'"
            ),
        );
        assert_eq!(has_match, 1, "保留期内每个用量行仍应有计价记录");
    }
}

#[test]
fn sessions_without_remaining_detail_are_archived_too() {
    let (_dir, db) = test_db("sessions");
    seed_calls(&db, "old", -80, 3);
    // insert_call 只存 session_id 字符串（无外键），会话行需显式写入
    let started = (Utc::now() - Duration::days(80)).to_rfc3339();
    db.upsert_session(&json!({
        "source_session_id": "sess-old", "node_id": "n1", "started_at": started,
        "timestamp": started, "status": "ended", "client_id": "codex",
        "collector_id": "c1", "source_id": "s1",
    }))
    .unwrap();
    archive::set_policy(&db, true, 10).unwrap();
    let before_sessions = count(&db, "SELECT COUNT(*) FROM sessions");
    assert!(before_sessions > 0, "应有会话数据");
    archive::run(&db).unwrap();

    let dangling = count(
        &db,
        "SELECT COUNT(*) FROM sessions s
         WHERE NOT EXISTS (SELECT 1 FROM model_calls m WHERE m.session_id = s.id)
           AND NOT EXISTS (SELECT 1 FROM messages ms WHERE ms.session_id = s.id)",
    );
    assert_eq!(dangling, 0, "明细已清空的会话也应归档，不能留下空壳会话");
}

#[test]
fn freed_bytes_reports_reclaimed_pages() {
    let (_dir, db) = test_db("freed-bytes");
    // 造足量数据让表跨多页，删除后才有整页被腾空
    seed_calls(&db, "old", -90, 1_500);
    seed_calls(&db, "new", -1, 50);
    archive::set_policy(&db, true, 30).unwrap();
    let report = archive::run(&db).unwrap();
    assert!(!report.skipped);
    assert!(
        report.deleted >= 3_000,
        "应删除 1500 调用 + 1500 用量 + 1500 匹配: {}",
        report.deleted
    );

    let freed = archive::status(&db)["freed_bytes"]
        .as_i64()
        .expect("应记录释放空间");
    assert!(
        freed > 0,
        "删除上千行并执行 incremental_vacuum 后必须量到真实释放字节数，实际 {freed}"
    );
    assert!(
        archive::status(&db)["earliest_detail"].as_str().is_some(),
        "最早可查明细时间应可查询，供设置页展示"
    );
}
