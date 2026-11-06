//! B9：会话明细分页、旧响应兼容与 /stats 统计。

use std::time::Instant;

use chrono::{Duration, Utc};
use metria_hub::api::AppState;
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_storage::rusqlite::params;
use serde_json::{json, Value};
use tokio::net::TcpListener;

const BIG_SESSION: &str = "sess-big";
const BIG_CALLS: i64 = 28_000;
const BIG_TOOLS: i64 = 150;
const BIG_MESSAGES: i64 = 250;

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

async fn spawn_hub(dir: &std::path::Path) -> (String, HubDb) {
    let cfg = test_cfg(dir);
    let db = HubDb::open(&cfg).unwrap();
    db.apply_migrations().unwrap();
    seed(&db);
    let state = AppState {
        db: db.clone(),
        cfg,
        sse: metria_hub::api::SseHub::new(),
        sessions: Default::default(),
        oidc: Default::default(),
        collector_token: Some("testtok".into()),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, metria_hub::api::app_router(state)).await;
    });
    (format!("http://{addr}"), db)
}

fn seed(db: &HubDb) {
    let base = Utc::now() - Duration::hours(8);
    let now = Utc::now().to_rfc3339();
    let mut c = db.conn();
    let tx = c.transaction().unwrap();
    {
        let mut call = tx
            .prepare(
                "INSERT INTO model_calls
                 (id,node_id,collector_id,client_id,source_id,session_id,started_at,duration_ms,
                  status,call_granularity,input_tokens,output_tokens,reported_cost_micro_usd,
                  created_at,updated_at)
                 VALUES (?1,'n1','c1','codex','s1',?2,?3,?4,?5,'call',?6,?7,?8,?9,?9)",
            )
            .unwrap();
        for i in 0..BIG_CALLS {
            let started = (base + Duration::seconds(i / 10)).to_rfc3339();
            let status = if i % 100 == 0 { "error" } else { "success" };
            call.execute(params![
                format!("call-{i:05}"),
                BIG_SESSION,
                started,
                100 + i % 1_000,
                status,
                1_000 + i,
                200 + i % 50,
                i % 7,
                now,
            ])
            .unwrap();
        }
        for i in 0..3 {
            call.execute(params![
                format!("small-{i}"),
                "sess-small",
                (base + Duration::seconds(i)).to_rfc3339(),
                10,
                "success",
                1,
                1,
                1,
                now,
            ])
            .unwrap();
        }
    }
    {
        tx.execute(
            "INSERT INTO usage_events
             (event_id,schema_version,node_id,collector_id,source_id,client_id,adapter_id,adapter_version,
              model_call_id,timestamp,provider_normalized,model_normalized,input_tokens,output_tokens,
              reported_cost_micro_usd,calculated_cost_micro_usd,usage_source,usage_granularity)
             VALUES ('small-usage',1,'n1','c1','s1','codex','codex','0.1.0',
                     'small-0',?1,'openai','gpt-test',1,1,123,456,'reported','call')",
            [now.as_str()],
        )
        .unwrap();
    }
    {
        let mut tool = tx
            .prepare(
                "INSERT INTO tool_events
                 (id,session_id,name,tool_type,status,input_length,output_length,started_at,created_at)
                 VALUES (?1,?2,'read','file','success',10,20,?3,?4)",
            )
            .unwrap();
        for i in 0..BIG_TOOLS {
            tool.execute(params![
                format!("tool-{i:04}"),
                BIG_SESSION,
                (base + Duration::seconds(i / 5)).to_rfc3339(),
                now,
            ])
            .unwrap();
        }
    }
    {
        let mut message = tx
            .prepare(
                "INSERT INTO messages
                 (id,session_id,sequence,role,content_type,content,content_length,utf8_bytes,created_at,redacted)
                 VALUES (?1,?2,?3,'user','text','hello',5,5,?4,0)",
            )
            .unwrap();
        for i in 0..BIG_MESSAGES {
            message
                .execute(params![
                    format!("msg-{i:04}"),
                    BIG_SESSION,
                    i,
                    (base + Duration::seconds(i)).to_rfc3339(),
                ])
                .unwrap();
        }
    }
    tx.commit().unwrap();
}

fn admin_token(base: &str) -> String {
    let resp = ureq::post(&format!("{base}/api/v1/auth/login"))
        .send_json(json!({ "username": "admin", "password": "change-me-please" }))
        .unwrap();
    resp.into_json::<Value>().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

fn get_json(url: &str, token: &str) -> (Value, usize) {
    let body = ureq::get(url)
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_string()
        .unwrap();
    (serde_json::from_str(&body).unwrap(), body.len())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_calls_pagination_is_complete_and_legacy_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _db) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);

    let started = Instant::now();
    let (first, first_bytes) = get_json(
        &format!("{base}/api/v1/sessions/{BIG_SESSION}/calls?limit=500"),
        &token,
    );
    let first_cost = started.elapsed();
    println!("calls first page: {first_cost:?}, {first_bytes} bytes");
    assert_eq!(first["calls"].as_array().unwrap().len(), 500);
    assert_eq!(first["total"], BIG_CALLS);
    assert_eq!(first["has_more"], true);
    assert!(first_cost.as_millis() < 200, "第一页耗时 {first_cost:?}");
    assert!(
        first_bytes < 500 * 1024,
        "第一页 payload {first_bytes} bytes"
    );

    let mut ids = Vec::new();
    let mut cursor = first["next_cursor"].as_str().map(str::to_string);
    ids.extend(
        first["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string()),
    );
    let mut pages = 1;
    while let Some(next) = cursor {
        let (page, _) = get_json(
            &format!("{base}/api/v1/sessions/{BIG_SESSION}/calls?limit=500&cursor={next}"),
            &token,
        );
        assert_eq!(page["total"], BIG_CALLS);
        ids.extend(
            page["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["id"].as_str().unwrap().to_string()),
        );
        cursor = page["next_cursor"].as_str().map(str::to_string);
        pages += 1;
        assert!(pages < 100, "分页未终止");
        if page["has_more"] == false {
            break;
        }
    }
    assert_eq!(ids.len() as i64, BIG_CALLS);
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(unique.len() as i64, BIG_CALLS, "分页不得重复或遗漏");

    // 无效 cursor 必须 400。
    match ureq::get(&format!(
        "{base}/api/v1/sessions/{BIG_SESSION}/calls?limit=10&cursor=not-base64"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        other => panic!("无效 cursor 期望 400，得到 {other:?}"),
    }

    // 不传分页参数：保持旧响应结构，只含 calls。
    let legacy: Value = ureq::get(&format!("{base}/api/v1/sessions/sess-small/calls"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(legacy["calls"].as_array().unwrap().len(), 3);
    let small = legacy["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "small-0")
        .unwrap();
    assert_eq!(small["reported_cost_micro_usd"], 123);
    assert_eq!(small["calculated_cost_micro_usd"], 456);
    let small1 = legacy["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "small-1")
        .unwrap();
    assert_eq!(
        small1["reported_cost_micro_usd"], 1,
        "没有 usage 时必须保留 model_calls 自身费用"
    );
    assert!(small1["calculated_cost_micro_usd"].is_null());
    assert!(small1["estimated_cost_micro_usd"].is_null());
    assert!(legacy.get("next_cursor").is_none());
    assert!(legacy.get("total").is_none());
    assert!(legacy.get("has_more").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeline_and_tools_pagination_are_stable() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _db) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);

    let started = Instant::now();
    let (first, first_bytes) = get_json(
        &format!("{base}/api/v1/sessions/{BIG_SESSION}/timeline?limit=100"),
        &token,
    );
    let first_cost = started.elapsed();
    println!("timeline first page: {first_cost:?}, {first_bytes} bytes");
    assert_eq!(first["messages"].as_array().unwrap().len(), 100);
    assert_eq!(first["total"], BIG_MESSAGES);
    assert!(
        first_cost.as_millis() < 200,
        "timeline 第一页 {first_cost:?}"
    );
    assert!(
        first_bytes < 1024 * 1024,
        "timeline payload {first_bytes} bytes"
    );

    let mut message_ids = Vec::new();
    let mut cursor = first["next_cursor"].as_str().map(str::to_string);
    message_ids.extend(
        first["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string()),
    );
    while let Some(next) = cursor {
        let (page, _) = get_json(
            &format!("{base}/api/v1/sessions/{BIG_SESSION}/timeline?limit=100&cursor={next}"),
            &token,
        );
        message_ids.extend(
            page["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["id"].as_str().unwrap().to_string()),
        );
        cursor = page["next_cursor"].as_str().map(str::to_string);
        if page["has_more"] == false {
            break;
        }
    }
    assert_eq!(message_ids.len() as i64, BIG_MESSAGES);
    assert_eq!(
        message_ids,
        (0..BIG_MESSAGES)
            .map(|i| format!("msg-{i:04}"))
            .collect::<Vec<_>>()
    );

    let mut tool_ids = Vec::new();
    let (first_tools, _) = get_json(
        &format!("{base}/api/v1/sessions/{BIG_SESSION}/tools?limit=100"),
        &token,
    );
    tool_ids.extend(
        first_tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_string()),
    );
    let mut cursor = first_tools["next_cursor"].as_str().map(str::to_string);
    while let Some(next) = cursor {
        let (page, _) = get_json(
            &format!("{base}/api/v1/sessions/{BIG_SESSION}/tools?limit=100&cursor={next}"),
            &token,
        );
        tool_ids.extend(
            page["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["id"].as_str().unwrap().to_string()),
        );
        cursor = page["next_cursor"].as_str().map(str::to_string);
        if page["has_more"] == false {
            break;
        }
    }
    assert_eq!(tool_ids.len() as i64, BIG_TOOLS);
    assert_eq!(
        tool_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        BIG_TOOLS as usize
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_stats_matches_full_aggregate() {
    let dir = tempfile::tempdir().unwrap();
    let (base, db) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);

    let (stats, _) = get_json(
        &format!("{base}/api/v1/sessions/{BIG_SESSION}/stats"),
        &token,
    );
    assert_eq!(stats["call_count"], BIG_CALLS);
    assert_eq!(stats["error_count"], BIG_CALLS / 100);
    let status_total: i64 = stats["status_counts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["count"].as_i64().unwrap())
        .sum();
    assert_eq!(status_total, BIG_CALLS);
    let series_calls: i64 = stats["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["call_count"].as_i64().unwrap())
        .sum();
    assert_eq!(series_calls, BIG_CALLS);

    let (input, output, duration): (i64, i64, i64) = db
        .read_conn()
        .query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(duration_ms),0)
             FROM model_calls WHERE session_id = ?1",
            [BIG_SESSION],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let series_input: i64 = stats["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["input_tokens"].as_i64().unwrap())
        .sum();
    let series_output: i64 = stats["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["output_tokens"].as_i64().unwrap())
        .sum();
    let series_duration: i64 = stats["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["duration_ms"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(series_input, input);
    assert_eq!(series_output, output);
    assert_eq!(series_duration, duration);

    let (daily, _) = get_json(
        &format!("{base}/api/v1/sessions/{BIG_SESSION}/stats?granularity=day"),
        &token,
    );
    assert_eq!(daily["granularity"], "day");
    let daily_calls: i64 = daily["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["call_count"].as_i64().unwrap())
        .sum();
    assert_eq!(daily_calls, BIG_CALLS);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeline_pagination_handles_duplicate_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let (base, db) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);

    {
        let c = db.conn();
        for (id, sequence) in [
            ("dup-0", 1),
            ("dup-1", 1),
            ("dup-2", 2),
            ("dup-3", 2),
            ("dup-4", 3),
            ("dup-5", 3),
        ] {
            c.execute(
                "INSERT INTO messages
                 (id,session_id,sequence,role,content_type,content,content_length,utf8_bytes,created_at,redacted)
                 VALUES (?1,'sess-dup',?2,'user','text','x',1,1,'2026-08-10T00:00:00Z',0)",
                params![id, sequence],
            )
            .unwrap();
        }
    }

    let mut ids = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let url = match &cursor {
            Some(next) => format!("{base}/api/v1/sessions/sess-dup/timeline?limit=2&cursor={next}"),
            None => format!("{base}/api/v1/sessions/sess-dup/timeline?limit=2"),
        };
        let (page, _) = get_json(&url, &token);
        ids.extend(
            page["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["id"].as_str().unwrap().to_string()),
        );
        cursor = page["next_cursor"].as_str().map(str::to_string);
        if page["has_more"] == false {
            break;
        }
        assert!(ids.len() <= 6, "分页不得循环");
    }

    assert_eq!(
        ids,
        vec!["dup-0", "dup-1", "dup-2", "dup-3", "dup-4", "dup-5"],
        "重复 sequence 必须按 (sequence, id) 稳定分页"
    );
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), 6, "分页不得重复或遗漏");

    match ureq::get(&format!(
        "{base}/api/v1/sessions/sess-dup/timeline?limit=2&cursor=not-base64"
    ))
    .set("Authorization", &format!("Bearer {token}"))
    .call()
    {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        other => panic!("无效 cursor 期望 400，得到 {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_stats_keeps_invalid_bucket_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (base, db) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);

    db.conn()
        .execute(
            "INSERT INTO model_calls
             (id,node_id,collector_id,client_id,source_id,session_id,started_at,status,
              call_granularity,created_at,updated_at)
             VALUES ('bad-time','n1','c1','codex','s1','sess-bad','not-a-time','success',
                     'call','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

    let (stats, _) = get_json(&format!("{base}/api/v1/sessions/sess-bad/stats"), &token);
    assert_eq!(stats["call_count"], 1);
    let series = stats["series"].as_array().unwrap();
    let series_calls: i64 = series
        .iter()
        .map(|s| s["call_count"].as_i64().unwrap())
        .sum();
    assert_eq!(series_calls, 1, "无法解析 started_at 的行不得被静默丢弃");
    assert!(
        series.iter().any(|s| s["bucket"] == "unavailable"),
        "无效时间应归入 unavailable 桶"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_stats_exposes_average_duration() {
    let dir = tempfile::tempdir().unwrap();
    let (base, db) = spawn_hub(dir.path()).await;
    let token = admin_token(&base);

    {
        let c = db.conn();
        for (id, started, duration) in [
            ("avg-1", "2026-08-10T10:05:00Z", Some(100)),
            ("avg-2", "2026-08-10T10:15:00Z", Some(300)),
            ("avg-3", "2026-08-10T11:05:00Z", None),
        ] {
            c.execute(
                "INSERT INTO model_calls
                 (id,node_id,collector_id,client_id,source_id,session_id,started_at,duration_ms,
                  status,call_granularity,created_at,updated_at)
                 VALUES (?1,'n1','c1','codex','s1','sess-avg',?2,?3,'success','call',
                         '2026-08-10T00:00:00Z','2026-08-10T00:00:00Z')",
                params![id, started, duration],
            )
            .unwrap();
        }
    }

    let (stats, _) = get_json(&format!("{base}/api/v1/sessions/sess-avg/stats"), &token);
    let series = stats["series"].as_array().unwrap();
    assert_eq!(series.len(), 2);
    let first = &series[0];
    assert_eq!(first["duration_ms"], 400, "duration_ms 保留为总和");
    assert_eq!(first["avg_duration_ms"], 200.0, "延迟应使用平均值");
    assert_eq!(first["duration_samples"], 2);

    let detail_avg: Option<f64> = db
        .read_conn()
        .query_row(
            "SELECT AVG(duration_ms) FROM model_calls
             WHERE session_id = 'sess-avg' AND started_at < '2026-08-10T11:00:00Z'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(first["avg_duration_ms"].as_f64(), detail_avg);

    let no_samples = &series[1];
    assert_eq!(no_samples["duration_samples"], 0);
    assert!(no_samples["duration_ms"].is_null());
    assert!(
        no_samples["avg_duration_ms"].is_null(),
        "无 duration 样本时平均值必须为 null，不能填 0"
    );
}
