//! 首页「活跃时长 / 平均调用时长」按窗口裁剪的回归（spec `dashboard-usage-insights`：
//! 场景「调用跨越窗口终点」「窗口终点不早于当前时刻」）。
//!
//! 背景：这两个指标曾只按 `started_at` 过滤、不把单条 duration 裁到 `to`，导致
//! 一条"范围内开始、范围结束后才完成"的调用把超出部分也算进来，与「会话总时长」
//! 已按边界裁剪的口径不一致。

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
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
    let cfg = test_cfg(dir);
    let db = HubDb::open(&cfg).expect("open db");
    db.apply_migrations().expect("migrate");
    let state = AppState {
        db,
        cfg,
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

fn login(base: &str) -> String {
    let resp = ureq::post(&format!("{base}/api/v1/auth/login"))
        .send_json(json!({ "username": "admin", "password": "change-me-please" }))
        .unwrap();
    resp.into_json::<Value>().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

/// 写入一条带 `duration_ms` 的调用。
fn seed_call(state: &AppState, id: &str, started: DateTime<Utc>, duration_ms: i64) {
    state
        .db
        .insert_call(
            &json!({
                "id": id, "source_call_id": id,
                "node_id": "n1", "collector_id": "c1", "client_id": "codex",
                "source_id": "s1", "session_id": format!("sess-{id}"),
                "model_normalized": "gpt-5.6-sol", "provider_normalized": "openai",
                "started_at": started.to_rfc3339(),
                "completed_at": (started + Duration::milliseconds(duration_ms)).to_rfc3339(),
                "duration_ms": duration_ms,
                "status": "success", "call_granularity": "call",
                "input_tokens": 100, "output_tokens": 50,
            }),
            &format!("n1:sess-{id}"),
        )
        .unwrap();
}

/// 取总览；时间戳一律用 `Z` 后缀（`+00:00` 在 query 中会被解码成空格）。
fn overview(base: &str, token: &str, from: &str, to: Option<&str>) -> Value {
    let mut url = format!("{base}/api/v1/overview?from={from}");
    if let Some(to) = to {
        url.push_str(&format!("&to={to}"));
    }
    let resp = ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap();
    resp.into_json::<Value>().unwrap()
}

fn current_hour() -> DateTime<Utc> {
    let now = Utc::now();
    Utc.with_ymd_and_hms(now.year(), now.month(), now.day(), now.hour(), 0, 0)
        .unwrap()
}

/// 窗口终点在过去时，跨越终点的调用只计窗口内的部分。
#[tokio::test(flavor = "multi_thread")]
async fn active_duration_clips_calls_that_straddle_the_window_end() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    let token = login(&base);

    // 历史窗口 [12:00, 13:00)：终点早于当前时刻 → 必须裁剪
    let win_from = current_hour() - Duration::hours(2);
    let win_to = win_from + Duration::hours(1);

    // X：12:30 开始、声明 120 分钟 → 应只计 30 分钟（否则 120 分钟）
    seed_call(
        &state,
        "clip-x",
        win_to - Duration::minutes(30),
        120 * 60 * 1000,
    );
    // Y：12:05 开始、5 分钟 → 完整落在窗口内
    seed_call(
        &state,
        "clip-y",
        win_from + Duration::minutes(5),
        5 * 60 * 1000,
    );
    // Z：窗口之前开始 → 不得计入
    seed_call(
        &state,
        "clip-z",
        win_from - Duration::hours(2),
        60 * 60 * 1000,
    );

    let ov = overview(
        &base,
        &token,
        &win_from.to_rfc3339().replace("+00:00", "Z"),
        Some(&win_to.to_rfc3339().replace("+00:00", "Z")),
    );

    assert_eq!(
        ov["active_duration_ms"],
        json!(35 * 60 * 1000),
        "应为 30 分钟（裁剪）+ 5 分钟，而不是 120 分钟；实际 {:?}",
        ov["active_duration_ms"]
    );
    assert_ne!(
        ov["active_duration_ms"],
        json!(125 * 60 * 1000),
        "未裁剪值（120+5 分钟）不应出现"
    );
    assert_eq!(
        ov["avg_duration_ms"],
        json!((35 * 60 * 1000) / 2),
        "平均值必须与裁剪后的合计/计数自洽"
    );
    // 分位数仍描述完整调用时长分布，且标注为近似（design D2）
    assert_eq!(ov["quantile_approx"], json!(true));
    assert!(
        ov["duration_p95_ms"].is_number(),
        "分位数应来自完整调用时长分布"
    );
}

/// 终点不早于当前时刻时没有调用可跨越窗口 → 无需裁剪，且仍走预聚合路径。
#[tokio::test(flavor = "multi_thread")]
async fn window_ending_now_needs_no_clip() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    let token = login(&base);

    // 只传 from，to 默认为 now → needs_clip = false
    let from = Utc::now() - Duration::hours(2);
    seed_call(
        &state,
        "now-y",
        Utc::now() - Duration::minutes(10),
        5 * 60 * 1000,
    );

    let ov = overview(
        &base,
        &token,
        &from.to_rfc3339().replace("+00:00", "Z"),
        None,
    );
    assert_eq!(
        ov["active_duration_ms"],
        json!(5 * 60 * 1000),
        "窗内调用的完整时长应等于裁剪后时长（无调用可跨越 now）: {:?}",
        ov["active_duration_ms"]
    );
    assert_eq!(ov["avg_duration_ms"], json!(5 * 60 * 1000));
    assert_eq!(ov["quantile_approx"], json!(true));
}

/// 归档区间：活动类字段仍为 null + 标记，文案改动不得覆盖该行为。
#[tokio::test(flavor = "multi_thread")]
async fn archived_range_keeps_honest_nulls() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    let token = login(&base);

    let day = Utc::now() - Duration::days(5);
    let win_from = Utc
        .with_ymd_and_hms(day.year(), day.month(), day.day(), 4, 0, 0)
        .unwrap();
    let win_to = win_from + Duration::hours(2);
    seed_call(
        &state,
        "arch-x",
        win_from + Duration::minutes(5),
        5 * 60 * 1000,
    );

    metria_hub::archive::set_policy(&state.db, true, 1).unwrap();
    let report = metria_hub::archive::run(&state.db).unwrap();
    assert!(!report.skipped && report.deleted > 0, "应确实归档了明细");

    let ov = overview(
        &base,
        &token,
        &win_from.to_rfc3339().replace("+00:00", "Z"),
        Some(&win_to.to_rfc3339().replace("+00:00", "Z")),
    );
    assert!(
        ov["activity_archived_before"].as_str().is_some(),
        "归档区间必须携带标记: {}",
        ov["activity_archived_before"]
    );
    assert!(
        ov["active_duration_ms"].is_null(),
        "归档区间活动类指标必须为 null 而非 0: {:?}",
        ov["active_duration_ms"]
    );
}
