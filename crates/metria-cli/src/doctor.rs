//! metria doctor：环境诊断检查。

use std::path::PathBuf;
use std::time::Instant;

use crate::registry;
use metria_adapter_api::{DiscoveryContext, ScanIdentity};

/// doctor 子命令入口。
pub fn run_doctor(
    adapter: Option<&str>,
    hub: bool,
    database: bool,
    spool: bool,
    traffic: bool,
) -> Result<(), String> {
    let mut failures = 0usize;

    if let Some(name) = adapter {
        failures += check_adapter(name)?;
    }
    if traffic {
        check_traffic();
    }
    if hub && check_hub().is_err() {
        failures += 1;
    }
    if database && check_database().is_err() {
        failures += 1;
    }
    if spool && check_spool().is_err() {
        failures += 1;
    }
    if adapter.is_none() && !traffic && !hub && !database && !spool {
        eprintln!(
            "用法：metria doctor [--adapter <name>] [--traffic] [--hub] [--database] [--spool]"
        );
    }

    if failures > 0 {
        return Err(format!("诊断发现 {failures} 项失败"));
    }
    Ok(())
}

fn check_adapter(name: &str) -> Result<usize, String> {
    let adapter = registry::adapter(name).ok_or_else(|| format!("未知 Adapter `{name}`"))?;
    println!(
        "== Adapter: {} ({} v{}) ==",
        adapter.display_name(),
        adapter.id(),
        adapter.version()
    );

    let ctx = DiscoveryContext {
        node_id: "doctor".into(),
        collector_id: "doctor".into(),
        root_paths: vec![default_root(name)],
    };
    let sources = adapter.discover(&ctx).unwrap_or_default();
    println!("发现 {} 个来源", sources.len());
    let mut failures = 0usize;
    let identity = ScanIdentity {
        node_id: "doctor".into(),
        collector_id: "doctor".into(),
    };
    for s in &sources {
        let health = adapter
            .health(s)
            .unwrap_or_else(|e| metria_adapter_api::SourceHealth {
                ok: false,
                status: metria_core::model::SourceStatus::Error,
                message: Some(e.to_string()),
                last_error: None,
            });
        let status = if health.ok { "OK" } else { "FAIL" };
        if !health.ok {
            failures += 1;
        }
        let msg = health.message.clone().unwrap_or_default();
        let scan = adapter.scan(s, None, &identity).ok();
        let scan_info = scan
            .map(|b| {
                format!(
                    "sessions={} usage={} warnings={}",
                    b.sessions.len(),
                    b.usage_events.len(),
                    b.warnings.len()
                )
            })
            .unwrap_or_else(|| "scan=N/A".into());
        println!(
            "  [{status}] {} | {msg} | {scan_info}",
            s.canonical_path.display()
        );
    }
    Ok(failures)
}

fn default_root(name: &str) -> PathBuf {
    std::env::var(env_key(name))
        .map(PathBuf::from)
        .unwrap_or_else(|_| match name {
            "claude" | "claude-code" => PathBuf::from("/sources/claude"),
            "codex" => PathBuf::from("/sources/codex"),
            "opencode" => PathBuf::from("/sources/opencode"),
            _ => PathBuf::from("/sources"),
        })
}

fn env_key(name: &str) -> String {
    match name {
        "claude" | "claude-code" => "METRIA_CLAUDE_PATH".into(),
        "codex" => "METRIA_CODEX_PATH".into(),
        "opencode" => "METRIA_OPENCODE_PATH".into(),
        _ => "METRIA_SOURCES_PATH".into(),
    }
}

fn check_traffic() {
    println!("== Traffic 估算能力 ==");
    for a in registry::all() {
        let caps = a.capabilities();
        println!(
            "- {}: request_reconstruction={} response_reconstruction={} context_transport_detection={} cache_tokens={} reasoning_tokens={}",
            a.display_name(),
            caps.request_reconstruction,
            caps.response_reconstruction,
            caps.context_transport_detection,
            caps.cache_tokens,
            caps.reasoning_tokens,
        );
    }
}

fn check_hub() -> Result<(), String> {
    let url = std::env::var("METRIA_HUB_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    let endpoint = format!("{}/healthz", url.trim_end_matches('/'));
    println!("== Hub 连通性 ==");
    let start = Instant::now();
    let resp = ureq::get(&endpoint)
        .timeout(std::time::Duration::from_secs(5))
        .call();
    let elapsed = start.elapsed();
    match resp {
        Ok(r) => {
            println!(
                "  [OK] {} ({:?}, {}ms)",
                endpoint,
                r.status(),
                elapsed.as_millis()
            );
            Ok(())
        }
        Err(ureq::Error::Status(code, _)) => {
            println!("  [WARN] HTTP {code} from {endpoint}");
            Ok(())
        }
        Err(e) => {
            println!("  [FAIL] {endpoint}: {e}");
            Err(e.to_string())
        }
    }
}

fn check_database() -> Result<(), String> {
    println!("== Hub 数据库 ==");
    let cfg = metria_hub::HubConfig::from_env().map_err(|e| e.to_string())?;
    let db = metria_hub::db::HubDb::open(&cfg).map_err(|e| e.to_string())?;
    let version = db.schema_version().map_err(|e| e.to_string())?;
    println!("  schema 版本: {version}");
    match db.quick_check() {
        Ok(()) => println!("  完整性检查: OK"),
        Err(e) => {
            println!("  完整性检查: FAIL ({e})");
            return Err(e.to_string());
        }
    }
    let calls: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM model_calls", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let usage: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let rollup_calls: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(SUM(model_call_count),0) FROM hourly_rollups",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    println!("  model_calls={calls} usage_events={usage} rollup_model_calls={rollup_calls}");
    if rollup_calls > calls {
        println!("  [WARN] rollup 计数高于事件数（重复统计风险）");
    }
    Ok(())
}

fn check_spool() -> Result<(), String> {
    println!("== Agent Spool ==");
    let data_dir = std::env::var("METRIA_DATA_DIR").unwrap_or_else(|_| "/data".into());
    let spool = metria_agent::spool::Spool::open(
        &PathBuf::from(&data_dir).join("spool.db"),
        2_000_000,
        512 * 1024 * 1024,
    )
    .map_err(|e: metria_agent::AgentError| e.to_string())?;
    println!("  pending_events={}", spool.pending_count());
    println!("  spool_bytes={}", spool.spool_bytes());
    println!("  dead_letters={}", spool.dead_letter_count());
    if spool.full_flag().is_full() {
        println!("  [WARN] Spool 满：Agent 已停止采集并告警");
    }
    if let Some((batch, err)) = spool.last_batch_error() {
        println!("  [WARN] 最近上传失败: {batch} => {err}");
    }
    let health = spool.source_health_all();
    for (sid, ok, err) in health {
        println!(
            "  source {}: {} {}",
            sid,
            if ok { "OK" } else { "FAIL" },
            err
        );
    }
    Ok(())
}
