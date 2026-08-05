//! metria-hub: Metria Hub 服务。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod api;
pub mod assets;
pub mod catalog;
pub mod config;
pub mod db;
pub mod demo;
pub mod export;
pub mod http;
pub mod rollup;
pub mod share;

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

    // Demo 模式：生成确定性合成数据
    if cfg.demo {
        match demo::seed_demo(&db) {
            Ok(()) => info!("Demo 数据已生成"),
            Err(e) => warn!("Demo 数据生成失败: {e}"),
        }
    }

    let collector_token = std::env::var("METRIA_COLLECTOR_TOKEN").ok();
    let state = AppState {
        db,
        cfg: cfg.clone(),
        sse: api::SseHub::new(),
        sessions: Default::default(),
        collector_token,
    };

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
            "openrouter" => std::env::var("METRIA_PRICING_OPENROUTER_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
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
fn spawn_catalog_sync(db: db::HubDb) {
    tokio::spawn(async move {
        // 延迟启动，避免阻塞启动路径
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        loop {
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
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

fn ensure_admin(db: &db::HubDb) {
    let user = std::env::var("METRIA_ADMIN_USER").unwrap_or_else(|_| "admin".into());
    let pass = std::env::var("METRIA_ADMIN_PASSWORD").unwrap_or_else(|_| "metria-admin".into());
    let hash = format!("prehash:{}", blake3_hex(&pass));
    let now = chrono::Utc::now().to_rfc3339();
    let c = db.conn();
    let _ = c.execute(
        "INSERT OR IGNORE INTO users (id, username, password_hash, must_change_password, role, created_at, updated_at) VALUES (?1, ?2, ?3, 1, 'admin', ?4, ?4)",
        metria_storage::rusqlite::params![format!("user-{user}"), user, hash, now],
    );
}

fn blake3_hex(s: &str) -> String {
    metria_core::model::ContentHash::hash_str(s)
        .as_str()
        .to_string()
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
    db.quick_check()?;
    Ok(())
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
