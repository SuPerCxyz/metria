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
    let base = url.trim_end_matches('/');
    let endpoint = format!("{base}/healthz");
    println!("== Hub 连通性 ==");
    let start = Instant::now();
    let resp = ureq::get(&endpoint)
        .timeout(std::time::Duration::from_secs(5))
        .call();
    let elapsed = start.elapsed();

    // TLS 检测：https 协议（连接成功即视为证书校验通过）
    let scheme = base.split("://").next().unwrap_or("http");
    if scheme == "https" {
        println!("  [INFO] TLS 已启用（{base}），证书校验由 rustls 完成");
    } else {
        println!("  [WARN] 未启用 TLS（http）。生产建议配置 TLS 终止（反代 / ingress）");
    }

    match resp {
        Ok(r) => {
            println!(
                "  [OK] {endpoint} ({}, 往返 {:.0}ms)",
                r.status(),
                elapsed.as_millis()
            );
            check_hub_time(base);
            check_hub_upload(base)?;
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

/// 尝试查询节点与最近上传信息（S2.17）。需 admin 凭据，缺失时提示跳过。
fn check_hub_upload(base: &str) -> Result<(), String> {
    let (user, pass) = (
        std::env::var("METRIA_ADMIN_USER").unwrap_or_default(),
        std::env::var("METRIA_ADMIN_PASSWORD").unwrap_or_default(),
    );
    if user.is_empty() || pass.is_empty() {
        println!("  [INFO] 未提供 METRIA_ADMIN_USER/PASSWORD，跳过最近上传检查");
        return Ok(());
    }
    let login = ureq::post(&format!("{base}/api/v1/auth/login"))
        .send_json(serde_json::json!({ "username": user, "password": pass }))
        .ok();
    let Some(token) = login
        .and_then(|r| r.into_json::<serde_json::Value>().ok())
        .and_then(|v| v.get("token").cloned())
        .and_then(|t| t.as_str().map(String::from))
    else {
        println!("  [WARN] Hub 登录失败，跳过最近上传检查");
        return Ok(());
    };
    match ureq::get(&format!("{base}/api/v1/nodes"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(r) => {
            if let Ok(body) = r.into_json::<serde_json::Value>() {
                let nodes = body.get("nodes").and_then(|v| v.as_array());
                let n = nodes.map(|n| n.len()).unwrap_or(0);
                println!("  [OK] 节点数: {n}");
                // 最近上传：遍历节点 detail 取 collector last_upload_at
                if let Some(arr) = nodes {
                    let mut last_upload: Option<(String, String)> = None;
                    let mut last_heartbeat: Option<(String, String)> = None;
                    for node in arr {
                        let id = node.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        if let Ok(detail) = ureq::get(&format!("{base}/api/v1/nodes/{id}"))
                            .set("Authorization", &format!("Bearer {token}"))
                            .timeout(std::time::Duration::from_secs(5))
                            .call()
                        {
                            if let Ok(dbody) = detail.into_json::<serde_json::Value>() {
                                for c in dbody
                                    .get("collectors")
                                    .and_then(|v| v.as_array())
                                    .unwrap_or(&vec![])
                                {
                                    if let Some(u) =
                                        c.get("last_upload_at").and_then(|v| v.as_str())
                                    {
                                        if last_upload
                                            .as_ref()
                                            .map(|(_, t)| u > t.as_str())
                                            .unwrap_or(true)
                                        {
                                            last_upload = Some((id.to_string(), u.to_string()));
                                        }
                                    }
                                    if let Some(h) =
                                        c.get("last_heartbeat_at").and_then(|v| v.as_str())
                                    {
                                        if last_heartbeat
                                            .as_ref()
                                            .map(|(_, t)| h > t.as_str())
                                            .unwrap_or(true)
                                        {
                                            last_heartbeat = Some((id.to_string(), h.to_string()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    match last_upload {
                        Some((node_id, ts)) => {
                            println!("  [OK] 最近上传: node={node_id} at {ts}");
                        }
                        None => println!("  [WARN] 未发现任何 Collector 上传记录"),
                    }
                    match last_heartbeat {
                        Some((node_id, ts)) => {
                            println!("  [OK] 最近心跳: node={node_id} at {ts}");
                        }
                        None => println!("  [WARN] 未发现任何 Collector 心跳记录"),
                    }
                }
            }
            Ok(())
        }
        Err(e) => {
            println!("  [WARN] 查询 nodes 失败: {e}");
            Ok(())
        }
    }
}

/// 检查本机与 Hub 的时钟偏差（S2.17）：对比 Hub 响应头日期与本机时间。
fn check_hub_time(base: &str) {
    println!("== Hub 时间差 ==");
    let resp = ureq::get(&format!("{base}/healthz"))
        .timeout(std::time::Duration::from_secs(5))
        .call();
    let hub_date = match resp {
        Ok(r) => r.header("Date").map(|s| s.to_string()),
        Err(e) => {
            println!("  [FAIL] 无法获取 Hub 时间: {e}");
            return;
        }
    };
    let Some(hub_date) = hub_date else {
        println!("  [WARN] Hub 未返回 Date 响应头，跳过时间差检查");
        return;
    };
    let Ok(hub_http_date) = httpdate::parse_http_date(&hub_date) else {
        println!("  [WARN] 无法解析 Hub Date 头（{hub_date}），跳过时间差检查");
        return;
    };
    let now = std::time::SystemTime::now();
    let skew = now
        .duration_since(hub_http_date)
        .or_else(|_| hub_http_date.duration_since(now))
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if skew > 60.0 {
        println!(
            "  [WARN] 本机与 Hub 时钟偏差约 {:.0}s（Hub: {hub_date}）。过大偏差会导致时间戳与告警失真",
            skew
        );
    } else {
        println!(
            "  [OK] 本机与 Hub 时钟偏差 {:.0}s 以内（Hub: {hub_date}）",
            skew
        );
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
