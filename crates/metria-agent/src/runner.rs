//! Agent 主循环：注册 → 扫描/监听 → 上传 → 心跳。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use metria_adapter_api::ScanIdentity;
use metria_core::model::EventId;
use metria_protocol::{HeartbeatRequest, RegisterRequest, UploadBatch};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::config::AgentConfig;
use crate::error::{AgentError, Result};
use crate::scanner::Scanner;
use crate::spool::Spool;
use crate::wire::HubClient;

/// 运行 Agent（阻塞直至退出信号）。
pub fn run(cfg: AgentConfig) -> Result<()> {
    std::fs::create_dir_all(&cfg.data_dir)?;
    let mut spool = Spool::open(
        &cfg.data_dir.join("spool.db"),
        cfg.max_pending_events,
        cfg.max_spool_bytes,
    )?;

    // Node ID：显式 > 持久化 > 由 Node Name 生成
    let node_id = resolve_node_id(&cfg, &mut spool)?;
    let token = crate::config::resolve_token(&cfg);

    let client = HubClient::new(&cfg.hub_url, token);
    let identity = register(&cfg, node_id, &client)?;
    // 持久化 collector_id 便于重启复用
    let _ = spool.meta_set("collector_id", &identity.collector_id);
    tracing::info!(node = %identity.node_id, collector = %identity.collector_id, "Agent 注册完成");

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    std::thread::spawn(move || {
        // ctrlc crate 同时处理 SIGINT 与 SIGTERM（cargo 特性 signal-hook）
        let _ = ctrlc::set_handler(move || {
            tracing::info!("收到退出信号（SIGINT/SIGTERM）");
            stop_thread.store(true, Ordering::Relaxed);
        });
    });

    // 扫描线程
    let scan_cfg = cfg.clone();
    let scan_spool = reopen_spool(&cfg)?;
    let scan_identity = identity.clone();
    let scan_stop = stop.clone();
    std::thread::spawn(move || {
        if let Err(e) = scanner_loop(scan_cfg, scan_spool, scan_identity, scan_stop) {
            tracing::error!("扫描线程退出: {e}");
        }
    });

    // 上传线程
    let up_cfg = cfg.clone();
    let up_spool = reopen_spool(&cfg)?;
    let up_client = client.clone();
    let up_stop = stop.clone();
    std::thread::spawn(move || {
        if let Err(e) = uploader_loop(up_cfg, up_spool, up_client, up_stop) {
            tracing::error!("上传线程退出: {e}");
        }
    });

    // 心跳线程
    let hb_cfg = cfg.clone();
    let hb_spool = reopen_spool(&cfg)?;
    let hb_client = client;
    let hb_stop = stop.clone();
    std::thread::spawn(move || {
        if let Err(e) = heartbeat_loop(hb_cfg, hb_spool, hb_client, identity, hb_stop) {
            tracing::error!("心跳线程退出: {e}");
        }
    });

    // 主线程等待退出信号
    loop {
        if stop.load(Ordering::Relaxed) {
            tracing::info!("Agent 退出，等待子线程收尾");
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    // 优雅停止：给各线程一个心跳周期的收尾时间（冲刷 in-flight 批次）
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

/// 重新打开同一 Spool（各线程独立连接）。
fn reopen_spool(cfg: &AgentConfig) -> Result<Spool> {
    Spool::open(
        &cfg.data_dir.join("spool.db"),
        cfg.max_pending_events,
        cfg.max_spool_bytes,
    )
}

fn resolve_node_id(cfg: &AgentConfig, spool: &mut Spool) -> Result<String> {
    // 优先级：显式 METRIA_NODE_ID > 持久化 > 由 Node Name 生成
    if !cfg.node_id.trim().is_empty() {
        return Ok(cfg.node_id.trim().to_string());
    }
    if let Some(id) = spool.meta_get("node_id") {
        return Ok(id);
    }
    let gen = EventId::from_content(&format!("node:{}", cfg.node_name));
    let id = format!("node-{}", &gen.as_str()[7..15]);
    spool.meta_set("node_id", &id)?;
    Ok(id)
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

fn scanner_loop(
    cfg: AgentConfig,
    mut spool: Spool,
    identity: ScanIdentity,
    stop: Arc<AtomicBool>,
) -> Result<()> {
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

fn client_roots(cfg: &AgentConfig) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for c in ["claude", "claude-code", "codex", "opencode"] {
        if let Some(p) = cfg.client_root(c) {
            v.push(p);
        }
    }
    v
}

fn uploader_loop(
    cfg: AgentConfig,
    mut spool: Spool,
    client: HubClient,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let mut backoff = Duration::from_secs(5);
    let max_backoff = Duration::from_secs(300);
    // 指数退避 + 抖动（S2.4）：避免多 Agent 同步冲击
    let jittered = |b: Duration| {
        let jitter = rand::Rng::gen_range(&mut rand::thread_rng(), 0.0..=0.5);
        b + Duration::from_secs_f64(b.as_secs_f64() * jitter)
    };
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if !spool.full_flag().is_full() {
            let (batch_id, events) = spool.next_batch(cfg.batch_max_events, cfg.batch_max_bytes);
            if !events.is_empty() {
                let batch = UploadBatch {
                    schema_version: metria_protocol::limits::SCHEMA_VERSION,
                    batch_id: batch_id.clone(),
                    node_id: "".into(), // 由 hub 从事件校验；此处占位
                    collector_id: "".into(),
                    agent_version: metria_core::VERSION.to_string(),
                    events: events
                        .iter()
                        .map(|e| metria_protocol::BatchEvent {
                            kind: e.kind.clone(),
                            event_id: e.event_id.clone(),
                            payload: e.payload.clone(),
                        })
                        .collect(),
                };
                match client.upload(&batch) {
                    Ok(resp) => {
                        let mut to_ack = Vec::new();
                        to_ack.extend(resp.accepted.iter().cloned());
                        to_ack.extend(resp.duplicate.iter().cloned());
                        if !to_ack.is_empty() {
                            spool.ack_uploaded(&batch_id, &to_ack)?;
                        }
                        let retryable: Vec<String> = resp
                            .failed
                            .iter()
                            .filter(|f| f.retryable)
                            .map(|f| f.event_id.clone())
                            .collect();
                        let fatal: Vec<String> = resp
                            .failed
                            .iter()
                            .filter(|f| !f.retryable)
                            .map(|f| f.event_id.clone())
                            .collect();
                        if !retryable.is_empty() {
                            spool.fail_events(&batch_id, &retryable, true, "hub 重试")?;
                        }
                        if !fatal.is_empty() {
                            spool.fail_events(&batch_id, &fatal, false, "hub 拒绝")?;
                        }
                        backoff = Duration::from_secs(5);
                    }
                    Err(e) => {
                        tracing::warn!("上传失败（将重试）: {e}");
                        std::thread::sleep(jittered(backoff));
                        backoff = (backoff * 2).min(max_backoff);
                    }
                }
            }
        } else {
            tracing::warn!("Spool 满，暂停上传与采集");
        }
        std::thread::sleep(Duration::from_secs(cfg.upload_interval_seconds));
    }
    Ok(())
}

fn heartbeat_loop(
    cfg: AgentConfig,
    spool: Spool,
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
            spool_pending_events: spool.pending_count(),
            spool_size_bytes: spool.spool_bytes(),
            source_count: 0,
            agent_clock: chrono::Utc::now(),
        };
        match client.heartbeat(&req) {
            Ok(_resp) => {}
            Err(e) => {
                tracing::debug!("心跳失败: {e}");
            }
        }
        std::thread::sleep(Duration::from_secs(cfg.heartbeat_interval_seconds));
    }
    Ok(())
}
