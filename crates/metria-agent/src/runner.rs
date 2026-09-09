//! Agent 主循环：注册 → 游标探测 → （push）无状态轮询 / （pull）本地采集+Pull 服务 → 心跳。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use metria_adapter_api::ScanIdentity;
use metria_core::model::EventId;
use metria_protocol::{HeartbeatRequest, RegisterRequest};

use crate::config::AgentConfig;
use crate::error::{AgentError, Result};
use crate::scanner::Scanner;
use crate::spool::Spool;
use crate::stateless::{polling_loop, sleep_interruptible};
use crate::wire::HubClient;

/// 运行 Agent（阻塞直至退出信号）。
pub fn run(cfg: AgentConfig) -> Result<()> {
    let token = crate::config::resolve_token(&cfg);

    // 双模式判定：配置 HUB_URL → push（无状态轮询）；仅 token → pull（Hub 主动拉取）
    match (&cfg.hub_url, &token) {
        (Some(_), _) => run_push_stateless(cfg, token),
        (None, Some(_)) => run_pull(cfg, token),
        (None, None) => Err(AgentError::Internal(
            "pull 模式需要 METRIA_AGENT_TOKEN；或配置 METRIA_HUB_URL 走无状态 push 模式".into(),
        )),
    }
}

/// Pull 模式：本地采集 + 暴露采集 API，Hub 主动拉取；无需注册/上传/心跳出站。
/// 该模式保留本地 spool（Hub 拉取-确认语义依赖本地待拉取队列）。
pub fn run_pull(cfg: AgentConfig, token: Option<String>) -> Result<()> {
    std::fs::create_dir_all(&cfg.data_dir)?;
    let token = token.unwrap_or_default();
    tracing::info!(
        port = cfg.listen_port,
        "Agent 以 pull 模式启动（Hub 主动拉取），本地采集照常进行"
    );
    let stop = Arc::new(AtomicBool::new(false));
    spawn_stop_handler(stop.clone());

    // 扫描线程（identity 使用本地占位，Hub 侧按 token 归属重写）
    let identity = ScanIdentity {
        node_id: "pull".into(),
        collector_id: "pull".into(),
    };
    let scan_cfg = cfg.clone();
    let scan_spool = reopen_spool(&cfg)?;
    let scan_stop = stop.clone();
    std::thread::spawn(move || {
        if let Err(e) = scanner_loop(scan_cfg, scan_spool, identity, scan_stop) {
            tracing::error!("扫描线程退出: {e}");
        }
    });

    // Pull 服务线程
    let serve_cfg = cfg.clone();
    let serve_stop = stop.clone();
    std::thread::spawn(move || {
        if let Err(e) = crate::pullserver::serve(serve_cfg, token, serve_stop) {
            tracing::error!("Pull 服务退出: {e}");
        }
    });

    // 主线程等待退出
    wait_for_stop(stop);
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

/// Push 模式：无状态轮询（游标外置 Hub，无本地 spool）。
///
/// 启动流程：注册 → 游标能力探测（Hub 离线退避等待；不支持则明确退出）→
/// 遗留 spool 一次性迁移 → 轮询线程 + 心跳线程。
fn run_push_stateless(cfg: AgentConfig, token: Option<String>) -> Result<()> {
    std::fs::create_dir_all(&cfg.data_dir)?;
    let hub_url = cfg
        .hub_url
        .clone()
        .unwrap_or_else(|| "http://localhost:8080".into());
    let client = HubClient::new(&hub_url, token);
    let node_id = resolve_node_id(&cfg);

    let stop = Arc::new(AtomicBool::new(false));
    spawn_stop_handler(stop.clone());

    // 注册 + 游标能力探测（Hub 离线 → 退避等待，不退出）
    let identity = loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match try_start(&cfg, &client, node_id.clone()) {
            Ok(identity) => break identity,
            Err(StartError::Unsupported) => {
                return Err(AgentError::Internal(
                    "Hub 未提供游标同步接口：无状态模式需先升级 Hub（升级后重启 Agent 即可）"
                        .into(),
                ));
            }
            Err(StartError::Transient(e)) => {
                tracing::warn!("注册/游标探测失败，退避重试: {e}");
                if sleep_interruptible(&stop, Duration::from_secs(10)) {
                    return Ok(());
                }
            }
        }
    };
    tracing::info!(
        node = %identity.node_id,
        collector = %identity.collector_id,
        "Agent 注册完成（无状态轮询模式）"
    );

    // 遗留 spool 一次性迁移（失败即退出，容器重启后重试）
    crate::stateless::migrate_legacy_spool(&cfg, &client, &identity)?;

    // 轮询线程
    let poll_cfg = cfg.clone();
    let poll_client = client.clone();
    let poll_identity = identity.clone();
    let poll_stop = stop.clone();
    std::thread::spawn(move || {
        if let Err(e) = polling_loop(poll_cfg, poll_client, poll_identity, poll_stop) {
            tracing::error!("轮询线程退出: {e}");
        }
    });

    // 心跳线程
    let hb_cfg = cfg.clone();
    let hb_client = client;
    let hb_stop = stop.clone();
    std::thread::spawn(move || {
        if let Err(e) = heartbeat_loop(hb_cfg, hb_client, identity, hb_stop) {
            tracing::error!("心跳线程退出: {e}");
        }
    });

    // 主线程等待退出信号；给线程一个心跳周期的收尾时间
    wait_for_stop(stop);
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

enum StartError {
    /// Hub 不支持游标同步（旧版 Hub），需先升级。
    Unsupported,
    /// 网络等瞬态错误，可退避重试。
    Transient(AgentError),
}

fn try_start(
    cfg: &AgentConfig,
    client: &HubClient,
    node_id: String,
) -> std::result::Result<ScanIdentity, StartError> {
    let identity = register(cfg, node_id, client).map_err(StartError::Transient)?;
    match client.fetch_cursors() {
        Ok(Some(_)) => Ok(identity),
        Ok(None) => Err(StartError::Unsupported),
        Err(e) => Err(StartError::Transient(e)),
    }
}

fn spawn_stop_handler(stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        // ctrlc crate 同时处理 SIGINT 与 SIGTERM（cargo 特性 signal-hook）
        let _ = ctrlc::set_handler(move || {
            tracing::info!("收到退出信号（SIGINT/SIGTERM）");
            stop.store(true, Ordering::Relaxed);
        });
    });
}

fn wait_for_stop(stop: Arc<AtomicBool>) {
    loop {
        if stop.load(Ordering::Relaxed) {
            tracing::info!("Agent 退出，等待子线程收尾");
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// 重新打开本地 Spool（Pull 模式使用）。
fn reopen_spool(cfg: &AgentConfig) -> Result<Spool> {
    Spool::open(
        &cfg.data_dir.join("spool.db"),
        cfg.max_pending_events,
        cfg.max_spool_bytes,
    )
}

/// Node 身份：显式 METRIA_NODE_ID > 按 node_name 确定性派生（无本地持久化）。
fn resolve_node_id(cfg: &AgentConfig) -> String {
    if !cfg.node_id.trim().is_empty() {
        return cfg.node_id.trim().to_string();
    }
    let gen = EventId::from_content(&format!("node:{}", cfg.node_name));
    format!("node-{}", &gen.as_str()[7..15])
}

fn register(cfg: &AgentConfig, node_id: String, client: &HubClient) -> Result<ScanIdentity> {
    let platform = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let req = RegisterRequest {
        schema_version: metria_protocol::limits::SCHEMA_VERSION,
        node_id,
        node_name: cfg.node_name.clone(),
        node_platform: Some(platform),
        node_architecture: Some(arch),
        node_timezone: Some("UTC".into()),
        agent_version: metria_core::VERSION.to_string(),
        protocol_version: metria_protocol::limits::PROTOCOL_VERSION,
        container_image: None,
        collector_id_hint: None,
    };
    let resp = client.register(&req)?;
    if !resp.ok {
        return Err(AgentError::Http(format!(
            "注册失败: {}",
            resp.message.unwrap_or_default()
        )));
    }
    Ok(ScanIdentity {
        node_id: resp.node_id,
        collector_id: resp.collector_id,
    })
}

/// Pull 模式扫描线程：notify 监听 + reconcile，事件与游标写本地 Spool。
fn scanner_loop(
    cfg: AgentConfig,
    mut spool: Spool,
    identity: ScanIdentity,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    use notify::{RecommendedWatcher, RecursiveMode, Watcher};

    let scanner = Scanner::new(cfg.clone(), identity);
    let (tx, rx) = mpsc::channel::<notify::Event>();

    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        if let Ok(ev) = res {
            let _ = tx.send(ev);
        }
    })
    .map_err(|e| AgentError::Internal(format!("notify 初始化失败: {e}")))?;

    for root in client_roots(&cfg) {
        if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
            tracing::warn!("监听 {} 失败: {e}", root.display());
        }
    }

    let reconcile = Duration::from_secs(cfg.reconcile_interval_seconds);
    let debounce = Duration::from_millis(500); // S2.3 debounce 500ms

    // 初始扫描
    let t = scanner.scan_all(&mut spool);
    tracing::info!(
        "初始扫描: sources={} sessions={} calls={} usage={} errors={}",
        t.sources,
        t.sessions,
        t.calls,
        t.usage,
        t.errors
    );

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match rx.recv_timeout(reconcile) {
            Ok(_) => {
                // 文件变化：debounce 后增量扫描
                std::thread::sleep(debounce);
                let t = scanner.scan_all(&mut spool);
                if t.usage > 0 || t.calls > 0 {
                    tracing::debug!(
                        "增量扫描: sources={} sessions={} calls={} usage={} traffic={} errors={}",
                        t.sources,
                        t.sessions,
                        t.calls,
                        t.usage,
                        t.traffic,
                        t.errors
                    );
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Reconcile：补偿丢失的文件系统事件
                let t = scanner.scan_all(&mut spool);
                if t.sources > 0 {
                    tracing::debug!(
                        "reconcile: sources={} sessions={} calls={} usage={}",
                        t.sources,
                        t.sessions,
                        t.calls,
                        t.usage
                    );
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn client_roots(cfg: &AgentConfig) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    for c in ["claude", "claude-code", "codex", "opencode"] {
        if let Some(p) = cfg.client_root(c) {
            v.push(p);
        }
    }
    v
}

/// 心跳线程：定期心跳 + collector token 续期。
/// 无状态模式无本地 spool，统计如实报告 0。
fn heartbeat_loop(
    cfg: AgentConfig,
    client: HubClient,
    mut identity: ScanIdentity,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let mut last_register = std::time::Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // 定期重新注册，续期 Hub 侧 collector token（默认 6 天 < 7 天有效期）
        if last_register.elapsed() >= Duration::from_secs(cfg.token_refresh_interval_seconds) {
            match register(&cfg, identity.node_id.clone(), &client) {
                Ok(id) => {
                    identity = id;
                    last_register = std::time::Instant::now();
                    tracing::info!("collector token 已续期");
                }
                Err(e) => tracing::warn!("collector token 续期失败: {e}"),
            }
        }
        let req = HeartbeatRequest {
            schema_version: metria_protocol::limits::SCHEMA_VERSION,
            node_id: identity.node_id.clone(),
            collector_id: identity.collector_id.clone(),
            // 无状态轮询模式：无本地 spool，如实报告 0
            spool_pending_events: 0,
            spool_size_bytes: 0,
            source_count: 0,
            agent_clock: chrono::Utc::now(),
        };
        match client.heartbeat(&req) {
            Ok(_resp) => {}
            Err(e) => {
                tracing::debug!("心跳失败: {e}");
            }
        }
        if sleep_interruptible(&stop, Duration::from_secs(cfg.heartbeat_interval_seconds)) {
            break;
        }
    }
    Ok(())
}
