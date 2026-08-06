//! Demo 模式：生成确定性合成数据，展示多节点/多客户端/多模型/多项目。
//!
//! 不读取真实客户端目录，不包含真实用户信息；使用固定种子 PRNG，数据可复现。

use chrono::{DateTime, Utc};
use metria_core::model::EventId;
use serde_json::{json, Value};

use crate::db::HubDb;

/// 确定性 PRNG（xorshift64）。
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

const NODES: &[&str] = &["demo-node-01", "demo-node-02", "demo-node-03"];
const CLIENTS: &[(&str, &str)] = &[
    ("claude-code", "Claude Code"),
    ("codex", "Codex"),
    ("opencode", "OpenCode"),
];
const MODELS: &[(&str, &str)] = &[
    ("claude-sonnet-4-5", "anthropic"),
    ("claude-opus-4-6", "anthropic"),
    ("gpt-5-codex", "openai"),
    ("o3-mini", "openai"),
    ("deepseek-chat", "deepseek"),
];
const PROJECTS: &[&str] = &[
    "project-alpha",
    "project-beta",
    "project-gamma",
    "project-delta",
];

/// 向空数据库写入 demo 数据。幂等：重复调用在非空库上跳过。
pub fn seed_demo(db: &HubDb) -> Result<(), String> {
    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if count > 0 {
        return Ok(()); // 已有数据，避免重复生成
    }
    let mut rng = Rng(0x5eed_c0de_2026);
    let now = Utc::now();

    for node in NODES {
        let _ =
            db.register_node_collector(node, node, Some("linux"), Some("x86_64"), "0.1.0", 1, now);
    }

    // 7 天 × 每节点每小时若干事件（全部落在过去：起始回退 1 小时避免边界未来数据）
    let days = 7i64;
    for day in 0..days {
        for hour in 0..24 {
            let base = now
                - chrono::Duration::days(day)
                - chrono::Duration::hours(hour)
                - chrono::Duration::hours(1);
            for node in NODES {
                let activity = rng.range(0, 2);
                for _ in 0..activity {
                    let (client, display) =
                        CLIENTS[rng.range(0, CLIENTS.len() as u64 - 1) as usize];
                    let (model, provider) = MODELS[rng.range(0, MODELS.len() as u64 - 1) as usize];
                    let project = rng.pick(PROJECTS);
                    gen_session(
                        db, &mut rng, node, client, display, model, provider, project, base,
                    );
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn gen_session(
    db: &HubDb,
    rng: &mut Rng,
    node: &str,
    client: &str,
    _display: &str,
    model: &str,
    provider: &str,
    project: &str,
    base: DateTime<Utc>,
) {
    let sid = format!("demo-{}-{}-{}", node, client, base.timestamp_millis());
    let collector_id = format!("collector-{node}");
    let source_id = format!("src-{client}");
    let started = base + chrono::Duration::minutes(rng.range(0, 55) as i64);
    let calls = rng.range(1, 7);

    // source 事件
    let src_event = json!({
        "id": format!("{source_id}-{node}"),
        "node_id": node,
        "collector_id": collector_id,
        "client_id": client,
        "adapter_id": client,
        "adapter_version": "0.1.0",
        "source_fingerprint": format!("{client}:{node}"),
        "source_path_hash": EventId::from_content(&format!("path:{node}:{client}")).as_str(),
        "status": "active",
    });
    let _ = db.upsert_source(&src_event);

    // session 事件
    let session_key = HubDb::session_key(node, &sid);
    let session = json!({
        "id": format!("sess-{sid}"),
        "source_session_id": sid,
        "node_id": node,
        "collector_id": collector_id,
        "source_id": format!("{source_id}-{node}"),
        "client_id": client,
        "project_id": project,
        "title": format!("{} 演示会话", client),
        "started_at": started.to_rfc3339(),
        "status": "ended",
        "message_count": calls * 3,
        "tool_call_count": calls,
        "model_call_count": calls,
        "primary_model_raw": model,
        "primary_model_normalized": metria_core::normalize::normalize_model(model),
        "provider_raw": provider,
        "provider_normalized": provider,
        "created_at": started.to_rfc3339(),
    });
    let _ = db.upsert_session(&session);
    let _ = db.rollup_event("session", &session);

    let mut total_input = 0i64;
    let mut total_output = 0i64;
    for i in 0..calls {
        let ts = started + chrono::Duration::seconds((i * 7 + 1) as i64);
        let input = rng.range(500, 40_000) as i64;
        let output = rng.range(100, 4_000) as i64;
        let cache_read = if rng.next() % 2 == 0 {
            rng.range(0, input as u64 / 2) as i64
        } else {
            0
        };
        let cache_write = if rng.next() % 3 == 0 {
            rng.range(0, 3000) as i64
        } else {
            0
        };
        let reasoning = if provider == "openai" || provider == "deepseek" {
            rng.range(0, output as u64 / 2) as i64
        } else {
            0
        };
        total_input += input;
        total_output += output;
        let call_id = format!("call-{sid}-{i}");

        let call = json!({
            "id": call_id,
            "source_call_id": format!("c{i}"),
            "node_id": node,
            "collector_id": collector_id,
            "client_id": client,
            "source_id": format!("{source_id}-{node}"),
            "project_id": project,
            "session_id": format!("sess-{sid}"),
            "model_raw": model,
            "model_normalized": metria_core::normalize::normalize_model(model),
            "provider_raw": provider,
            "provider_normalized": provider,
            "started_at": ts.to_rfc3339(),
            "status": "success",
            "call_granularity": "call",
            "input_tokens": input,
            "output_tokens": output,
            "cache_read_tokens": cache_read,
            "cache_write_tokens": cache_write,
            "reasoning_tokens": reasoning,
        });
        let _ = db.insert_call(&call, &session_key);
        let _ = db.rollup_event("call", &call);

        // usage 事件（token + cost 归入 rollup）
        let usage = json!({
            "event_id": EventId::from_content(&format!("usage-{call_id}")).as_str(),
            "schema_version": 1,
            "node_id": node,
            "collector_id": collector_id,
            "source_id": format!("{source_id}-{node}"),
            "client_id": client,
            "adapter_id": client,
            "adapter_version": "0.1.0",
            "session_id": sid,
            "model_call_id": call_id,
            "timestamp": ts.to_rfc3339(),
            "model_raw": model,
            "model_normalized": metria_core::normalize::normalize_model(model),
            "provider_raw": provider,
            "provider_normalized": provider,
            "usage": { "input": input, "output": output, "cache_read": cache_read, "cache_write": cache_write, "reasoning": reasoning },
            "cost": { "reported_micro_usd": Value::Null, "calculated_micro_usd": (input / 1000) * 3000 + (output / 1000) * 15000, "estimated_micro_usd": Value::Null, "pricing_rule_id": Value::Null, "pricing_snapshot_id": Value::Null },
            "quality": { "usage_source": "reported", "granularity": "call", "confidence": 1.0 },
        });
        if db.insert_usage(&usage, &session_key).unwrap_or(false) {
            let _ = db.rollup_event("usage", &usage);
        }

        // 流量（来源：token_profile / partial_reconstruction 混合，体现不同置信度）
        let est_source = if rng.next() % 4 == 0 {
            "token_profile"
        } else {
            "partial_reconstruction"
        };
        let conf = if est_source == "partial_reconstruction" {
            0.65
        } else {
            0.5
        };
        let req = (input as f64 * 3.8).round() as i64;
        let resp = (output as f64 * 4.0).round() as i64;
        let total = req + resp;
        let traffic = json!({
            "id": format!("te-{call_id}"),
            "model_call_id": call_id,
            "node_id": node,
            "client_id": client,
            "session_id": format!("sess-{sid}"),
            "model": metria_core::normalize::normalize_model(model),
            "estimated_request_wire_bytes": req,
            "estimated_response_wire_bytes": resp,
            "estimated_total_wire_bytes": total,
            "lower_bound_bytes": (total as f64 * 0.7).round() as i64,
            "upper_bound_bytes": (total as f64 * 1.4).round() as i64,
            "estimation_source": est_source,
            "context_transport_mode": if client == "codex" && rng.next() % 2 == 0 { "stateful_reference" } else { "full_context" },
            "cache_transport_behavior": if cache_read > 0 { "full_content_sent" } else { "unknown" },
            "request_reconstruction_quality": "partial",
            "response_reconstruction_quality": "complete",
            "confidence": conf,
            "calculated_at": ts.to_rfc3339(),
        });
        let _ = db.insert_traffic(&traffic);
        let _ = db.rollup_event("traffic", &traffic);
    }

    let _ = total_input;
    let _ = total_output;
}

/// 生成部分 message 事件（metadata 模式：无正文）。
#[allow(dead_code)]
fn _gen_messages(db: &HubDb, session_key: &str, calls: u64, base: DateTime<Utc>) {
    for i in 0..calls {
        let m = json!({
            "id": format!("msg-demo-{session_key}-{i}"),
            "session_id": session_key,
            "sequence": i as i64,
            "role": if i % 2 == 0 { "user" } else { "assistant" },
            "content_type": "text",
            "content": Value::Null,
            "content_length": 100 + i as i64 * 3,
            "utf8_bytes": 100 + i as i64 * 3,
            "redacted": true,
        });
        let _ = db.insert_message(&m, session_key);
        let _ = base;
    }
}
