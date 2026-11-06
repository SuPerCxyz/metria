//! 启动顺序：HTTP 端口必须先于首次性能预聚合全量重建可用。

use chrono::{Duration as ChronoDuration, Utc};
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_storage::rusqlite::params;
use std::time::{Duration, Instant};

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

fn seed_calls(db: &HubDb, count: i64) {
    let base = Utc::now() - ChronoDuration::hours(6);
    let mut c = db.conn();
    let tx = c.transaction().unwrap();
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO model_calls
                 (id, node_id, collector_id, client_id, source_id, session_id, started_at,
                  status, call_granularity, created_at, updated_at)
                 VALUES (?1, 'n1', 'c1', 'codex', 's1', 'n1:sess', ?2,
                         'success', 'call', ?2, ?2)",
            )
            .unwrap();
        for index in 0..count {
            let started = (base + ChronoDuration::seconds(index % 21_600)).to_rfc3339();
            stmt.execute(params![format!("startup-{index}"), started])
                .unwrap();
        }
    }
    tx.commit().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "startup ordering smoke test; run with --ignored --nocapture"]
async fn healthz_responds_before_initial_perf_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_cfg(dir.path());
    let db = HubDb::open(&cfg).unwrap();
    db.apply_migrations().unwrap();
    seed_calls(&db, 100_000);
    assert!(
        metria_hub::perfrollup::needs_full_rebuild(&db).unwrap(),
        "空预聚合 + 有明细应触发首次全量重建"
    );

    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let mut cfg = cfg;
    cfg.listen = format!("127.0.0.1:{port}").parse().unwrap();

    let started = Instant::now();
    let server = tokio::spawn(async move {
        let _ = metria_hub::serve(cfg).await;
    });

    let mut health_elapsed = None;
    while started.elapsed() < Duration::from_secs(5) {
        match ureq::get(&format!("http://127.0.0.1:{port}/healthz")).call() {
            Ok(response) if response.status() == 200 => {
                health_elapsed = Some(started.elapsed());
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
    let health_elapsed = health_elapsed.expect("/healthz 未在 5s 内响应");
    let perf_at_health: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM hourly_performance_rollups", [], |r| {
            r.get(0)
        })
        .unwrap();

    let mut perf_ready = None;
    while started.elapsed() < Duration::from_secs(60) {
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM hourly_performance_rollups", [], |r| {
                r.get(0)
            })
            .unwrap();
        if count > 0 {
            perf_ready = Some(started.elapsed());
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let perf_ready = perf_ready.expect("性能预聚合未在 60s 内完成");
    println!(
        "healthz={health_elapsed:?} perf_ready={perf_ready:?} perf_at_health={perf_at_health}"
    );

    assert!(
        health_elapsed < Duration::from_secs(2),
        "/healthz 被首次全量重建阻塞: {health_elapsed:?}"
    );
    assert!(
        health_elapsed < perf_ready,
        "/healthz 应在预聚合完成前响应: health={health_elapsed:?} perf={perf_ready:?}"
    );

    server.abort();
}
