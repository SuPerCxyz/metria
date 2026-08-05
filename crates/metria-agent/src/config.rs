//! Agent 配置。

use std::path::PathBuf;

use metria_core::config::{optional_bool, optional_int, var_opt, ContentMode};
use metria_core::error::ConfigError;

/// Agent 配置。
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub node_id: String,
    pub node_name: String,
    pub hub_url: String,
    pub token: Option<String>,
    pub claude_path: Option<PathBuf>,
    pub codex_path: Option<PathBuf>,
    pub opencode_path: Option<PathBuf>,
    pub content_mode: ContentMode,
    /// 数据目录（Spool 存放处）。
    pub data_dir: PathBuf,
    pub max_pending_events: i64,
    pub max_spool_bytes: i64,
    pub batch_max_events: usize,
    pub batch_max_bytes: usize,
    pub scan_interval_seconds: u64,
    pub reconcile_interval_seconds: u64,
    pub heartbeat_interval_seconds: u64,
    pub upload_interval_seconds: u64,
    pub log_filter: String,
}

impl AgentConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let node_name = var_opt("METRIA_NODE_NAME")?.unwrap_or_else(|| "node-01".into());
        let node_id = var_opt("METRIA_NODE_ID")?.unwrap_or_default();
        let hub_url = var_opt("METRIA_HUB_URL")?.unwrap_or_else(|| "http://localhost:8080".into());
        let token = var_opt("METRIA_AGENT_TOKEN")?.or_else(|| {
            var_opt("METRIA_AGENT_TOKEN_FILE")
                .ok()
                .flatten()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|s| s.trim().to_string())
        });

        let data_dir = var_opt("METRIA_DATA_DIR")?
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/data"));

        let content_mode = var_opt("METRIA_CONTENT_MODE")?
            .map(|v| v.parse::<ContentMode>())
            .transpose()?
            .unwrap_or_default();

        let log_filter = var_opt("METRIA_LOG")?.unwrap_or_else(|| "info".into());

        Ok(Self {
            node_id,
            node_name,
            hub_url,
            token,
            claude_path: var_opt("METRIA_CLAUDE_PATH")?.map(PathBuf::from),
            codex_path: var_opt("METRIA_CODEX_PATH")?.map(PathBuf::from),
            opencode_path: var_opt("METRIA_OPENCODE_PATH")?.map(PathBuf::from),
            content_mode,
            data_dir,
            max_pending_events: optional_int("METRIA_MAX_PENDING_EVENTS")?.unwrap_or(2_000_000),
            max_spool_bytes: optional_int("METRIA_MAX_SPOOL_BYTES")?.unwrap_or(512 * 1024 * 1024),
            batch_max_events: optional_int("METRIA_BATCH_MAX_EVENTS")?
                .unwrap_or(metria_protocol::limits::MAX_EVENTS_PER_BATCH as i64)
                as usize,
            batch_max_bytes: optional_int("METRIA_BATCH_MAX_BYTES")?.unwrap_or(1024 * 1024)
                as usize,
            scan_interval_seconds: optional_int("METRIA_SCAN_INTERVAL")?.unwrap_or(10) as u64,
            reconcile_interval_seconds: optional_int("METRIA_RECONCILE_INTERVAL")?.unwrap_or(300)
                as u64,
            heartbeat_interval_seconds: optional_int("METRIA_HEARTBEAT_INTERVAL")?.unwrap_or(60)
                as u64,
            upload_interval_seconds: optional_int("METRIA_UPLOAD_INTERVAL")?.unwrap_or(15) as u64,
            log_filter,
        })
    }

    /// 获取该客户端对应的 root 路径。
    pub fn client_root(&self, client: &str) -> Option<PathBuf> {
        match client {
            "claude" | "claude-code" => self.claude_path.clone(),
            "codex" => self.codex_path.clone(),
            "opencode" => self.opencode_path.clone(),
            _ => None,
        }
    }
}

/// 读取 Agent token（环境或文件）。
pub fn resolve_token(cfg: &AgentConfig) -> Option<String> {
    cfg.token.clone()
}

/// 调试开关（保留给后续）。
#[allow(dead_code)]
fn _debug_env() -> Result<Option<bool>, ConfigError> {
    optional_bool("METRIA_DEBUG")
}
