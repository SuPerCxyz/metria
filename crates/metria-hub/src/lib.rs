//! metria-hub: Metria Hub 服务。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod assets;
pub mod config;
pub mod http;

use metria_core::logging::init_logging;
use metria_storage::{migrate_embedded, open, DbOptions};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info};

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

/// 打开（必要时创建）Hub 数据库并应用迁移。
pub fn open_database(cfg: &HubConfig) -> Result<metria_storage::rusqlite::Connection, HubError> {
    let path = cfg.sqlite_path()?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut conn = open(&path, &DbOptions::default())?;
    let applied = migrate_embedded(&mut conn, None)?;
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

    // 打开数据库并应用迁移（启动路径不含 rollup 重建等重活）。
    let _conn = open_database(&cfg)?;

    let app = http::app_router()
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

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
    let conn = open_database(cfg)?;
    metria_storage::quick_check(&conn)?;
    Ok(())
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
