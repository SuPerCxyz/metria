//! 归档配置 API 的鉴权、参数校验与列表接口回归。
//!
//! 其中「列表接口返回 `archived_before`」同时是**死锁回归**：watermark 需要取连接锁
//! 读 settings，而 `std::sync::Mutex` 不可重入，若在已持有 `db.conn()` 时读取，
//! `/api/v1/calls`、`/api/v1/sessions` 首次调用就会把整个 Hub 的 DB 锁永久卡死
//! （Code Review 的阻塞项 B1，e2e 曾以挂死形式暴露）。

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

fn get(base: &str, path: &str, token: Option<&str>) -> (u16, Value) {
    let mut req = ureq::get(&format!("{base}{path}"));
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    match req.call() {
        Ok(resp) => {
            let status = resp.status();
            (status, resp.into_json::<Value>().unwrap_or(Value::Null))
        }
        Err(ureq::Error::Status(code, resp)) => {
            (code, resp.into_json::<Value>().unwrap_or(Value::Null))
        }
        Err(e) => panic!("请求 {path} 失败: {e}"),
    }
}

fn put(base: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    match ureq::put(&format!("{base}{path}"))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(body)
    {
        Ok(resp) => (
            resp.status(),
            resp.into_json::<Value>().unwrap_or(Value::Null),
        ),
        Err(ureq::Error::Status(code, resp)) => {
            (code, resp.into_json::<Value>().unwrap_or(Value::Null))
        }
        Err(e) => panic!("请求 {path} 失败: {e}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn archive_settings_api_enforces_auth_validation_and_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;

    // 1) 未登录一律 401
    let (code, _) = get(&base, "/api/v1/settings/archive", None);
    assert_eq!(code, 401, "未登录读取归档配置应 401");

    let token = login(&base);

    // 2) 登录后可读，默认关闭
    let (code, body) = get(&base, "/api/v1/settings/archive", Some(&token));
    assert_eq!(code, 200, "登录后应可读归档配置");
    assert_eq!(body["policy"]["enabled"], json!(false), "归档必须默认关闭");
    assert!(
        body["status"]["label"]
            .as_str()
            .unwrap_or_default()
            .contains("全量保留"),
        "未启用时应如实显示全量保留: {body}"
    );

    // 3) 保留天数 <1 拒绝（避免被配成「立刻删光」）
    let (code, body) = put(
        &base,
        "/api/v1/settings/archive",
        &token,
        json!({ "enabled": true, "retention_days": 0 }),
    );
    assert_eq!(code, 400, "保留 0 天必须被拒: {body}");
    assert!(
        !metria_hub::archive::policy(&state.db).enabled,
        "非法配置不得改变开关状态"
    );

    // 4) 列表接口必须返回 archived_before，且**不能**在这里死锁
    for path in [
        "/api/v1/calls?from=2026-09-01T00:00:00Z&to=2026-10-01T00:00:00Z",
        "/api/v1/sessions?from=2026-09-01T00:00:00Z&to=2026-10-01T00:00:00Z",
    ] {
        let (code, body) = get(&base, path, Some(&token));
        assert_eq!(code, 200, "{path} 应正常返回（若死锁会直接挂起本测试）");
        assert!(
            body.get("archived_before").is_some(),
            "{path} 必须回传 archived_before 字段供前端标注已归档: {body}"
        );
        assert_eq!(
            body["archived_before"],
            Value::Null,
            "从未归档时水位应为 null"
        );
    }

    // 5) 非管理员不可修改（归档会开启不可逆删除，不能与普通登录等权）
    state
        .db
        .conn()
        .execute(
            "UPDATE users SET role = 'user' WHERE username = 'admin'",
            [],
        )
        .unwrap();
    let (code, body) = put(
        &base,
        "/api/v1/settings/archive",
        &token,
        json!({ "enabled": true, "retention_days": 365 }),
    );
    assert_eq!(code, 403, "非管理员修改归档策略应 403: {body}");
    state
        .db
        .conn()
        .execute(
            "UPDATE users SET role = 'admin' WHERE username = 'admin'",
            [],
        )
        .unwrap();

    // 6) 管理员合法保存（关闭状态，不会触发删除）
    let (code, body) = put(
        &base,
        "/api/v1/settings/archive",
        &token,
        json!({ "enabled": false, "retention_days": 365 }),
    );
    assert_eq!(code, 200, "管理员保存应成功: {body}");
    assert_eq!(body["policy"]["retention_days"], json!(365));
    assert_eq!(body["policy"]["enabled"], json!(false));

    // 7) 系统信息同步暴露归档状态
    let (_, info) = get(&base, "/api/v1/system/info", Some(&token));
    assert_eq!(info["archive"]["enabled"], json!(false));
    assert_eq!(info["archive"]["retention_days"], json!(365));
    assert_eq!(
        info["retention"]["automatic_cleanup"],
        json!(false),
        "retention 兼容字段必须与归档真实状态一致"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn overview_aggregates_survive_archiving_and_activity_becomes_null() {
    // spec `data-archival`：聚合支撑的汇总（Token/调用数/会话数/费用）归档前后完全一致；
    // 依赖明细的活动类指标归档后返回 null + activity_archived_before，绝不给偏小的假数字。
    let dir = tempfile::tempdir().unwrap();
    let (base, state) = spawn_hub(dir.path()).await;
    let token = login(&base);

    // 造 3 天前的整点区间数据（2 小时窗口，起点/终点都整点对齐）
    let day = chrono::Utc::now() - chrono::Duration::days(3);
    let start = day.date_naive().and_hms_opt(4, 0, 0).unwrap();
    // 用 Z 后缀：URL query 里的 `+` 会被解码成空格，导致 RFC3339 解析失败、
    // parse_range 静默回退到默认区间（非整点），测试就再也命中不到整点分支。
    let from = format!("{}T04:00:00Z", start.format("%Y-%m-%d"));
    let to = format!("{}T06:00:00Z", start.format("%Y-%m-%d"));

    for i in 0..3i64 {
        state
            .db
            .upsert_session(&serde_json::json!({
                "source_session_id": format!("ov-s-{i}"), "node_id": "n1",
                "started_at": format!("{}T04:1{}:00+00:00", start.format("%Y-%m-%d"), i),
                "timestamp": format!("{}T04:1{}:00+00:00", start.format("%Y-%m-%d"), i),
                "status": "ended", "client_id": "codex",
                "collector_id": "c1", "source_id": "s1",
            }))
            .unwrap();
        state
            .db
            .insert_call(
                &serde_json::json!({
                    "id": format!("ov-c-{i}"), "source_call_id": format!("ov-c-{i}"),
                    "node_id": "n1", "collector_id": "c1", "client_id": "codex",
                    "source_id": "s1", "session_id": format!("ov-s-{i}"),
                    "model_normalized": "gpt-5.6-sol", "provider_normalized": "openai",
                    "started_at": format!("{}T04:2{}:00+00:00", start.format("%Y-%m-%d"), i),
                    "status": "success", "call_granularity": "call",
                    "input_tokens": 100, "output_tokens": 50,
                    "project_id": "demo",
                }),
                &format!("n1:ov-s-{i}"),
            )
            .unwrap();
    }
    state.db.rebuild_rollups(90).unwrap();

    let get_overview = |token: &str, from: &str, to: &str| -> Value {
        let resp = ureq::get(&format!("{base}/api/v1/overview?from={from}&to={to}"))
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .unwrap();
        resp.into_json::<Value>().unwrap()
    };

    let before = get_overview(&token, &from, &to);
    assert_eq!(
        before["sessions"],
        json!(3),
        "归档前应统计到 3 个根会话: {before}"
    );
    assert_eq!(before["model_calls"], json!(3), "请求数应为 3");
    assert!(
        before["activity_archived_before"].is_null(),
        "归档前不应带归档标记: {}",
        before["activity_archived_before"]
    );
    assert_eq!(
        before["message_count"],
        json!(0),
        "无消息明细时是真实 0 而非 null"
    );

    // 启用归档（保留 1 天 → 3 天前的数据会被归档），同步执行
    metria_hub::archive::set_policy(&state.db, true, 1).unwrap();
    let report = metria_hub::archive::run(&state.db).unwrap();
    assert!(!report.skipped, "应确实执行了归档");
    assert!(report.deleted > 0, "应删除了明细行");

    let after = get_overview(&token, &from, &to);
    // 聚合支撑的四项：归档前后完全一致
    assert_eq!(after["sessions"], json!(3), "会话数必须保持不变: {after}");
    assert_eq!(after["model_calls"], json!(3), "请求数必须保持不变");
    assert_eq!(
        after["output_tokens"], before["output_tokens"],
        "Token 必须保持不变"
    );
    assert_eq!(
        after["calculated_cost_micro_usd"], before["calculated_cost_micro_usd"],
        "费用必须保持不变"
    );
    // 活动类指标：诚实返回 null + 标记，绝不返回偏小值
    assert!(
        after["activity_archived_before"].as_str().is_some(),
        "归档区间必须携带 activity_archived_before: {}",
        after["activity_archived_before"]
    );
    for key in [
        "message_count",
        "user_message_count",
        "tool_call_count",
        "active_sessions",
        "session_duration_ms",
    ] {
        assert!(
            after[key].is_null(),
            "{key} 在归档区间必须为 null（诚实标注），实际 {:?}",
            after[key]
        );
    }
}
