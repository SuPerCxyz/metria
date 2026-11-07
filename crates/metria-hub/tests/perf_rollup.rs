//! 小时级性能预聚合的端到端行为（spec `hub-query-performance`）。
//!
//! 覆盖：预聚合与明细结果一致、缺桶整体回退而不是填 0、当前小时新数据立即可见、
//! 维度筛选回退明细，以及刷新路径的 CPU 成本上界。

use std::time::Instant;

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_hub::perfrollup;
use metria_storage::rusqlite::types::Value as SqlValue;
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

fn test_db(tag: &str) -> (std::path::PathBuf, HubDb) {
    let dir = std::env::temp_dir().join(format!("metria-perfrollup-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = HubDb::open(&test_cfg(&dir)).unwrap();
    db.apply_migrations().unwrap();
    // 生产启动会初始化只读连接池；测试也按同样形态运行，验证快路径不抢写连接。
    db.init_read_pool().unwrap();
    (dir, db)
}

/// 当前小时的整点。
fn current_hour() -> DateTime<Utc> {
    let now = Utc::now();
    Utc.with_ymd_and_hms(now.year(), now.month(), now.day(), now.hour(), 0, 0)
        .single()
        .unwrap()
}

fn fast_agg(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    filter: &str,
) -> Option<perfrollup::PerfHour> {
    perfrollup::fast_aggregate(db, from, to, filter).unwrap()
}

fn fast_hrs(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    filter: &str,
) -> Option<Vec<(DateTime<Utc>, perfrollup::PerfHour)>> {
    perfrollup::fast_hours(db, from, to, filter).unwrap()
}

/// 插入一批调用：`hour_offset` 为相对当前整点的小时偏移（负数表示过去）。
///
/// 返回 `(调用数, duration 总和)`，供与预聚合结果对照。
fn seed_calls(db: &HubDb, hour_offset: i64, count: i64) -> (i64, i64) {
    let base = current_hour() + Duration::hours(hour_offset);
    let mut inserted = 0i64;
    let mut duration_sum = 0i64;
    for i in 0..count {
        let started = base + Duration::seconds(i * 7 % 3600);
        let duration = 100 + (i * 37) % 5_000;
        let call = json!({
            "id": format!("call-{hour_offset}-{i}"),
            "source_call_id": format!("src-{hour_offset}-{i}"),
            "node_id": "n1", "collector_id": "c1", "client_id": "codex",
            "source_id": "s1", "session_id": format!("sess-{hour_offset}"),
            "model_raw": "gpt-5.6-sol", "model_normalized": "gpt-5.6-sol",
            "provider_normalized": "openai",
            "started_at": started.to_rfc3339(),
            "first_response_at": (started + Duration::milliseconds(duration)).to_rfc3339(),
            "completed_at": (started + Duration::milliseconds(duration)).to_rfc3339(),
            "duration_ms": duration,
            "status": if i % 5 == 0 { "error" } else { "success" },
            "status_code": if i % 5 == 0 { 500 } else { 200 },
            "call_granularity": "call",
            "input_tokens": 1_000, "output_tokens": 200,
            // 观测字段：生成耗时与输出速度走运行时观测，first_byte/首 Token 走时间戳
            "first_byte_at": (started + Duration::milliseconds(duration / 4)).to_rfc3339(),
            "first_token_at": (started + Duration::milliseconds(duration / 3)).to_rfc3339(),
            "generation_duration_ms": duration - 50,
            "output_tokens_per_second_milli": 1_500 + i * 3,
            "inter_token_latency_avg_ms": 20 + i % 40,
            "inter_token_latency_p95_ms": 60 + i % 90,
            "stall_count": i % 3,
            "observability_source": "runtime",
            "observability_quality": "observed",
            "observed_request_payload_bytes": 512 + i,
            "observed_response_payload_bytes": 2_048 + i,
            "observed_request_wire_bytes": 640 + i,
            "observed_response_wire_bytes": 2_560 + i,
            "rate_limited": i % 11 == 0,
        });
        if db
            .insert_call(&call, &format!("n1:sess-{hour_offset}"))
            .unwrap()
        {
            inserted += 1;
            duration_sum += duration;
        }
        // 每条调用带一条用量行，验证热力图 Token 口径
        let usage = json!({
            "event_id": format!("ue-{hour_offset}-{i}"), "schema_version": 1,
            "node_id": "n1", "collector_id": "c1", "source_id": "s1", "client_id": "codex",
            "adapter_id": "codex", "adapter_version": "0.1.0",
            "model_call_id": format!("call-{hour_offset}-{i}"),
            "timestamp": started.to_rfc3339(),
            "provider_normalized": "openai", "model_normalized": "gpt-5.6-sol",
            "usage": {"input": 1_000, "output": 200, "cache_read": 100, "cache_write": 50, "reasoning": 10},
            "cost": {"calculated_micro_usd": 777},
            "quality": {"usage_source": "reported", "granularity": "call", "confidence": 1.0},
        });
        db.insert_usage(&usage, &format!("n1:sess-{hour_offset}"))
            .unwrap();
    }
    (inserted, duration_sum)
}

/// 插入单条额外调用（ID 唯一），用于模拟预聚合构建后的补采。
fn seed_extra_call(db: &HubDb, hour_offset: i64, id: &str) {
    let started = current_hour() + Duration::hours(hour_offset) + Duration::minutes(45);
    let call = json!({
        "id": id,
        "source_call_id": id,
        "node_id": "n1", "collector_id": "c1", "client_id": "codex",
        "source_id": "s1", "session_id": format!("sess-{hour_offset}"),
        "model_raw": "gpt-5.6-sol", "model_normalized": "gpt-5.6-sol",
        "provider_normalized": "openai",
        "started_at": started.to_rfc3339(),
        "completed_at": (started + Duration::milliseconds(333)).to_rfc3339(),
        "duration_ms": 333,
        "status": "success",
        "call_granularity": "call",
        "input_tokens": 1_000, "output_tokens": 200,
    });
    assert!(db
        .insert_call(&call, &format!("n1:sess-{hour_offset}"))
        .unwrap());
}

fn heatmap_totals(list: &[(DateTime<Utc>, perfrollup::PerfHour)]) -> (i64, u64, i64) {
    list.iter()
        .fold((0, 0, 0), |(tokens, calls, cost), (_, hour)| {
            let h = &hour.heatmap;
            (
                tokens
                    + h.input_tokens
                    + h.output_tokens
                    + h.cache_read_tokens
                    + h.cache_write_tokens
                    + h.reasoning_tokens,
                calls + h.model_calls,
                cost + h.reported_cost + h.calculated_cost + h.estimated_cost,
            )
        })
}

#[test]
fn fast_path_matches_detail_scan_including_quiet_hours() {
    let (_dir, db) = test_db("equivalent");
    // -4 与 -2 小时有数据，-3 小时故意留空：空小时必须有预聚合行，否则覆盖检查会失败
    seed_calls(&db, -4, 40);
    seed_calls(&db, -2, 60);
    let built = perfrollup::rebuild_history(&db).unwrap();
    assert!(built >= 4, "应为区间内每个完整小时写行，含空小时: {built}");

    let from = current_hour() - Duration::hours(6);
    let to = Utc::now();

    let fast = fast_agg(&db, from, to, "").expect("覆盖完整时应命中快速路径");
    let raw = perfrollup::scan_calls(&db, from, to, "", &[]).unwrap();

    assert_eq!(fast.call_count, raw.call_count, "调用总数必须一致");
    assert_eq!(fast.success, raw.success, "成功数必须一致");
    assert_eq!(fast.errors, raw.errors, "失败数必须一致");
    assert_eq!(
        fast.duration.count, raw.duration.count,
        "时长样本数必须一致"
    );
    assert_eq!(fast.duration.sum, raw.duration.sum, "时长总和必须精确一致");
    assert_eq!(fast.ttft.count, raw.ttft.count, "TTFT 样本数必须一致");
    assert_eq!(fast.ttft.sum, raw.ttft.sum, "TTFT 总和必须一致");
    assert_eq!(fast.generation.count, raw.generation.count);
    assert_eq!(fast.inter_token.count, raw.inter_token.count);
    assert_eq!(fast.observed_bytes, raw.observed_bytes, "观测字节必须一致");

    // 分位数：两条路径共用同一套直方图，合并结果必须完全相同
    for q in [0.50, 0.95, 0.99] {
        assert_eq!(
            fast.duration.quantile(q),
            raw.duration.quantile(q),
            "duration p{q} 应与明细一致"
        );
        assert_eq!(fast.ttft.quantile(q), raw.ttft.quantile(q));
    }
    assert!(
        fast.duration.quantile(0.95).is_some(),
        "有样本时分位数不能是 null"
    );

    // /overview 直接依赖的去重集合与计数：必须与 SQL 的 COUNT(DISTINCT) 完全等价
    let range_count = |sql: &str| -> i64 {
        db.conn()
            .query_row(sql, [from.to_rfc3339(), to.to_rfc3339()], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert_eq!(
        fast.distinct_clients.len() as i64,
        range_count("SELECT COUNT(DISTINCT client_id) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2"),
        "agent_tools 计数必须与 COUNT(DISTINCT client_id) 一致"
    );
    assert_eq!(
        fast.distinct_models.len() as i64,
        range_count("SELECT COUNT(DISTINCT model_normalized) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 AND model_normalized IS NOT NULL AND model_normalized != ''"),
        "models 计数必须与 COUNT(DISTINCT model_normalized) 一致"
    );
    assert_eq!(
        fast.distinct_projects.len() as i64,
        range_count("SELECT COUNT(DISTINCT project_id) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 AND project_id IS NOT NULL AND project_id != ''"),
        "projects 计数必须与 COUNT(DISTINCT project_id) 一致"
    );
    assert_eq!(
        fast.token_call_count as i64,
        range_count("SELECT COUNT(*) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 AND (input_tokens IS NOT NULL OR output_tokens IS NOT NULL OR cache_read_tokens IS NOT NULL OR cache_write_tokens IS NOT NULL OR reasoning_tokens IS NOT NULL)"),
        "token_calls 必须与带 Token 的调用数一致"
    );
    assert_eq!(
        fast.errors as i64,
        range_count("SELECT COUNT(*) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 AND status != 'success' AND status != 'ok' AND status != 'completed'"),
        "failed_calls 必须与 SQL 口径一致"
    );

    // 明细回退路径（不经过 merge）必须给出同样的去重结果：
    // 这正是 e2e 曾漏掉的盲区——去重只做在 merge 里，scan_calls 直接累加会重复计数。
    assert_eq!(
        raw.distinct_clients.len() as i64,
        range_count("SELECT COUNT(DISTINCT client_id) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2"),
        "明细路径的 agent_tools 计数同样必须去重"
    );
    assert_eq!(
        raw.distinct_models.len() as i64,
        range_count("SELECT COUNT(DISTINCT model_normalized) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 AND model_normalized IS NOT NULL AND model_normalized != ''"),
        "明细路径的 models 计数同样必须去重"
    );
    assert_eq!(
        raw.distinct_projects.len() as i64,
        range_count("SELECT COUNT(DISTINCT project_id) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2 AND project_id IS NOT NULL AND project_id != ''"),
        "明细路径的 projects 计数同样必须去重"
    );
    assert_eq!(
        raw.token_pair_call_count as i64, fast.token_pair_call_count as i64,
        "计价分母两条路径必须一致"
    );
    assert_eq!(
        raw.priced_call_count as i64, fast.priced_call_count as i64,
        "已计价条数两条路径必须一致"
    );
    assert_eq!(
        raw.unpriced_by_model.len(),
        fast.unpriced_by_model.len(),
        "未计价分组条目数必须一致"
    );

    // 热力图口径
    let hours = fast_hrs(&db, from, to, "").unwrap();
    let fast_tokens = heatmap_totals(&hours);
    let detail: Vec<(DateTime<Utc>, perfrollup::PerfHour)> = vec![(from, {
        let mut merged = perfrollup::PerfHour::new();
        merged.merge(&perfrollup::scan_calls(&db, from, to, "", &[]).unwrap());
        merged
    })];
    let raw_tokens = heatmap_totals(&detail);
    assert_eq!(fast_tokens.1, raw_tokens.1, "热力图调用数必须一致");
    assert_eq!(fast_tokens.0, raw_tokens.0, "热力图 Token 必须一致");
    assert_eq!(fast_tokens.2, raw_tokens.2, "热力图费用必须一致");
    // 逐字段对照：除浮点累加顺序导致的末位差异外，所有指标、来源分布与热力图字段必须一致。
    let mut fast_cmp = fast.clone();
    let mut raw_cmp = raw.clone();
    fast_cmp.speed.sum = 0.0;
    raw_cmp.speed.sum = 0.0;
    assert_eq!(fast_cmp, raw_cmp, "快慢路径逐字段必须一致");
    assert!(
        (fast.speed.sum - raw.speed.sum).abs() < 1e-9,
        "输出速度总和只允许浮点末位差异: {} vs {}",
        fast.speed.sum,
        raw.speed.sum
    );
}

#[test]
fn recent_rebuild_matches_full_for_recent_hours() {
    let (_dir, db) = test_db("recent-rebuild");
    for offset in -5..0 {
        seed_calls(&db, offset, 20);
    }
    perfrollup::rebuild_history(&db).unwrap();

    // 删掉最近 3 个已结束小时，模拟维护任务尚未补齐。
    for offset in [-3, -2, -1] {
        let bucket = perfrollup::hour_bucket(current_hour() + Duration::hours(offset));
        db.conn()
            .execute(
                "DELETE FROM hourly_performance_rollups WHERE bucket = ?1",
                [&bucket],
            )
            .unwrap();
    }
    let rebuilt = perfrollup::rebuild_recent(&db, 3).unwrap();
    assert!(rebuilt >= 3, "最近 3 小时应重新写入: {rebuilt}");

    for offset in [-3, -2, -1] {
        let hour = current_hour() + Duration::hours(offset);
        let start = hour.to_rfc3339();
        let end = (hour + Duration::hours(1)).to_rfc3339();
        let fast = fast_agg(&db, hour, hour + Duration::hours(1), "").unwrap();
        let raw = perfrollup::scan_calls(&db, hour, hour + Duration::hours(1), "", &[]).unwrap();
        assert_eq!(
            fast.call_count, raw.call_count,
            "{start}..{end} 调用数不一致"
        );
        assert_eq!(
            fast.duration.sum, raw.duration.sum,
            "{start}..{end} 时长不一致"
        );
    }
}

#[test]
fn missing_bucket_uses_detail_for_only_that_hour() {
    let (_dir, db) = test_db("missing");
    // 覆盖 -4..-1 四个完整小时，然后删掉**中间**的 -3 桶
    seed_calls(&db, -4, 30);
    seed_calls(&db, -2, 30);
    perfrollup::rebuild_history(&db).unwrap();

    let from = current_hour() - Duration::hours(6);
    let to = Utc::now();
    assert!(
        fast_agg(&db, from, to, "").is_some(),
        "覆盖完整时应命中快速路径"
    );

    // 中间桶缺失：只补该小时，绝不整段回退，也不把缺失当成 0。
    let victim = perfrollup::hour_bucket(current_hour() - Duration::hours(3));
    db.conn()
        .execute(
            "DELETE FROM hourly_performance_rollups WHERE bucket = ?1",
            [&victim],
        )
        .unwrap();
    let fast = fast_agg(&db, from, to, "").expect("单桶缺失不应导致整段回退");
    let raw = perfrollup::scan_calls(&db, from, to, "", &[]).unwrap();
    assert_eq!(fast.call_count, raw.call_count);
    assert_eq!(fast.duration.sum, raw.duration.sum);
    let hours = fast_hrs(&db, from, to, "").unwrap();
    assert!(
        hours
            .iter()
            .any(|(bucket, _)| perfrollup::hour_bucket(*bucket) == victim),
        "缺失小时仍应出现在返回桶序列中"
    );
}

#[test]
fn early_built_bucket_is_repaired_from_detail() {
    let (_dir, db) = test_db("early-built");
    seed_calls(&db, -2, 30);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(2);
    let bucket = perfrollup::hour_bucket(hour);
    // 模拟该桶在小时结束前就构建：后续补采的行不在 payload 里。
    db.conn()
        .execute(
            "UPDATE hourly_performance_rollups SET built_at = ?1 WHERE bucket = ?2",
            [(hour + Duration::minutes(5)).to_rfc3339(), bucket.clone()],
        )
        .unwrap();
    seed_extra_call(&db, -2, "call-early-extra");

    let from = hour;
    let to = hour + Duration::hours(1);
    let fast = fast_agg(&db, from, to, "").unwrap();
    let raw = perfrollup::scan_calls(&db, from, to, "", &[]).unwrap();
    assert_eq!(
        fast.call_count, raw.call_count,
        "早建桶必须用该小时明细补齐"
    );
    assert_eq!(fast.duration.sum, raw.duration.sum);

    let hours = fast_hrs(&db, from, to, "").unwrap();
    assert_eq!(hours.len(), 1);
    assert_eq!(hours[0].1.call_count, raw.call_count);
}

#[test]
fn payload_version_mismatch_uses_detail_for_that_hour() {
    let (_dir, db) = test_db("stale-payload");
    seed_calls(&db, -3, 20);
    perfrollup::rebuild_history(&db).unwrap();
    let bucket = perfrollup::hour_bucket(current_hour() - Duration::hours(3));
    db.conn()
        .execute(
            "UPDATE hourly_performance_rollups SET payload = json_set(payload, '$.v', 0) WHERE bucket = ?1",
            [&bucket],
        )
        .unwrap();
    let from = current_hour() - Duration::hours(4);
    let to = Utc::now();
    let fast = fast_agg(&db, from, to, "").expect("版本过期的单桶应回退该小时明细，而不是整段回退");
    let raw = perfrollup::scan_calls(&db, from, to, "", &[]).unwrap();
    assert_eq!(fast.call_count, raw.call_count);
    assert_eq!(fast.duration.sum, raw.duration.sum);
    assert!(fast_hrs(&db, from, to, "").is_some());
}

#[test]
fn corrupt_payload_repairs_from_detail_when_not_archived() {
    let (_dir, db) = test_db("corrupt-payload");
    seed_calls(&db, -3, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let bucket = perfrollup::hour_bucket(current_hour() - Duration::hours(3));
    db.conn()
        .execute(
            "UPDATE hourly_performance_rollups SET payload = '{ broken' WHERE bucket = ?1",
            [&bucket],
        )
        .unwrap();
    let from = current_hour() - Duration::hours(4);
    let fast = fast_agg(&db, from, Utc::now(), "").expect("水位后损坏应按明细补齐");
    let raw = perfrollup::scan_calls(&db, from, Utc::now(), "", &[]).unwrap();
    assert_eq!(fast.call_count, raw.call_count);
    assert!(fast_hrs(&db, from, Utc::now(), "").is_some());
}

#[test]
fn archived_corrupt_payload_returns_error_instead_of_zero() {
    let (_dir, db) = test_db("archived-corrupt-query");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    let bucket = perfrollup::hour_bucket(hour);
    db.conn()
        .execute(
            "UPDATE hourly_performance_rollups SET payload = '{ broken' WHERE bucket = ?1",
            [&bucket],
        )
        .unwrap();
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(current_hour() - Duration::hours(3)).to_rfc3339(),
    )
    .unwrap();

    let result = perfrollup::fast_aggregate(&db, hour, hour + Duration::hours(1), "");
    assert!(
        result.is_err(),
        "归档水位前损坏必须返回错误，不能回退到已归档明细得到 0"
    );
    assert!(
        perfrollup::fast_hours(&db, hour, hour + Duration::hours(1), "").is_err(),
        "fast_hours 同样必须返回错误"
    );
}

#[test]
fn current_hour_data_is_visible_without_rebuild() {
    let (_dir, db) = test_db("freshness");
    seed_calls(&db, -3, 25);
    perfrollup::rebuild_history(&db).unwrap();
    let from = current_hour() - Duration::hours(5);
    let before = fast_agg(&db, from, Utc::now(), "")
        .expect("应命中快速路径")
        .call_count;

    // 预聚合之后、当前小时内新到的调用：必须由尾段明细回退立即反映
    seed_calls(&db, 0, 5);
    let after = fast_agg(&db, from, Utc::now(), "")
        .expect("应仍命中快速路径")
        .call_count;
    assert_eq!(
        after,
        before + 5,
        "当前小时的新数据必须立即可见，不能读到过期预聚合"
    );
}

#[test]
fn dimension_filter_falls_back_to_detail() {
    let (_dir, db) = test_db("filtered");
    seed_calls(&db, -3, 20);
    perfrollup::rebuild_history(&db).unwrap();
    let from = current_hour() - Duration::hours(5);
    // 预聚合不带维度，任何维度筛选都必须回退明细以保证正确性
    assert!(
        fast_agg(&db, from, Utc::now(), "AND node_id = ?3").is_none(),
        "带维度筛选时必须回退明细"
    );
    let agg = perfrollup::scan_calls(
        &db,
        from,
        Utc::now(),
        "AND node_id = ?3",
        &[SqlValue::Text("no-such-node".into())],
    )
    .unwrap();
    assert_eq!(agg.call_count, 0, "该节点没有调用，应为 0 而不是假数据");
}

#[test]
fn fast_path_stays_within_refresh_budget() {
    let (_dir, db) = test_db("budget");
    // 48 小时 × 250 条 ≈ 1.2 万行，量级接近真实 30 天窗口的刷新成本
    for offset in -48..0 {
        seed_calls(&db, offset, 250);
    }
    let built_at = Instant::now();
    perfrollup::rebuild_history(&db).unwrap();
    let build_cost = built_at.elapsed();

    let from = current_hour() - Duration::hours(48);
    let to = Utc::now();

    let fast_start = Instant::now();
    let fast = fast_agg(&db, from, to, "").expect("应命中快速路径");
    let fast_cost = fast_start.elapsed();

    let raw_start = Instant::now();
    let raw = perfrollup::scan_calls(&db, from, to, "", &[]).unwrap();
    let raw_cost = raw_start.elapsed();

    assert_eq!(fast.call_count, raw.call_count, "成本换口径是不可接受的");
    assert!(
        fast_cost.as_millis() <= 500,
        "一次刷新的快速路径必须 ≤500ms，实际 {fast_cost:?}（明细 {raw_cost:?}，构建 {build_cost:?}）"
    );
    println!(
        "快速路径 {:?} / 明细 {:?} / 构建 {:?} / 行数 {}",
        fast_cost, raw_cost, build_cost, raw.call_count
    );
}

#[test]
fn rebuild_with_unaligned_earliest_keeps_every_hour_complete() {
    // 回归：最早数据落在非整点（如 xx:37）时，窗口边界若不对齐，
    // 同一个小时会被两个窗口各写一次、后者覆盖前者，该小时只剩尾部样本。
    let (_dir, db) = test_db("unaligned");
    let now = Utc::now();
    let base = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), now.hour(), 0, 0)
        .unwrap()
        - Duration::hours(3)
        + Duration::minutes(37);
    let c = db.conn();
    for i in 0..120i64 {
        let started = base + Duration::seconds((i * 13) % 3600);
        c.execute(
            "INSERT INTO model_calls (id, node_id, collector_id, client_id, source_id, session_id,
                 started_at, status, call_granularity, duration_ms, created_at, updated_at)
             VALUES (?1, 'n1', 'c1', 'codex', 's1', 'sess-u', ?2, 'success', 'call', ?3, ?2, ?2)",
            [
                format!("unaligned-{i}"),
                started.to_rfc3339(),
                (100 + i).to_string(),
            ],
        )
        .unwrap();
    }
    drop(c);

    perfrollup::rebuild_history(&db).unwrap();
    let from = current_hour() - Duration::hours(6);
    let fast = fast_agg(&db, from, Utc::now(), "").expect("应命中快速路径");
    let raw = perfrollup::scan_calls(&db, from, Utc::now(), "", &[]).unwrap();
    assert_eq!(
        fast.call_count, raw.call_count,
        "非整点起步的重建不得丢失任何小时的样本"
    );
    assert_eq!(fast.duration.sum, raw.duration.sum);

    // 逐桶核对：每个有明细的小时，预聚合 call_count 必须与明细一致
    let hours = fast_hrs(&db, from, Utc::now(), "").unwrap();
    let missing: Vec<String> = {
        let conn = db.conn();
        let mut bad = Vec::new();
        for (bucket, agg) in &hours {
            let end = *bucket + Duration::hours(1);
            let real: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2",
                    [bucket.to_rfc3339(), end.to_rfc3339()],
                    |r| r.get(0),
                )
                .unwrap();
            if agg.call_count as i64 != real {
                bad.push(format!(
                    "{}: 预聚合 {} vs 明细 {}",
                    bucket.to_rfc3339(),
                    agg.call_count,
                    real
                ));
            }
        }
        bad
    };
    assert!(
        missing.is_empty(),
        "以下小时的预聚合与明细不一致（窗口覆盖被覆盖）: {missing:?}"
    );
}

#[test]
fn cross_hour_short_range_falls_back_instead_of_single_bucket() {
    // 回归：10:30→11:30 这种「没有完整小时但跨多个 UTC 小时」的区间，
    // 若把结果压进 floor(from) 一个桶，热力图/日趋势会把 11:00 之后的行
    // 算进 10:00 那一格，跨日界时更会记到前一天。
    let (_dir, db) = test_db("cross-hour");
    // 600 条按 i*7%3600 铺满整小时，保证跨小时区间里确实有数据
    seed_calls(&db, -3, 600);
    perfrollup::rebuild_history(&db).unwrap();

    let base = current_hour() - Duration::hours(3);
    let from = base + Duration::minutes(50);
    let to = base + Duration::hours(1) + Duration::minutes(10);
    let list = fast_hrs(&db, from, to, "").expect("跨小时短区间应由逐小时明细补齐，而不是整段回退");
    assert_eq!(list.len(), 2, "跨小时短区间应产出两个独立小时桶");
    assert_eq!(
        list.iter().map(|(_, hour)| hour.call_count).sum::<u64>(),
        perfrollup::scan_calls(&db, from, to, "", &[])
            .unwrap()
            .call_count,
        "逐小时补齐不得重复计数"
    );
    let fast = fast_agg(&db, from, to, "").unwrap();
    let raw = perfrollup::scan_calls(&db, from, to, "", &[]).unwrap();
    assert_eq!(fast.call_count, raw.call_count);

    // 单个小时内的区间仍可安全压成一个桶
    let single = base + Duration::minutes(10);
    let single_end = base + Duration::minutes(50);
    let list = fast_hrs(&db, single, single_end, "").expect("同一小时内的区间应命中快速路径");
    assert_eq!(list.len(), 1, "同一小时区间只应产出一个桶");

    // 回退路径的口径仍与明细一致（不因回退而变慢或变错）
    let agg = perfrollup::scan_calls(&db, from, to, "", &[]).unwrap();
    assert!(agg.call_count > 0, "该区间应有调用数据");
}

#[test]
fn late_call_invalidates_old_perf_rollup() {
    let (_dir, db) = test_db("late-invalidate");
    seed_calls(&db, -4, 20);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    let before = fast_agg(&db, hour, hour + Duration::hours(1), "")
        .unwrap()
        .call_count;

    seed_extra_call(&db, -4, "call-late-extra");
    let deleted = perfrollup::invalidate_hours(&db, &[hour]).unwrap();
    assert_eq!(deleted, 1, "应删除目标小时的预聚合行");

    let fast = fast_agg(&db, hour, hour + Duration::hours(1), "").unwrap();
    let raw = perfrollup::scan_calls(&db, hour, hour + Duration::hours(1), "", &[]).unwrap();
    assert_eq!(fast.call_count, raw.call_count, "失效后应按明细补齐");
    assert_eq!(fast.call_count, before + 1, "补采事件必须被计入且只计一次");
}

#[test]
fn payload_version_key_prevents_repeated_full_rebuild() {
    let (_dir, db) = test_db("version-key");
    seed_calls(&db, -3, 10);
    assert!(
        perfrollup::needs_full_rebuild(&db).unwrap(),
        "版本键缺失应触发全量"
    );

    perfrollup::rebuild_history(&db).unwrap();
    assert_eq!(
        db.setting_get(perfrollup::PAYLOAD_VERSION_KEY).unwrap(),
        Some(perfrollup::PAYLOAD_VERSION.to_string()),
        "全量重建后必须写版本键"
    );
    assert!(
        !perfrollup::needs_full_rebuild(&db).unwrap(),
        "第二次启动不应全量"
    );

    db.setting_set(perfrollup::PAYLOAD_VERSION_KEY, "0")
        .unwrap();
    assert!(
        perfrollup::needs_full_rebuild(&db).unwrap(),
        "版本不一致应全量"
    );

    db.setting_set(
        perfrollup::PAYLOAD_VERSION_KEY,
        &perfrollup::PAYLOAD_VERSION.to_string(),
    )
    .unwrap();
    let bucket = perfrollup::hour_bucket(current_hour() - Duration::hours(3));
    db.conn()
        .execute(
            "UPDATE hourly_performance_rollups SET payload = '{ broken' WHERE bucket = ?1",
            [&bucket],
        )
        .unwrap();
    assert!(
        !perfrollup::needs_full_rebuild(&db).unwrap(),
        "损坏行不得导致无限全量重建"
    );

    db.conn()
        .execute("DELETE FROM hourly_performance_rollups", [])
        .unwrap();
    assert!(perfrollup::needs_full_rebuild(&db).unwrap(), "表为空应全量");
}

#[test]
fn archived_corrupt_rollup_is_kept_without_full_rebuild() {
    let (_dir, db) = test_db("archived-corrupt");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let old_bucket = perfrollup::hour_bucket(current_hour() - Duration::hours(4));
    db.conn()
        .execute(
            "UPDATE hourly_performance_rollups SET payload = '{ broken' WHERE bucket = ?1",
            [&old_bucket],
        )
        .unwrap();
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(current_hour() - Duration::hours(3)).to_rfc3339(),
    )
    .unwrap();

    assert!(
        !perfrollup::needs_full_rebuild(&db).unwrap(),
        "归档水位前的坏行不得触发全量重建"
    );
    perfrollup::rebuild_history(&db).unwrap();

    let kept: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM hourly_performance_rollups WHERE bucket = ?1",
            [&old_bucket],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1, "归档水位前的旧行必须保留，不能删除后重建为空");
}

fn dirty_hours(db: &HubDb) -> Vec<String> {
    let c = db.read_conn();
    let mut stmt = c
        .prepare("SELECT hour FROM perf_rollup_dirty_hours ORDER BY hour")
        .unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.filter_map(Result::ok).collect()
}

fn perf_rows_for(db: &HubDb, hour: DateTime<Utc>) -> i64 {
    db.read_conn()
        .query_row(
            "SELECT COUNT(*) FROM hourly_performance_rollups WHERE bucket = ?1",
            [perfrollup::hour_bucket(hour)],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn dirty_hour_is_rebuilt_and_cleared() {
    let (_dir, db) = test_db("dirty-rebuild");
    seed_calls(&db, -4, 20);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    let before = fast_agg(&db, hour, hour + Duration::hours(1), "")
        .unwrap()
        .call_count;

    seed_extra_call(&db, -4, "call-dirty-extra");
    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 1);
    assert_eq!(dirty_hours(&db), vec![perfrollup::hour_bucket(hour)]);
    assert_eq!(perf_rows_for(&db, hour), 0, "失效后应删除预聚合行");

    let rebuilt = perfrollup::rebuild_dirty_hours(&db, 48).unwrap();
    assert!(rebuilt >= 1);
    assert!(dirty_hours(&db).is_empty(), "重建成功后应清空 dirty");
    let fast = fast_agg(&db, hour, hour + Duration::hours(1), "").unwrap();
    let raw = perfrollup::scan_calls(&db, hour, hour + Duration::hours(1), "", &[]).unwrap();
    assert_eq!(fast.call_count, raw.call_count);
    assert_eq!(fast.call_count, before + 1, "补采事件只计一次");
}

#[test]
fn dirty_hours_kept_when_rebuild_guard_is_busy() {
    let (_dir, db) = test_db("dirty-guard");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    assert_eq!(perf_rows_for(&db, hour), 1);

    let guard = db.rebuild_guard();
    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 1);
    assert_eq!(dirty_hours(&db).len(), 1, "guard 被占用时仍登记 dirty");
    assert_eq!(
        perf_rows_for(&db, hour),
        1,
        "guard 被占用时不得删除现有 rollup，避免归档推进后既无 rollup 又无明细"
    );
    assert_eq!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap(), 0);
    drop(guard);

    assert!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap() >= 1);
    assert!(dirty_hours(&db).is_empty());
}

#[test]
fn archived_hour_is_not_dirtied_or_deleted() {
    let (_dir, db) = test_db("dirty-archived");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    assert_eq!(perf_rows_for(&db, hour), 1);
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(current_hour() - Duration::hours(3)).to_rfc3339(),
    )
    .unwrap();

    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 0);
    assert!(dirty_hours(&db).is_empty(), "水位前不得写 dirty");
    assert_eq!(perf_rows_for(&db, hour), 1, "水位前不得删除预聚合行");
}

#[test]
fn full_rebuild_clears_dirty_hours() {
    let (_dir, db) = test_db("dirty-full-clear");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 1);
    assert_eq!(dirty_hours(&db).len(), 1);

    perfrollup::rebuild_history(&db).unwrap();
    assert!(
        dirty_hours(&db).is_empty(),
        "全量重建成功应清理覆盖范围 dirty"
    );
    assert_eq!(perf_rows_for(&db, hour), 1);
}

#[test]
fn cancelled_full_rebuild_does_not_write_version_key() {
    let (_dir, db) = test_db("cancel-full");
    seed_calls(&db, -30, 10);
    seed_calls(&db, -1, 10);
    db.cancel_rebuild();
    let written = perfrollup::rebuild_history(&db).unwrap();
    assert_eq!(written, 0);
    assert!(
        db.setting_get(perfrollup::PAYLOAD_VERSION_KEY)
            .unwrap()
            .is_none(),
        "取消后不得写版本键"
    );
    db.reset_rebuild_cancel();
}

#[test]
fn fast_path_range_cap_is_90_days() {
    let (_dir, db) = test_db("range-cap");
    seed_calls(&db, -1, 10);
    let to = Utc::now();
    let from_90 = to - Duration::days(90);
    assert!(
        perfrollup::fast_hours(&db, from_90, to, "")
            .unwrap()
            .is_some(),
        "90 天内应走快路径"
    );
    let from_91 = to - Duration::days(91);
    assert!(
        perfrollup::fast_hours(&db, from_91, to, "")
            .unwrap()
            .is_none(),
        "超过 90 天应回退明细"
    );
}

#[test]
fn full_rebuild_keeps_newer_dirty_marks() {
    let (_dir, db) = test_db("dirty-newer");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    db.conn()
        .execute(
            "INSERT INTO perf_rollup_dirty_hours (hour, marked_at) VALUES (?1, ?2)",
            [
                perfrollup::hour_bucket(hour),
                "2999-01-01T00:00:00+00:00".to_string(),
            ],
        )
        .unwrap();

    perfrollup::rebuild_history(&db).unwrap();
    assert_eq!(
        dirty_hours(&db).len(),
        1,
        "全量重建开始后新写入的 dirty 标记必须保留"
    );
}

fn dirty_mark(db: &HubDb, hour: DateTime<Utc>) -> Option<String> {
    db.read_conn()
        .query_row(
            "SELECT marked_at FROM perf_rollup_dirty_hours WHERE hour = ?1",
            [perfrollup::hour_bucket(hour)],
            |r| r.get(0),
        )
        .ok()
}

fn built_at_for(db: &HubDb, hour: DateTime<Utc>) -> String {
    db.read_conn()
        .query_row(
            "SELECT built_at FROM hourly_performance_rollups WHERE bucket = ?1",
            [perfrollup::hour_bucket(hour)],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn fast_path_does_not_wait_for_writer_connection() {
    let (_dir, db) = test_db("fast-no-writer");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let from = current_hour() - Duration::hours(5);
    let to = Utc::now();

    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder_db = db.clone();
    let holder = std::thread::spawn(move || {
        let c = holder_db.conn();
        let tx = c.unchecked_transaction().unwrap();
        tx.execute(
            "UPDATE settings SET value = value WHERE key = 'archive_watermark'",
            [],
        )
        .unwrap();
        held_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        tx.rollback().unwrap();
    });
    held_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("写事务应在超时前持有连接");

    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let query_db = db.clone();
    let worker = std::thread::spawn(move || {
        let result = perfrollup::fast_aggregate(&query_db, from, to, "")
            .map(|agg| agg.map(|agg| agg.call_count));
        result_tx.send(result).unwrap();
    });
    let result = result_rx.recv_timeout(std::time::Duration::from_secs(5));

    // 无论成功失败都先释放写事务，避免测试线程泄漏。
    let _ = release_tx.send(());
    holder.join().unwrap();
    let result = result.expect("快路径不得等待写连接/写事务");
    assert!(result.unwrap().is_some(), "应命中快速路径");
    worker.join().unwrap();
}

#[test]
fn fast_path_over_90_days_with_archived_range_returns_error() {
    let (_dir, db) = test_db("range-cap-archived");
    seed_calls(&db, -1, 10);
    let to = Utc::now();
    let from_91 = to - Duration::days(91);
    let watermark = (to - Duration::days(60)).to_rfc3339();
    db.setting_set(metria_hub::archive::KEY_ARCHIVE_WATERMARK, &watermark)
        .unwrap();

    assert!(
        perfrollup::fast_hours(&db, from_91, to, "").is_err(),
        "超过 90 天且包含归档区间必须返回错误，不能静默回退到已归档明细"
    );

    let early_watermark = (from_91 - Duration::days(1)).to_rfc3339();
    db.setting_set(metria_hub::archive::KEY_ARCHIVE_WATERMARK, &early_watermark)
        .unwrap();
    assert!(
        perfrollup::fast_hours(&db, from_91, to, "")
            .unwrap()
            .is_none(),
        "完全位于水位之后的超长范围仍可回退明细"
    );

    // 水位落在 from_91 所在小时内但早于 from_91：旧 floor 判断会误报，
    // 原始水位判断应允许回退明细。
    let to_aligned = current_hour() + Duration::minutes(30) + Duration::seconds(30);
    let from_91_aligned = to_aligned - Duration::days(91);
    let in_hour_watermark = from_91_aligned - Duration::minutes(10);
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &in_hour_watermark.to_rfc3339(),
    )
    .unwrap();
    assert!(
        perfrollup::fast_hours(&db, from_91_aligned, to_aligned, "")
            .unwrap()
            .is_none(),
        "from 在原始水位之后、整点 floor 之前时不得误报归档"
    );
}

#[test]
fn fast_path_uses_raw_watermark_for_head_and_tail_fragments() {
    let (_dir, db) = test_db("raw-watermark-fragments");
    seed_calls(&db, -4, 20);
    perfrollup::rebuild_history(&db).unwrap();
    // 预聚合之后补采一条 45 分调用，确保水位后的头部片段确实从明细读到数据。
    seed_extra_call(&db, -4, "raw-watermark-extra");
    let hour = current_hour() - Duration::hours(4);
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(hour + Duration::minutes(30)).to_rfc3339(),
    )
    .unwrap();

    // from 在原始水位之后、ceil_hour(watermark) 之前：明细仍在，必须正常回退。
    let from = hour + Duration::minutes(40);
    let to = hour + Duration::minutes(50);
    let fast = perfrollup::fast_hours(&db, from, to, "")
        .unwrap()
        .expect("水位后的头部片段应回退明细");
    let raw = perfrollup::scan_calls(&db, from, to, "", &[]).unwrap();
    assert_eq!(
        fast.iter().map(|(_, hour)| hour.call_count).sum::<u64>(),
        raw.call_count,
        "水位后的头部片段必须按明细扫描，不得少计"
    );

    // from 在原始水位之前：归档区头部片段必须返回错误。
    assert!(
        perfrollup::fast_hours(&db, hour + Duration::minutes(10), to, "").is_err(),
        "水位前的头部片段必须返回错误"
    );

    // 尾部起点在原始水位之前、终点在水位之后：必须返回错误。
    assert!(
        perfrollup::fast_hours(&db, hour, hour + Duration::minutes(40), "").is_err(),
        "包含归档区的尾部片段必须返回错误"
    );

    // 头/尾完全在原始水位之后：应正常回退明细。
    let tail = perfrollup::fast_hours(
        &db,
        hour + Duration::minutes(30),
        hour + Duration::hours(1) + Duration::minutes(10),
        "",
    )
    .unwrap()
    .expect("水位后的尾部片段应回退明细");
    assert!(!tail.is_empty());
}

#[test]
fn fast_path_storage_error_with_archived_range_returns_error() {
    let (_dir, db) = test_db("storage-archived");
    let hour = current_hour() - Duration::hours(4);
    // 模拟预聚合读取失败（如 SQLITE_BUSY / 表损坏）：把表改名即可让 prepare 失败。
    db.conn()
        .execute(
            "ALTER TABLE hourly_performance_rollups RENAME TO hourly_performance_rollups_bak",
            [],
        )
        .unwrap();

    assert!(
        perfrollup::fast_hours(&db, hour, hour + Duration::hours(1), "")
            .unwrap()
            .is_none(),
        "无归档时读取失败可以回退明细"
    );

    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(hour + Duration::minutes(30)).to_rfc3339(),
    )
    .unwrap();
    assert!(
        perfrollup::fast_hours(&db, hour, hour + Duration::hours(1), "").is_err(),
        "归档区间内读取失败必须返回错误，不能回退到已删除明细"
    );
}

#[test]
fn fast_path_rejects_archived_missing_head_and_tail_fragments() {
    let (_dir, db) = test_db("archived-fragments");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);

    // 缺桶：水位前的小时被删除后无法用明细补齐。
    db.conn()
        .execute(
            "DELETE FROM hourly_performance_rollups WHERE bucket = ?1",
            [perfrollup::hour_bucket(hour)],
        )
        .unwrap();
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(hour + Duration::hours(1) + Duration::minutes(10)).to_rfc3339(),
    )
    .unwrap();
    assert!(
        perfrollup::fast_hours(&db, hour, hour + Duration::hours(1), "").is_err(),
        "归档区缺桶必须返回错误"
    );

    // 恢复该小时 rollup 后测试片段；清空水位才能让 rebuild 覆盖归档前的小时。
    db.conn()
        .execute(
            "DELETE FROM settings WHERE key = ?1",
            [metria_hub::archive::KEY_ARCHIVE_WATERMARK],
        )
        .unwrap();
    perfrollup::rebuild_history(&db).unwrap();

    // 头部片段：from 落在归档区，明细已删除。
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(hour + Duration::minutes(30)).to_rfc3339(),
    )
    .unwrap();
    assert!(
        perfrollup::fast_hours(
            &db,
            hour + Duration::minutes(10),
            hour + Duration::hours(1),
            ""
        )
        .is_err(),
        "归档区头部片段必须返回错误"
    );

    // 尾部片段：to 的整点部分落在归档区，尾部明细已删除。
    db.conn()
        .execute(
            "DELETE FROM settings WHERE key = ?1",
            [metria_hub::archive::KEY_ARCHIVE_WATERMARK],
        )
        .unwrap();
    perfrollup::rebuild_history(&db).unwrap();
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(hour + Duration::hours(2) + Duration::minutes(10)).to_rfc3339(),
    )
    .unwrap();
    assert!(
        perfrollup::fast_hours(
            &db,
            hour,
            hour + Duration::hours(1) + Duration::minutes(10),
            ""
        )
        .is_err(),
        "归档区尾部片段必须返回错误"
    );
}

#[test]
fn consecutive_dirty_hours_rebuild_in_one_window() {
    let (_dir, db) = test_db("dirty-consecutive");
    seed_calls(&db, -5, 10);
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let first = current_hour() - Duration::hours(5);
    let second = current_hour() - Duration::hours(4);
    assert_eq!(
        perfrollup::invalidate_hours(&db, &[first, second]).unwrap(),
        2
    );

    assert_eq!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap(), 2);
    assert!(dirty_hours(&db).is_empty());
    assert_eq!(
        built_at_for(&db, first),
        built_at_for(&db, second),
        "连续小时应合并到同一次 rebuild，共享同一 built_at"
    );
}

#[test]
fn dirty_hour_survives_rebuild_failure_and_retries() {
    let (_dir, db) = test_db("dirty-failure");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 1);

    db.conn()
        .execute("ALTER TABLE model_calls RENAME TO model_calls_bak", [])
        .unwrap();
    assert_eq!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap(), 0);
    assert_eq!(dirty_hours(&db).len(), 1, "重建失败必须保留 dirty 待重试");

    db.conn()
        .execute("ALTER TABLE model_calls_bak RENAME TO model_calls", [])
        .unwrap();
    assert!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap() >= 1);
    assert!(dirty_hours(&db).is_empty());
}

#[test]
fn archived_uncovered_dirty_hour_is_cleaned() {
    let (_dir, db) = test_db("dirty-uncovered-archived");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 1);

    // 归档水位推进到该小时之后：明细已删除，rebuild 被 clamp 到水位，无法覆盖该小时。
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(hour + Duration::hours(2)).to_rfc3339(),
    )
    .unwrap();
    assert_eq!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap(), 0);
    assert!(
        dirty_hours(&db).is_empty(),
        "归档区不可覆盖的 dirty 必须清理，避免每 5 分钟永久重试"
    );
    assert!(
        perfrollup::fast_hours(&db, hour, hour + Duration::hours(1), "").is_err(),
        "该小时必须由归档不可用路径返回错误，而不是伪造 0"
    );
}

#[test]
fn uncovered_dirty_hour_after_watermark_is_kept() {
    let (_dir, db) = test_db("dirty-uncovered-kept");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 1);
    // 水位在该小时之前：该小时属于水位之后，未覆盖时必须保留。
    db.setting_set(
        metria_hub::archive::KEY_ARCHIVE_WATERMARK,
        &(hour - Duration::hours(1)).to_rfc3339(),
    )
    .unwrap();

    // 模拟 rebuild 成功返回但 rollup 未真正落库：水位之后的小时仍应保留重试。
    db.conn()
        .execute(
            "CREATE TRIGGER drop_rebuilt_rollup AFTER INSERT ON hourly_performance_rollups
             BEGIN
               DELETE FROM hourly_performance_rollups WHERE bucket = NEW.bucket;
             END",
            [],
        )
        .unwrap();
    assert_eq!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap(), 0);
    assert_eq!(
        dirty_hours(&db).len(),
        1,
        "水位之后未覆盖的小时必须保留 dirty 待重试"
    );

    db.conn()
        .execute("DROP TRIGGER drop_rebuilt_rollup", [])
        .unwrap();
    assert!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap() >= 1);
    assert!(dirty_hours(&db).is_empty());
}

#[test]
fn current_hour_is_not_dirtied() {
    let (_dir, db) = test_db("dirty-current");
    assert_eq!(
        perfrollup::invalidate_hours(&db, &[current_hour(), current_hour() + Duration::hours(1)])
            .unwrap(),
        0
    );
    assert!(dirty_hours(&db).is_empty(), "当前/未来小时不得写 dirty");
}

#[test]
fn dirty_mark_added_during_rebuild_is_preserved() {
    let (_dir, db) = test_db("dirty-remark");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 1);
    let old_mark = dirty_mark(&db, hour).unwrap();

    // 模拟重建写入 rollup 时同一小时又被晚到补采重新标记。
    db.conn()
        .execute(
            "CREATE TRIGGER dirty_remark AFTER INSERT ON hourly_performance_rollups
             BEGIN
               UPDATE perf_rollup_dirty_hours SET marked_at = '2999-01-01T00:00:00+00:00'
               WHERE hour = NEW.bucket;
             END",
            [],
        )
        .unwrap();
    assert_eq!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap(), 1);
    assert_eq!(dirty_hours(&db).len(), 1, "重建期间的新标记不得删除");
    assert_ne!(dirty_mark(&db, hour).unwrap(), old_mark);

    db.conn().execute("DROP TRIGGER dirty_remark", []).unwrap();
    assert!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap() >= 1);
    assert!(dirty_hours(&db).is_empty());
}

#[test]
fn cancelled_dirty_rebuild_keeps_queue_without_writing_window() {
    let (_dir, db) = test_db("dirty-cancel");
    seed_calls(&db, -4, 10);
    perfrollup::rebuild_history(&db).unwrap();
    let hour = current_hour() - Duration::hours(4);
    assert_eq!(perfrollup::invalidate_hours(&db, &[hour]).unwrap(), 1);

    db.cancel_rebuild();
    let written = perfrollup::rebuild(&db, hour, hour + Duration::hours(1)).unwrap();
    assert_eq!(written, 0, "取消后不得写入该窗口");
    assert_eq!(perf_rows_for(&db, hour), 0);
    assert_eq!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap(), 0);
    assert_eq!(dirty_hours(&db).len(), 1, "取消时必须保留 dirty");

    db.reset_rebuild_cancel();
    assert!(perfrollup::rebuild_dirty_hours(&db, 48).unwrap() >= 1);
    assert!(dirty_hours(&db).is_empty());
}
