//! Hub 配置。
//!
//! S0 阶段提供最小可运行配置；S2 将扩展数据库、认证、价格目录等配置项。

use std::net::SocketAddr;
use std::path::PathBuf;

use metria_core::config::{parse_timezone, var_opt, ContentMode};
use metria_core::error::ConfigError;

/// Hub 配置。
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// 监听地址，如 `0.0.0.0:8080`。
    pub listen: SocketAddr,
    /// 数据目录（SQLite 等持久化文件存放处）。
    pub data_dir: PathBuf,
    /// SQLite 数据库 URL，如 `sqlite:///data/metria.db`。
    pub database_url: String,
    /// 内容保存模式。
    pub content_mode: ContentMode,
    /// 展示与分桶时区（IANA）。
    pub timezone: chrono_tz::Tz,
    /// 日志过滤器。
    pub log_filter: String,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:8080".parse().expect("static addr"),
            data_dir: PathBuf::from("/data"),
            database_url: "sqlite:///data/metria.db".to_string(),
            content_mode: ContentMode::Metadata,
            timezone: chrono_tz::Tz::Asia__Shanghai,
            log_filter: "info".to_string(),
        }
    }
}

impl HubConfig {
    /// 从 `METRIA_*` 环境变量构建配置，未设置项使用默认值。
    pub fn from_env() -> Result<Self, ConfigError> {
        let listen = var_opt("METRIA_LISTEN")?
            .map(|v| {
                v.parse::<SocketAddr>().map_err(|_| ConfigError::Invalid {
                    name: "METRIA_LISTEN".to_string(),
                    message: format!("期望 host:port，得到 `{v}`"),
                })
            })
            .transpose()?
            .unwrap_or_else(|| "0.0.0.0:8080".parse().expect("static addr"));

        let data_dir = var_opt("METRIA_DATA_DIR")?
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/data"));

        let database_url = var_opt("METRIA_DATABASE_URL")?
            .unwrap_or_else(|| format!("sqlite://{}/metria.db", data_dir.display()));

        let content_mode = var_opt("METRIA_CONTENT_MODE")?
            .map(|v| v.parse::<ContentMode>())
            .transpose()?
            .unwrap_or_default();

        let timezone = var_opt("METRIA_TIMEZONE")?
            .map(|v| parse_timezone(&v))
            .transpose()?
            .unwrap_or(chrono_tz::Tz::Asia__Shanghai);

        let log_filter = var_opt("METRIA_LOG")?.unwrap_or_else(|| "info".to_string());

        Ok(Self {
            listen,
            data_dir,
            database_url,
            content_mode,
            timezone,
            log_filter,
        })
    }

    /// 从数据库 URL 解析 SQLite 文件路径；非 sqlite 协议返回错误。
    pub fn sqlite_path(&self) -> Result<PathBuf, ConfigError> {
        let rest =
            self.database_url
                .strip_prefix("sqlite://")
                .ok_or_else(|| ConfigError::Invalid {
                    name: "METRIA_DATABASE_URL".to_string(),
                    message: "当前仅支持 sqlite:// 协议".to_string(),
                })?;
        Ok(PathBuf::from(rest))
    }
}
