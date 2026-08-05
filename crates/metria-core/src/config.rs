//! 跨 crate 共享的配置基础：内容模式、时区与通用的 `METRIA_*` 环境变量解析。
//!
//! 完整配置（AgentConfig / HubConfig）分别位于 metria-agent 与 metria-hub。

use std::env;
use std::str::FromStr;

use chrono_tz::Tz;

use crate::error::ConfigError;

/// 内容保存模式。
///
/// - `none`：只保存 Usage、Session 时间、Model、Provider、Project Hash、消息数、
///   Tool 名称、内容长度与 Traffic Estimate。
/// - `metadata`（默认）：额外保存 Session 标题、Message Hash、UTF-8 字节数、Tool 类型，不保存正文。
/// - `full`：保存完整会话正文与 Tool 内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentMode {
    #[default]
    Metadata,
    None,
    Full,
}

impl FromStr for ContentMode {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "metadata" => Ok(Self::Metadata),
            "full" => Ok(Self::Full),
            other => Err(ConfigError::Invalid {
                name: "content_mode".to_string(),
                message: format!("未知内容模式 `{other}`，可选 none|metadata|full"),
            }),
        }
    }
}

/// 跨组件共享配置。
#[derive(Debug, Clone)]
pub struct CommonConfig {
    /// IANA 时区，用于展示与时间分桶；默认 Asia/Shanghai。
    pub timezone: Tz,
    /// 内容保存模式；默认 metadata。
    pub content_mode: ContentMode,
    /// 日志过滤器（tracing EnvFilter）；默认 info。
    pub log_filter: String,
}

impl Default for CommonConfig {
    fn default() -> Self {
        Self {
            timezone: Tz::Asia__Shanghai,
            content_mode: ContentMode::Metadata,
            log_filter: "info".to_string(),
        }
    }
}

impl CommonConfig {
    /// 从 `METRIA_*` 环境变量构建配置，未设置项使用默认值。
    pub fn from_env() -> Result<Self, ConfigError> {
        let timezone = var_opt("METRIA_TIMEZONE")?
            .map(|v| parse_timezone(&v))
            .transpose()?
            .unwrap_or(Tz::Asia__Shanghai);
        let content_mode = var_opt("METRIA_CONTENT_MODE")?
            .map(|v| ContentMode::from_str(&v))
            .transpose()?
            .unwrap_or_default();
        let log_filter = var_opt("METRIA_LOG")?.unwrap_or_else(|| "info".to_string());
        Ok(Self {
            timezone,
            content_mode,
            log_filter,
        })
    }
}

/// 读取可选字符串环境变量：未设置时返回 `Ok(None)`。
pub fn var_opt(name: &str) -> Result<Option<String>, ConfigError> {
    match env::var(name) {
        Ok(v) => Ok(Some(v)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError::Invalid {
            name: name.to_string(),
            message: "环境变量包含非 UTF-8 字节".to_string(),
        }),
    }
}

/// 解析 IANA 时区字符串，如 `Asia/Shanghai`、`UTC`。
pub fn parse_timezone(s: &str) -> Result<Tz, ConfigError> {
    s.parse::<Tz>().map_err(|_| ConfigError::Invalid {
        name: "timezone".to_string(),
        message: format!("未知 IANA 时区 `{s}`"),
    })
}

/// 读取必需字符串环境变量。
pub fn require_env(name: &str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name.to_string()))
}

/// 读取可选字符串环境变量。
pub fn optional_env(name: &str) -> Option<String> {
    env::var(name).ok()
}

/// 读取可选整数环境变量。
pub fn optional_int(name: &str) -> Result<Option<i64>, ConfigError> {
    match var_opt(name)? {
        Some(v) => v
            .parse::<i64>()
            .map(Some)
            .map_err(|_| ConfigError::Invalid {
                name: name.to_string(),
                message: format!("期望整数，得到 `{v}`"),
            }),
        None => Ok(None),
    }
}

/// 读取可选布尔环境变量（true/false/1/0）。
pub fn optional_bool(name: &str) -> Result<Option<bool>, ConfigError> {
    match var_opt(name)? {
        Some(v) => match v.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(Some(true)),
            "false" | "0" | "no" => Ok(Some(false)),
            other => Err(ConfigError::Invalid {
                name: name.to_string(),
                message: format!("期望布尔值，得到 `{other}`"),
            }),
        },
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_mode_parse() {
        assert_eq!("none".parse::<ContentMode>().unwrap(), ContentMode::None);
        assert_eq!(
            "metadata".parse::<ContentMode>().unwrap(),
            ContentMode::Metadata
        );
        assert_eq!("FULL".parse::<ContentMode>().unwrap(), ContentMode::Full);
        assert!("bogus".parse::<ContentMode>().is_err());
    }

    #[test]
    fn timezone_parse() {
        assert_eq!(parse_timezone("Asia/Shanghai").unwrap(), Tz::Asia__Shanghai);
        assert_eq!(parse_timezone("UTC").unwrap(), Tz::UTC);
        assert!(parse_timezone("Mars/Olympus").is_err());
    }

    #[test]
    fn default_config() {
        let cfg = CommonConfig::default();
        assert_eq!(cfg.timezone, Tz::Asia__Shanghai);
        assert_eq!(cfg.content_mode, ContentMode::Metadata);
        assert_eq!(cfg.log_filter, "info");
    }
}
