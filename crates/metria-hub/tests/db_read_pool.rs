//! A 阶段：只读连接池与 WAL 读写分离。

use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use metria_hub::api::{app_router, AppState, SseHub};
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;

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
    let dir = std::env::temp_dir().join(format!("metria-readpool-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = HubDb::open(&test_cfg(&dir)).unwrap();
    db.apply_migrations().unwrap();
    db
}

#[test]
fn read_pool_is_query_only_and_falls_back_before_init() {
    let db = test_db("query-only");
    // 未初始化时回退写连接，旧测试与旧调用仍可用。
    let fallback = db.read_conn();
    let mode: i64 = fallback
        .query_row("PRAGMA query_only", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, 0, "回退写连接时 query_only 为 0");
    drop(fallback);

    db.init_read_pool().unwrap();
    assert!((2..=4).contains(&db.read_pool_size()));
    let pooled = db.read_conn();
    let mode: i64 = pooled
        .query_row("PRAGMA query_only", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, 1, "读池连接必须 query_only");
}

#[test]
fn reader_is_not_blocked_by_writer_transaction() {
    let db = test_db("writer-lock");
    db.init_read_pool().unwrap();
    let mut writer = db.conn();
    let tx = writer.transaction().unwrap();
    tx.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES ('readpool-test', '1', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    let started = Instant::now();
    let reader = db.read_conn();
    let count: i64 = reader
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key = 'readpool-test'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let elapsed = started.elapsed();
    // 未提交的写入对读连接不可见，但读连接不得等待 writer 事务释放。
    assert_eq!(count, 0);
    assert!(
        elapsed < Duration::from_secs(2),
        "读查询被 writer 事务阻塞: {elapsed:?}"
    );
    drop(reader);
    drop(tx);
    drop(writer);
}

#[test]
fn ten_concurrent_queries_use_the_read_pool() {
    let db = test_db("concurrent");
    db.init_read_pool().unwrap();
    let db = Arc::new(db);

    let query =
        "WITH RECURSIVE cnt(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM cnt WHERE x < 50000) \
                 SELECT COUNT(*) FROM cnt";
    let single = {
        let started = Instant::now();
        let c = db.read_conn();
        let _: i64 = c.query_row(query, [], |r| r.get(0)).unwrap();
        started.elapsed()
    };

    let barrier = Arc::new(Barrier::new(10));
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..10 {
            let db = Arc::clone(&db);
            let barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                barrier.wait();
                let c = db.read_conn();
                let _: i64 = c.query_row(query, [], |r| r.get(0)).unwrap();
            });
        }
    });
    let parallel = started.elapsed();
    println!("single={single:?} parallel10={parallel:?}");
    // 10 个查询、4 条只读连接：串行会接近 10×single，这里应明显低于该上界。
    assert!(
        parallel < single * 8,
        "并发读未复用连接池: single={single:?}, parallel={parallel:?}"
    );
}

#[test]
fn read_pool_init_is_idempotent() {
    let db = test_db("idempotent");
    db.init_read_pool().unwrap();
    db.init_read_pool().unwrap();
    assert!((2..=4).contains(&db.read_pool_size()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn healthz_stays_responsive_during_long_query() {
    let dir = std::env::temp_dir().join(format!("metria-readpool-health-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = test_cfg(&dir);
    let db = HubDb::open(&cfg).unwrap();
    db.apply_migrations().unwrap();
    db.init_read_pool().unwrap();

    let state = AppState {
        db: db.clone(),
        cfg,
        sse: SseHub::new(),
        sessions: Default::default(),
        oidc: Default::default(),
        collector_token: None,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app_router(state)).await;
    });

    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = Arc::clone(&barrier);
    let worker_db = db.clone();
    let long_query = std::thread::spawn(move || {
        worker_barrier.wait();
        let c = worker_db.read_conn();
        let _: i64 = c
            .query_row(
                "WITH RECURSIVE cnt(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM cnt WHERE x < 500000) \
                 SELECT COUNT(*) FROM cnt",
                [],
                |r| r.get(0),
            )
            .unwrap();
    });
    barrier.wait();

    let started = Instant::now();
    let status = ureq::get(&format!("http://{addr}/healthz"))
        .call()
        .unwrap()
        .status();
    let elapsed = started.elapsed();
    assert_eq!(status, 200);
    assert!(
        elapsed < Duration::from_secs(2),
        "/healthz 被长查询阻塞: {elapsed:?}"
    );
    long_query.join().unwrap();
}
