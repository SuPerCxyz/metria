//! 数据质量页「调用时长口径异常检测」测试（spec `frontend-data-coverage`）。
//!
//! 三类检查各有独立阈值（24h 单条、首响应不自洽、同回合重复累计 5 倍），
//! 并给出「>6h 调用占总时长比例」的口径统计；文案 MUST NOT 表述为真实模型响应时长。

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

fn iso(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339().replace("+00:00", "Z")
}

fn seed(
    state: &AppState,
    id: &str,
    session: &str,
    started: DateTime<Utc>,
    duration_ms: i64,
    first_response_at: Option<DateTime<Utc>>,
) {
    state
        .db
        .insert_call(
            &json!({
                "id": id, "source_call_id": id,
                "node_id": "n1", "collector_id": "c1", "client_id": "opencode",
                "source_id": "s1", "session_id": session,
                "model_normalized": "mimo-v2.5", "provider_normalized": "openai",
                "started_at": started.to_rfc3339(),
                "completed_at": (started + Duration::milliseconds(duration_ms)).to_rfc3339(),
                "first_response_at": first_response_at.map(|t| t.to_rfc3339()),
                "duration_ms": duration_ms,
                "call_granularity": "message",
                "timing_source": "opencode_message_timestamps",
                "status": "success", "call_granularity_note": "",
                "input_tokens": 100, "output_tokens": 50,
            }),
            &format!("n1:{session}"),
        )
        .unwrap();
}

fn data_quality(base: &str, token: &str, from: &str, to: &str) -> Value {
    let resp = ureq::get(&format!("{base}/api/v1/data-quality?from={from}&to={to}"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap();
    resp.into_json::<Value>().unwrap()
}

fn check_count(outliers: &Value, key: &str) -> i64 {
    outliers["checks"]
        .as_array()
        .expect("checks 应为数组")
        .iter()
        .find(|c| c["key"] == json!(key))
        .unwrap_or_else(|| panic!("缺少检查项 {key}"))["count"]
        .as_i64()
        .unwrap_or(-1)
}

#[tokio::test(flavor = "multi_thread")]
async fn duration_outliers_reports_three_checks_and_share() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    let token = login(&base);

    let now = Utc::now();
    let hour = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), now.hour(), 0, 0)
        .unwrap();
    let from = hour - Duration::hours(6);
    let to = hour + Duration::hours(1);

    // 1) 单条 25 小时 → over_24h
    seed(
        &state,
        "o24",
        "sess-a",
        now - Duration::hours(3),
        25 * 3_600_000,
        None,
    );
    // C8 防线（insert_call 把 >24h 降级为 NULL）已让新写入无法产生 over_24h 行，
    // 这里直连 SQL 回填，模拟防线部署前的 legacy 存量行——over_24h 检查此后只服务于
    // 「存量未修」与「防线回归」两种情况，必须仍能检出。
    state
        .db
        .conn()
        .execute(
            "UPDATE model_calls SET duration_ms = ?1 WHERE id = 'o24'",
            [25 * 3_600_000],
        )
        .unwrap();
    // 2) 首响应距起点 2 小时（且远超该调用 1 分钟时长）→ ttft_invalid
    seed(
        &state,
        "ttft-bad",
        "sess-b",
        now - Duration::hours(2),
        60_000,
        Some(now - Duration::hours(2) + Duration::hours(2)),
    );
    // 3) 6 条调用共享同一 started_at、各 10 分钟 → 组内累计 6×10min > 5×10min → duplicate_start
    for i in 0..6 {
        seed(
            &state,
            &format!("dup-{i}"),
            "sess-shared",
            now - Duration::hours(5),
            600_000,
            None,
        );
    }

    let dq = data_quality(&base, &token, &iso(from), &iso(to));
    let outliers = &dq["duration_outliers"];

    // 既有区块不受影响（回归）
    assert!(dq.get("usage_distribution").is_some(), "既有字段应保留");
    assert!(dq.get("clock_skew_warnings").is_some(), "既有字段应保留");
    assert!(dq.get("alerts").is_some(), "既有字段应保留");

    assert!(
        outliers["label"]
            .as_str()
            .unwrap_or_default()
            .contains("不等同于真实模型响应时长"),
        "文案必须诚实标注: {}",
        outliers["label"]
    );
    assert_eq!(
        check_count(outliers, "over_24h"),
        1,
        "应检出 1 条 25 小时调用"
    );
    assert_eq!(
        check_count(outliers, "ttft_invalid"),
        1,
        "应检出 1 条首响应不自洽"
    );
    assert_eq!(
        check_count(outliers, "duplicate_start"),
        1,
        "应检出 1 个同回合重复累计分组"
    );
    let share = outliers["share_of_total_duration"].as_f64().unwrap();
    assert!(share > 0.0 && share <= 1.0, "占比应在 (0,1]，实际 {share}");
    assert!(
        outliers["total_duration_hours"].as_f64().unwrap() > 25.0,
        "总时长应包含那条 25 小时调用"
    );
    let by_client = outliers["by_client"]
        .as_array()
        .expect("by_client 应为数组");
    assert!(!by_client.is_empty(), "应给出按客户端的分布");
    assert_eq!(by_client[0]["client_id"], json!("opencode"));
    assert_eq!(by_client[0]["call_granularity"], json!("message"));
    assert_eq!(
        by_client[0]["timing_source"],
        json!("opencode_message_timestamps")
    );

    // 无异常的范围 → 三类计数全 0（前端据此显示正常态而不是 0 条告警）
    let quiet_from = hour - Duration::days(30);
    let quiet_to = hour - Duration::days(29);
    let quiet = data_quality(&base, &token, &iso(quiet_from), &iso(quiet_to));
    let quiet_outliers = &quiet["duration_outliers"];
    for key in ["over_24h", "ttft_invalid", "duplicate_start"] {
        assert_eq!(
            check_count(quiet_outliers, key),
            0,
            "无数据范围的 {key} 应为 0"
        );
    }
    assert_eq!(
        quiet_outliers["share_of_total_duration"].as_f64().unwrap(),
        0.0
    );
}

/// C8 软降级防线：超过 24h 的单次调用入库即降级为 NULL，且不拒收整批。
#[tokio::test(flavor = "multi_thread")]
async fn over_long_duration_is_downgraded_not_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    let _token = login(&base);

    let now = Utc::now();
    // 一条 25 小时的异常调用 + 一条正常调用，同批次写入
    seed(
        &state,
        "over-25h",
        "sess-guard",
        now - Duration::hours(3),
        25 * 3_600_000,
        None,
    );
    seed(
        &state,
        "normal",
        "sess-guard",
        now - Duration::hours(1),
        90_000,
        None,
    );

    let (duration, quality): (Option<i64>, Option<String>) = {
        let c = state.db.conn();
        c.query_row(
            "SELECT duration_ms, timing_quality FROM model_calls WHERE id = 'over-25h'",
            [],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .unwrap()
    };
    assert_eq!(
        duration, None,
        "超过 24h 的时长必须降级为 NULL（缺失用 null、禁止保留不可信值）"
    );
    assert_eq!(
        quality.as_deref(),
        Some("unavailable"),
        "降级后时长质量必须标为不可用，不得沿用 observed"
    );

    // 同批次的正常调用不受影响（不拒收整批）
    let normal: (Option<i64>, Option<String>) = state
        .db
        .conn()
        .query_row(
            "SELECT duration_ms, timing_quality FROM model_calls WHERE id = 'normal'",
            [],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .unwrap();
    assert_eq!(normal.0, Some(90_000), "正常调用的时长应原样保留");
}
