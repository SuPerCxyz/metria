//! metria-hub: Metria Hub 服务。
#![warn(missing_debug_implementations, rust_2018_idioms)]
#![recursion_limit = "256"]

pub mod api;
pub mod assets;
pub mod catalog;
pub mod config;
pub mod crypto;
pub mod db;
pub mod demo;
pub mod export;
pub mod http;
pub mod memory;
pub mod pull;
pub mod report;
pub mod rollup;
pub mod share;
pub mod timeseries;

use metria_core::logging::init_logging;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use crate::api::AppState;
pub use config::HubConfig;

/// 服务错误。
#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("配置错误: {0}")]
    Config(#[from] metria_core::error::ConfigError),
    #[error("存储错误: {0}")]
    Storage(#[from] metria_storage::StorageError),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 打开 Hub 数据库并应用迁移（向后兼容旧入口）。
pub fn open_database(cfg: &HubConfig) -> Result<metria_storage::rusqlite::Connection, HubError> {
    let path = cfg.sqlite_path()?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut conn = metria_storage::open(&path, &metria_storage::DbOptions::default())?;
    let applied = metria_storage::migrate_embedded(&mut conn, None)?;
    if !applied.is_empty() {
        info!(applied = ?applied, "数据库迁移完成");
    }
    Ok(conn)
}

/// 启动 Hub 服务并阻塞直至收到退出信号。
pub async fn serve(cfg: HubConfig) -> Result<(), HubError> {
    init_logging(&cfg.log_filter);
    info!(version = VERSION, listen = %cfg.listen, "Metria Hub 启动");

    if !cfg.data_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&cfg.data_dir)?;
    }

    // 打开数据库并应用迁移
    let db = db::HubDb::open(&cfg)?;
    let applied = db.apply_migrations()?;
    if !applied.is_empty() {
        info!(applied = ?applied, "数据库迁移完成");
    }

    // 内置 admin（env 注入）与内置价格目录
    ensure_admin(&db);

    // 价格目录种子 + 后台同步
    seed_catalogs(&db);
    spawn_catalog_sync(db.clone());

    // 后台维护：周期 rollup 对账 + WAL checkpoint
    spawn_maintenance(db.clone());

    // Pull 调度：对配置了 Agent 地址的节点主动拉取（无配置时空转）

    // Demo 模式：生成确定性合成数据
    if cfg.demo {
        match demo::seed_demo(&db) {
            Ok(()) => info!("Demo 数据已生成"),
            Err(e) => warn!("Demo 数据生成失败: {e}"),
        }
    }

    // 历史修复放在数据就绪之后启动，避免与 Demo 播种并发导致汇总重复计数
    spawn_integrity_repair(db.clone());

    let collector_token = std::env::var("METRIA_COLLECTOR_TOKEN").ok();
    let state = AppState {
        db,
        cfg: cfg.clone(),
        sse: api::SseHub::new(),
        sessions: Default::default(),
        collector_token,
        oidc: Default::default(),
    };

    // Pull 调度：对配置了 Agent 地址的节点主动拉取（无配置时仅空转）
    crate::pull::spawn_pull_scheduler(state.clone());

    // 报告调度：每日/每周/每月三个独立周期任务（未启用时空转）
    crate::report::scheduler::spawn_report_scheduler(state.db.clone(), state.cfg.clone());

    let app = api::app_router(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .fallback(http::static_fallback);

    let listener = TcpListener::bind(cfg.listen).await.map_err(|e| {
        error!(%e, "监听失败");
        e
    })?;
    info!("Hub 已就绪，等待请求");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| HubError::Io(std::io::Error::other(e)))?;
    info!("Hub 已退出");
    Ok(())
}

/// 新统计口径上线后的幂等历史修复；标记成功后不在每次重启重复生成估算版本。
fn spawn_integrity_repair(db: db::HubDb) {
    tokio::task::spawn_blocking(move || {
        let _heap_release = crate::memory::HeapReleaseGuard;
        const KEY: &str = "observability_integrity_version";
        const TIMING_KEY: &str = "observability_timing_repair_version";
        // v1：子 Agent 会话父级回填 + 会话汇总口径收敛（会话数只含主会话）。
        // v2：清理历史重复子代理关系（确定性关系 id 上线前的重复行）。
        const SUBAGENT_KEY: &str = "subagent_parent_repair_version";
        const SUBAGENT_VERSION: &str = "2";
        // v2-v4：Codex Token 归一化回填后，重新计价并重建 usage rollup。
        const VERSION: &str = "4";
        let full_needed = db.setting_get(KEY).ok().flatten().as_deref() != Some(VERSION);
        let timing_needed = db.setting_get(TIMING_KEY).ok().flatten().as_deref() != Some(VERSION);
        let subagent_needed =
            db.setting_get(SUBAGENT_KEY).ok().flatten().as_deref() != Some(SUBAGENT_VERSION);
        if !full_needed && !timing_needed && !subagent_needed {
            return;
        }
        let result = (|| -> Result<usize, String> {
            let timings = db.repair_legacy_call_timings().map_err(|e| e.to_string())?;
            // 回填必须在重建之前：重建按主会话口径重放会话计数
            if subagent_needed {
                let deduped = db.dedupe_subagent_relations().map_err(|e| e.to_string())?;
                if deduped > 0 {
                    info!(deduped, "重复子代理关系已清理");
                }
                let backfilled = db.backfill_subagent_parents().map_err(|e| e.to_string())?;
                info!(backfilled, "子 Agent 会话父级回填完成");
            }
            if full_needed {
                crate::catalog::reprice_from_rules(&db, false)?;
                db.rebuild_rollups(36_500).map_err(|e| e.to_string())?;
            } else if timings > 0 || subagent_needed {
                db.rebuild_rollups(36_500).map_err(|e| e.to_string())?;
            }
            if full_needed {
                db.setting_set(KEY, VERSION).map_err(|e| e.to_string())?;
            }
            if timing_needed {
                db.setting_set(TIMING_KEY, VERSION)
                    .map_err(|e| e.to_string())?;
            }
            if subagent_needed {
                db.setting_set(SUBAGENT_KEY, SUBAGENT_VERSION)
                    .map_err(|e| e.to_string())?;
            }
            Ok(timings)
        })();
        match result {
            Ok(timings) => info!(timings, "历史统计一致性修复完成"),
            Err(error) => warn!(%error, "历史统计一致性修复失败，将在下次启动重试"),
        }
    });
}

/// 按环境变量启用外部价格目录。
#[allow(clippy::type_complexity)]
fn seed_catalogs(db: &db::HubDb) {
    let now = chrono::Utc::now().to_rfc3339();
    let or_url = "https://openrouter.ai/api/v1/models".to_string();
    let litellm_url =
        "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
            .to_string();
    let custom_url = std::env::var("METRIA_PRICING_CUSTOM_URL").unwrap_or_default();
    let custom_auth = std::env::var("METRIA_PRICING_CUSTOM_AUTH").ok();
    let defs: Vec<(&str, &str, &str, i64, String, Option<String>)> = vec![
        (
            "catalog-openrouter",
            "OpenRouter 价格目录",
            "openrouter",
            30,
            or_url,
            None,
        ),
        (
            "catalog-litellm",
            "LiteLLM 价格目录",
            "litellm",
            20,
            litellm_url,
            None,
        ),
        (
            "catalog-custom",
            "自定义 HTTP 价格目录",
            "custom",
            25,
            custom_url,
            custom_auth,
        ),
    ];
    let c = db.conn();
    for (id, name, kind, priority, url, auth) in defs {
        let enabled = match kind {
            // OpenRouter 默认启用（官方公开价格目录），可用 METRIA_PRICING_OPENROUTER_ENABLED=false 关闭
            "openrouter" => std::env::var("METRIA_PRICING_OPENROUTER_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            "litellm" => std::env::var("METRIA_PRICING_LITELLM_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            _ => !url.is_empty(),
        };
        if !enabled {
            continue;
        }
        let _ = c.execute(
            "INSERT OR IGNORE INTO pricing_catalogs (id, name, kind, enabled, base_url, authentication_type, refresh_interval_seconds, priority, created_at, updated_at) VALUES (?1,?2,?3,1,?4,?5,86400,?6,?7,?7)",
            metria_storage::rusqlite::params![id, name, kind, url, auth, priority, now],
        );
    }
}

/// 后台周期同步外部价格目录（失败保留旧快照，不影响 Hub 运行）。
fn sync_catalog_cycle(db: db::HubDb) {
    let _heap_release = crate::memory::HeapReleaseGuard;
    let catalogs = catalog::catalogs_from_db(&db);
    for cat in &catalogs {
        match catalog::sync_catalog(&db, cat) {
            Ok(r) => {
                if r.fetched {
                    info!("价格目录 {} 同步完成（{} 条规则）", cat.name, r.rules);
                }
            }
            Err(e) => {
                let _ = db.mark_catalog_error(&cat.id, &e);
                warn!("价格目录 {} 同步失败（使用旧快照）: {e}", cat.name);
            }
        }
    }
    // 每轮按最新目录增量重新计价（未计价事件），并重建费用 rollup，
    // 使新上传的调用也能按 OpenRouter 官方价格计算费用。
    match catalog::reprice_from_rules(&db, true) {
        Ok(n) => {
            if n > 0 {
                info!("增量重新计价完成（{} 条）", n);
            }
        }
        Err(e) => warn!("增量重新计价失败: {e}"),
    }
}

fn spawn_catalog_sync(db: db::HubDb) {
    tokio::spawn(async move {
        // 延迟启动，避免阻塞启动路径
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        loop {
            let cycle_db = db.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || sync_catalog_cycle(cycle_db)).await
            {
                warn!("价格目录后台任务异常: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

/// 后台维护任务：周期 rollup 对账 + WAL checkpoint（§9 要求）。
///
/// - 每 6 小时对最近 24h 的 rollup 做一次对账，发现漂移则重建最近 24h。
/// - 每 6 小时执行 `wal_checkpoint(TRUNCATE)` 回收 WAL，控制磁盘占用。
fn spawn_maintenance(db: db::HubDb) {
    tokio::spawn(async move {
        // 延迟启动，避免阻塞启动路径
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
        loop {
            tick.tick().await;
            // rollup 对账（最近 24h）
            match db.reconcile_rollups(1) {
                Ok(report) => {
                    if report.drift_buckets > 0 {
                        warn!(
                            buckets = report.buckets,
                            drift = report.drift_buckets,
                            "rollup 对账发现漂移，触发重建"
                        );
                        // 与启动全量重建互斥：已有重建在执行时跳过本轮，避免互相删写。
                        match db.try_rebuild_rollups(1) {
                            Ok(Some(rebuilt)) => info!("rollup 重建完成: {rebuilt} 条"),
                            Ok(None) => info!("已有 rollup 重建在执行，跳过本轮"),
                            Err(e) => warn!("rollup 重建失败: {e}"),
                        }
                    }
                }
                Err(e) => warn!("rollup 对账失败: {e}"),
            }
            // WAL checkpoint + 空间回收
            if let Err(e) = db.wal_checkpoint() {
                warn!("WAL checkpoint 失败: {e}");
            }
            if let Err(e) = db.incremental_vacuum() {
                warn!("incremental_vacuum 失败: {e}");
            }
        }
    });
}

fn ensure_admin(db: &db::HubDb) {
    let user = std::env::var("METRIA_ADMIN_USER").unwrap_or_else(|_| "admin".into());
    let pass = std::env::var("METRIA_ADMIN_PASSWORD").unwrap_or_else(|_| "change-me-please".into());
    // argon2 哈希（与 api::verify_password 配套）；旧库 prehash 兼容校验
    let hash = api::hash_password(&pass);
    let now = chrono::Utc::now().to_rfc3339();
    let c = db.conn();
    let _ = c.execute(
        "INSERT OR IGNORE INTO users (id, username, password_hash, must_change_password, role, created_at, updated_at) VALUES (?1, ?2, ?3, 1, 'admin', ?4, ?4)",
        metria_storage::rusqlite::params![format!("user-{user}"), user, hash, now],
    );
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("收到退出信号，正在关闭");
}

/// 健康检查入口（容器使用）：打开数据库并检查 schema。
pub fn healthcheck(cfg: &HubConfig) -> Result<(), HubError> {
    let db = db::HubDb::open(cfg)?;
    db.schema_version()?;
    Ok(())
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
