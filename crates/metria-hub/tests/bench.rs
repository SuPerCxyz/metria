//! 性能基准（默认忽略，手动运行）：
//!   cargo test -p metria-hub --test bench -- --ignored --nocapture
//!
//! 覆盖：10 万 / 100 万事件批量写入、rollup 重建、Overview 查询、
//! 价格匹配、流量重建。

use std::time::Instant;

use metria_core::model::Usage;
use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_pricing::PricingEngine;
use serde_json::json;

fn bench_cfg(dir: &std::path::Path) -> HubConfig {
    HubConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir.to_path_buf(),
        database_url: format!("sqlite://{}/bench.db", dir.display()),
        content_mode: metria_core::ContentMode::Metadata,
        timezone: chrono_tz::Tz::UTC,
        log_filter: "error".into(),
        demo: false,
    }
}

/// 生成 N 条 usage 事件 JSON（确定性）。
fn gen_usage_json(n: usize) -> Vec<serde_json::Value> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let _ts = 1783137427000i64 + i as i64 * 1000;
        let reasoning: serde_json::Value = if i % 4 == 0 {
            json!(i as i64 % 200)
        } else {
            serde_json::Value::Null
        };
        out.push(serde_json::json!({
            "event_id": format!("bench-e{i}"),
            "schema_version": 1,
            "node_id": format!("node-{}", i % 3),
            "collector_id": format!("collector-node-{}", i % 3),
            "source_id": "src1",
            "client_id": if i % 3 == 0 { "claude-code" } else if i % 3 == 1 { "codex" } else { "opencode" },
            "adapter_id": "bench",
            "adapter_version": "0.1.0",
            "session_id": format!("sess-{}", i % 500),
            "model_call_id": format!("call-{i}"),
            "timestamp": format!("2026-08-0{}T{:02}:{:02}:{:02}Z", (i/3600)%7+1, (i/60)%24, i%60, (i*7)%60),
            "model_raw": if i % 2 == 0 { "claude-sonnet-4-5" } else { "gpt-5-codex" },
            "model_normalized": if i % 2 == 0 { "claude-sonnet-4.5" } else { "gpt-5-codex" },
            "provider_raw": if i % 2 == 0 { "anthropic" } else { "openai" },
            "provider_normalized": if i % 2 == 0 { "anthropic" } else { "openai" },
            "usage": { "input": (i as i64 % 10000) + 500, "output": (i as i64 % 2000) + 50, "cache_read": i as i64 % 3000, "cache_write": i as i64 % 500, "reasoning": reasoning },
            "cost": { "reported_micro_usd": serde_json::Value::Null, "calculated_micro_usd": 1000 + (i as i64 % 5000), "estimated_micro_usd": serde_json::Value::Null, "pricing_rule_id": serde_json::Value::Null, "pricing_snapshot_id": serde_json::Value::Null },
            "quality": { "usage_source": "reported", "granularity": "call", "confidence": 1.0 }
        }));
    }
    // 保证时间戳落在桶内
    out
}

fn run_ingest_bench(n: usize) {
    let dir = std::env::temp_dir().join(format!("metria-bench-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = bench_cfg(&dir);
    let db = HubDb::open(&cfg).unwrap();
    db.apply_migrations().unwrap();

    let events = gen_usage_json(n);
    let key = "node-0:sess-0";

    // 批量写入（事务）
    let t0 = Instant::now();
    {
        let mut conn = db.conn();
        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO usage_events (
                        event_id, schema_version, node_id, collector_id, source_id, client_id, adapter_id, adapter_version,
                        session_id, turn_id, model_call_id, timestamp, provider_raw, provider_normalized, model_raw, model_normalized,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                        reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd, pricing_rule_id, pricing_snapshot_id,
                        usage_source, usage_granularity, usage_confidence
                    ) VALUES (?1,1,?2,?3,'src1',?4,'bench','0.1.0',?5,NULL,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,NULL,NULL,NULL,'reported','call',1.0)",
                )
                .unwrap();
            for e in &events {
                let u = &e["usage"];
                let c = &e["cost"];
                let q = &e["quality"];
                let _ = stmt.execute(metria_storage::rusqlite::params![
                    e["event_id"].as_str().unwrap(),
                    e["node_id"].as_str().unwrap(),
                    e["collector_id"].as_str().unwrap(),
                    e["client_id"].as_str().unwrap(),
                    e["session_id"].as_str().unwrap(),
                    e["model_call_id"].as_str().unwrap(),
                    e["timestamp"].as_str().unwrap(),
                    e["provider_raw"].as_str().unwrap(),
                    e["provider_normalized"].as_str().unwrap(),
                    e["model_raw"].as_str().unwrap(),
                    e["model_normalized"].as_str().unwrap(),
                    u["input"].as_i64(),
                    u["output"].as_i64(),
                    u["cache_read"].as_i64(),
                    u["cache_write"].as_i64(),
                    u["reasoning"].as_i64(),
                    c["reported_micro_usd"].as_i64(),
                    c["calculated_micro_usd"].as_i64(),
                    c["estimated_micro_usd"].as_i64(),
                    q["confidence"].as_f64(),
                ]);
            }
        }
        tx.commit().unwrap();
    }
    let write_ms = t0.elapsed().as_millis();

    // rollup 重建（从事件表聚合）
    let t1 = Instant::now();
    {
        let conn = db.conn();
        let _ = conn.execute_batch(
            "INSERT OR REPLACE INTO hourly_rollups (
                bucket, node_id, collector_id, client_id, source_id, project_id, provider, model,
                usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                reported_cost, calculated_cost, estimated_cost,
                estimated_request_bytes, estimated_response_bytes, estimated_total_bytes,
                estimated_lower_bound_bytes, estimated_upper_bound_bytes,
                session_count, model_call_count, turn_count, message_count, tool_call_count, subagent_count
            )
            SELECT substr(timestamp,1,13)||':00:00Z', node_id, collector_id, client_id, source_id, '', provider_normalized, model_normalized,
                'reported', 'call', 'calculated', '', '',
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0), COALESCE(SUM(reasoning_tokens),0),
                COALESCE(SUM(reported_cost_micro_usd),0), COALESCE(SUM(calculated_cost_micro_usd),0), COALESCE(SUM(estimated_cost_micro_usd),0),
                0,0,0,0,0,
                0, COUNT(*), 0, 0, 0, 0
            FROM usage_events GROUP BY substr(timestamp,1,13), node_id, collector_id, client_id, source_id, provider_normalized, model_normalized",
        );
    }
    let rollup_ms = t1.elapsed().as_millis();

    // Overview 查询（读 rollup）
    let t2 = Instant::now();
    let overview_ms = {
        let c = db.conn();
        let n: i64 = c
            .query_row(
                "SELECT COALESCE(SUM(input_tokens),0) FROM hourly_rollups",
                [],
                |r| r.get(0),
            )
            .unwrap();
        n
    };
    let query_ms = t2.elapsed().as_micros();

    let _ = key;
    println!(
        "[bench {n}] 写入 {write_ms}ms ({}/s) | rollup 重建 {rollup_ms}ms | overview 查询 {query_ms}us (input={overview_ms})",
        (n as u128 * 1000) / write_ms.max(1)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore]
fn bench_100k_events() {
    run_ingest_bench(100_000);
}

#[test]
#[ignore]
fn bench_1m_events() {
    run_ingest_bench(1_000_000);
}

#[test]
#[ignore]
fn bench_price_match_10k() {
    let engine = PricingEngine::new();
    let usage = Usage {
        input: Some(1000),
        output: Some(500),
        cache_read: Some(100),
        cache_write: Some(50),
        reasoning: Some(10),
    };
    let t = Instant::now();
    let mut total = 0i64;
    for _ in 0..10_000 {
        let r = engine
            .compute(
                &usage,
                Some("claude-sonnet-4.5"),
                Some("anthropic"),
                chrono::Utc::now(),
                None,
            )
            .unwrap();
        total += r.calculated_micro_usd.unwrap_or(0);
    }
    let ms = t.elapsed().as_millis();
    println!(
        "[bench 价格匹配] 10k 次 {ms}ms（{}/ms），sample={total}",
        10_000u64 / ms.max(1) as u64
    );
}

#[test]
#[ignore]
fn bench_traffic_reconstruction_10k() {
    let t = Instant::now();
    for _ in 0..10_000 {
        let input = metria_traffic::EstimateInput {
            client: "claude-code",
            provider: Some("anthropic"),
            model: Some("claude-sonnet-4.5"),
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_tokens: Some(100),
            cache_write_tokens: Some(50),
            reasoning_tokens: Some(10),
            streaming: true,
            request_text: None,
            response_text: None,
            request_reconstruction_quality: metria_core::model::ReconstructionQuality::None,
            response_reconstruction_quality: metria_core::model::ReconstructionQuality::None,
            context_transport_mode: metria_core::model::ContextTransportMode::Unknown,
            cache_transport_behavior: metria_core::model::CacheTransportBehavior::Unknown,
        };
        let _ = metria_traffic::estimate(&input).unwrap();
    }
    let ms = t.elapsed().as_millis();
    println!(
        "[bench 流量重建] 10k 次 {ms}ms（{}/ms）",
        10_000u64 / ms.max(1) as u64
    );
}
