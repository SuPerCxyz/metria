//! D 阶段索引验证：迁移可执行、查询计划命中、合成数据基准。

use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_storage::rusqlite::{params, Connection};
use metria_storage::{load_embedded, migrate, open, DbOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn test_cfg(dir: &Path) -> HubConfig {
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

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("metria-query-index-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn test_db(tag: &str) -> HubDb {
    let dir = temp_dir(tag);
    let db = HubDb::open(&test_cfg(&dir)).expect("open db");
    db.apply_migrations().expect("migrate");
    db
}

fn index_names(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA index_list('{table}')"))
        .unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
    rows.filter_map(Result::ok).collect()
}

fn index_columns(conn: &Connection, index: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA index_info('{index}')"))
        .unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(2)).unwrap();
    rows.filter_map(Result::ok).collect()
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |r| r.get(0),
    )
    .unwrap()
}

fn explain(db: &HubDb, sql: &str) -> String {
    let c = db.read_conn();
    let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(3)).unwrap();
    rows.filter_map(Result::ok).collect::<Vec<_>>().join("\n")
}

fn seed_rows(conn: &mut Connection, count: i64) {
    let now = chrono::Utc::now();
    let tx = conn.transaction().unwrap();
    {
        let mut calls = tx
            .prepare(
                "INSERT INTO model_calls
                 (id, node_id, collector_id, client_id, source_id, session_id, started_at,
                  status, call_granularity, created_at, updated_at)
                 VALUES (?1, 'n1', 'c1', ?2, 's1', 'n1:s1', ?3, 'success', 'call', ?3, ?3)",
            )
            .unwrap();
        for i in 0..count {
            let client = if i % 2 == 0 { "codex" } else { "claude-code" };
            let ts = (now - chrono::Duration::seconds(i * 40)).to_rfc3339();
            calls
                .execute(params![format!("call-{i}"), client, ts])
                .unwrap();
        }
    }
    {
        let mut usage = tx
            .prepare(
                "INSERT INTO usage_events
                 (event_id, schema_version, node_id, collector_id, source_id, client_id,
                  adapter_id, adapter_version, timestamp, usage_source, usage_granularity)
                 VALUES (?1, 1, 'n1', 'c1', 's1', ?2, 'codex', '0.1.0', ?3, 'reported', 'call')",
            )
            .unwrap();
        for i in 0..count {
            let client = if i % 2 == 0 { "codex" } else { "claude-code" };
            let ts = (now - chrono::Duration::seconds(i * 40)).to_rfc3339();
            usage
                .execute(params![format!("usage-{i}"), client, ts])
                .unwrap();
        }
    }
    tx.commit().unwrap();
}

#[test]
fn migration_creates_query_indexes_on_empty_db() {
    let dir = temp_dir("empty");
    let path = dir.join("hub.db");
    let mut conn = open(&path, &DbOptions::default()).unwrap();
    let migrations = load_embedded().unwrap();
    let applied = migrate(&mut conn, &migrations, Some(28)).unwrap();

    assert!(applied.contains(&27), "027 应被应用: {applied:?}");
    assert!(applied.contains(&28), "028 应被应用: {applied:?}");
    assert!(table_exists(&conn, "perf_rollup_dirty_hours"));
    assert!(index_names(&conn, "sessions").contains(&"idx_sessions_activity".to_string()));
    assert!(index_names(&conn, "sessions").contains(&"idx_sessions_source_session".to_string()));
    assert!(index_names(&conn, "sessions").contains(&"idx_sessions_sort_at".to_string()));
    assert!(index_names(&conn, "model_calls").contains(&"idx_calls_client_time".to_string()));
    assert!(index_names(&conn, "usage_events").contains(&"idx_usage_client_time".to_string()));
    assert_eq!(
        index_columns(&conn, "idx_calls_client_time"),
        vec!["client_id", "started_at"]
    );
    assert_eq!(
        index_columns(&conn, "idx_usage_client_time"),
        vec!["client_id", "timestamp"]
    );
}

#[test]
fn migration_applies_to_database_with_existing_rows() {
    let dir = temp_dir("existing");
    let path = dir.join("hub.db");
    let mut conn = open(&path, &DbOptions::default()).unwrap();
    let migrations = load_embedded().unwrap();
    migrate(&mut conn, &migrations, Some(26)).unwrap();

    conn.execute(
        "INSERT INTO sessions
         (id, source_session_id, node_id, collector_id, source_id, client_id, started_at,
          last_activity_at, created_at, updated_at)
         VALUES ('n1:s1', 's1', 'n1', 'c1', 'src1', 'codex',
                 '2026-01-01T00:00:00Z', '2026-01-01T01:00:00Z',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO model_calls
         (id, node_id, collector_id, client_id, source_id, session_id, started_at,
          status, call_granularity, created_at, updated_at)
         VALUES ('call-1', 'n1', 'c1', 'codex', 'src1', 'n1:s1',
                 '2026-01-01T00:10:00Z', 'success', 'call',
                 '2026-01-01T00:10:00Z', '2026-01-01T00:10:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO usage_events
         (event_id, schema_version, node_id, collector_id, source_id, client_id,
          adapter_id, adapter_version, timestamp, usage_source, usage_granularity)
         VALUES ('u1', 1, 'n1', 'c1', 'src1', 'codex', 'codex', '0.1.0',
                 '2026-01-01T00:10:30Z', 'reported', 'call')",
        [],
    )
    .unwrap();

    let applied = migrate(&mut conn, &migrations, Some(28)).unwrap();
    assert_eq!(applied, vec![27, 28]);

    let count = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count("sessions"), 1);
    assert_eq!(count("model_calls"), 1);
    assert_eq!(count("usage_events"), 1);
    assert!(table_exists(&conn, "perf_rollup_dirty_hours"));
    assert!(index_names(&conn, "sessions").contains(&"idx_sessions_activity".to_string()));
    assert!(index_names(&conn, "sessions").contains(&"idx_sessions_source_session".to_string()));
    assert!(index_names(&conn, "sessions").contains(&"idx_sessions_sort_at".to_string()));
    assert!(index_names(&conn, "model_calls").contains(&"idx_calls_client_time".to_string()));
    assert!(index_names(&conn, "usage_events").contains(&"idx_usage_client_time".to_string()));
}

#[test]
fn query_plans_use_new_indexes() {
    let db = test_db("plans");

    let sessions = explain(
        &db,
        "SELECT s.id FROM sessions s
         WHERE ((s.started_at >= '2026-01-01T00:00:00Z' AND s.started_at < '2026-02-01T00:00:00Z')
             OR (s.last_activity_at >= '2026-01-01T00:00:00Z' AND s.last_activity_at < '2026-02-01T00:00:00Z'))
           AND s.parent_session_id IS NULL
         ORDER BY s.started_at DESC, s.id DESC LIMIT 100",
    );
    assert!(
        sessions.contains("idx_sessions_activity"),
        "sessions OR 应命中活动索引: {sessions}"
    );
    assert!(
        sessions.contains("idx_sessions_time"),
        "sessions OR 应同时命中开始时间索引: {sessions}"
    );

    let session_lookup = explain(
        &db,
        "SELECT id FROM sessions WHERE source_session_id = 's1' OR id = 's1' LIMIT 1",
    );
    assert!(
        session_lookup.contains("idx_sessions_source_session"),
        "会话 ULID 解析应命中 source_session_id 索引: {session_lookup}"
    );

    let call_end_sessions = explain(
        &db,
        "SELECT s.id FROM sessions s
         WHERE ((COALESCE(s.ended_at, s.last_activity_at, s.started_at) >= '2026-01-01T00:00:00Z'
                 AND COALESCE(s.ended_at, s.last_activity_at, s.started_at) < '2026-02-01T00:00:00Z')
             OR (s.last_activity_at >= '2026-01-01T00:00:00Z'
                 AND s.last_activity_at < '2026-02-01T00:00:00Z'))
         ORDER BY COALESCE(s.ended_at, s.last_activity_at, s.started_at) DESC, s.id DESC LIMIT 100",
    );
    assert!(
        call_end_sessions.contains("idx_sessions_sort_at"),
        "call_end 会话 OR 应命中会话结束时间表达式索引: {call_end_sessions}"
    );

    let calls = explain(
        &db,
        "SELECT COUNT(*) FROM model_calls
         WHERE client_id = 'codex'
           AND started_at >= '2026-01-01T00:00:00Z'
           AND started_at < '2026-02-01T00:00:00Z'",
    );
    assert!(
        calls.contains("idx_calls_client_time"),
        "calls client+time 应命中复合索引: {calls}"
    );

    let usage = explain(
        &db,
        "SELECT COUNT(*) FROM usage_events
         WHERE client_id = 'codex'
           AND timestamp >= '2026-01-01T00:00:00Z'
           AND timestamp < '2026-02-01T00:00:00Z'",
    );
    assert!(
        usage.contains("idx_usage_client_time"),
        "usage client+time 应命中复合索引: {usage}"
    );

    // filter-options 明细回退是纯时间范围扫描，应继续命中时间索引而不是全表扫描。
    let filter_fallback = explain(
        &db,
        "SELECT id FROM (
             SELECT mc.client_id AS id FROM model_calls mc
             WHERE mc.started_at >= '2026-01-01T00:00:00Z'
               AND mc.started_at < '2026-02-01T00:00:00Z'
               AND mc.client_id IS NOT NULL AND mc.client_id != ''
             UNION ALL
             SELECT ue.client_id AS id FROM usage_events ue
             WHERE ue.timestamp >= '2026-01-01T00:00:00Z'
               AND ue.timestamp < '2026-02-01T00:00:00Z'
               AND ue.client_id IS NOT NULL AND ue.client_id != ''
         ) GROUP BY id",
    );
    assert!(
        filter_fallback.contains("idx_calls_time"),
        "filter-options calls 回退应命中时间索引: {filter_fallback}"
    );
    assert!(
        filter_fallback.contains("idx_usage_time"),
        "filter-options usage 回退应命中时间索引: {filter_fallback}"
    );
}

fn timed(db: &HubDb, sql: &str) -> Duration {
    let started = Instant::now();
    let _: i64 = db.read_conn().query_row(sql, [], |r| r.get(0)).unwrap();
    started.elapsed()
}

#[test]
#[ignore = "manual synthetic benchmark; run with --ignored --nocapture"]
fn client_filtered_30d_benchmark() {
    let db = test_db("bench");
    {
        let mut c = db.conn();
        seed_rows(&mut c, 50_000);
    }
    db.conn().execute_batch("ANALYZE").unwrap();

    let from = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let to = chrono::Utc::now().to_rfc3339();
    let calls_sql = format!(
        "SELECT COUNT(*) FROM model_calls WHERE client_id = 'codex'
         AND started_at >= '{from}' AND started_at < '{to}'"
    );
    let calls_full_sql = format!(
        "SELECT COUNT(*) FROM model_calls NOT INDEXED WHERE client_id = 'codex'
         AND started_at >= '{from}' AND started_at < '{to}'"
    );
    let usage_sql = format!(
        "SELECT COUNT(*) FROM usage_events WHERE client_id = 'codex'
         AND timestamp >= '{from}' AND timestamp < '{to}'"
    );
    let usage_full_sql = format!(
        "SELECT COUNT(*) FROM usage_events NOT INDEXED WHERE client_id = 'codex'
         AND timestamp >= '{from}' AND timestamp < '{to}'"
    );

    let calls_indexed = timed(&db, &calls_sql);
    let calls_full = timed(&db, &calls_full_sql);
    let usage_indexed = timed(&db, &usage_sql);
    let usage_full = timed(&db, &usage_full_sql);
    println!(
        "30d client-filtered (50k rows each): calls indexed={calls_indexed:?} full={calls_full:?}; \
         usage indexed={usage_indexed:?} full={usage_full:?}"
    );
    assert!(calls_indexed < calls_full, "calls 索引应快于全表扫描");
    assert!(usage_indexed < usage_full, "usage 索引应快于全表扫描");
}

#[test]
#[ignore = "manual migration timing; run with --ignored --nocapture"]
fn migration_027_build_time_on_existing_data() {
    let dir = temp_dir("build-time");
    let path = dir.join("hub.db");
    let mut conn = open(&path, &DbOptions::default()).unwrap();
    let migrations = load_embedded().unwrap();
    migrate(&mut conn, &migrations, Some(26)).unwrap();
    seed_rows(&mut conn, 50_000);

    let pages_before: i64 = conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let wal_before = std::fs::metadata(format!("{}-wal", path.display()))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let started = Instant::now();
    let applied = migrate(&mut conn, &migrations, Some(27)).unwrap();
    let elapsed = started.elapsed();
    let pages_after: i64 = conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    let wal_after = std::fs::metadata(format!("{}-wal", path.display()))
        .map(|meta| meta.len())
        .unwrap_or(0);
    println!(
        "027 applied={applied:?} elapsed={elapsed:?} pages={pages_before}->{pages_after} \
         wal_before={wal_before} wal_after={wal_after}"
    );
    assert_eq!(applied, vec![27]);
}

#[test]
fn migration_028_backfills_source_session_index() {
    let dir = temp_dir("028-backfill");
    let path = dir.join("hub.db");
    let mut conn = open(&path, &DbOptions::default()).unwrap();
    let migrations = load_embedded().unwrap();
    migrate(&mut conn, &migrations, Some(27)).unwrap();
    conn.execute("DROP INDEX IF EXISTS idx_sessions_source_session", [])
        .unwrap();
    assert!(!index_names(&conn, "sessions").contains(&"idx_sessions_source_session".to_string()));

    let applied = migrate(&mut conn, &migrations, Some(28)).unwrap();
    assert_eq!(applied, vec![28]);
    assert!(index_names(&conn, "sessions").contains(&"idx_sessions_source_session".to_string()));
    assert!(index_names(&conn, "sessions").contains(&"idx_sessions_sort_at".to_string()));
}
