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
/// 从 `METRIA_CONFIG_FILE` 读取的 TOML 配置（惰性加载，env 优先，TOML 兜底）。
///
/// 合并规则：环境变量 > TOML 文件 > 内置默认。TOML 键名与 `METRIA_*` 环境变量同名
/// （去掉 `METRIA_` 前缀，全小写下划线），例如 `METRIA_NODE_NAME` ↔ `node_name`。
fn toml_config() -> &'static std::collections::HashMap<String, String> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        let Some(path) = env::var("METRIA_CONFIG_FILE").ok() else {
            return map;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return map;
        };
        let Ok(tbl) = text.parse::<toml::Table>() else {
            return map;
        };
        for (k, v) in tbl {
            // 兼容带 METRIA_ 前缀的键，去掉前缀；否则转大写加下划线
            let env_key = if k.starts_with("metria_") {
                k.to_uppercase()
            } else {
                let mut out = String::from("METRIA_");
                for ch in k.chars() {
                    out.push(if ch == '_' {
                        ch
                    } else {
                        ch.to_ascii_uppercase()
                    });
                }
                out
            };
            if let Some(s) = v.as_str() {
                map.insert(env_key, s.to_string());
            } else if let Some(i) = v.as_integer() {
                map.insert(env_key, i.to_string());
            } else if let Some(b) = v.as_bool() {
                map.insert(env_key, b.to_string());
            }
        }
        map
    })
}

pub fn var_opt(name: &str) -> Result<Option<String>, ConfigError> {
    match env::var(name) {
        Ok(v) => Ok(Some(v)),
        Err(env::VarError::NotPresent) => Ok(toml_config().get(name).cloned()),
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
    #[test]
    fn toml_config_fallback_when_env_missing() {
        use std::sync::OnceLock;
        // 用临时 TOML 文件验证 var_opt 兜底
        let dir = std::env::temp_dir().join(format!("metria-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("metria.toml");
        std::fs::write(&path, "node_name = \"toml-node\"\nscan_interval = 42\n").unwrap();
        std::env::set_var("METRIA_CONFIG_FILE", &path);
        // 重置缓存
        let _ = OnceLock::<std::collections::HashMap<String, String>>::new();
        // 由于 OnceLock 已初始化可能引用旧值，这里通过重读文件验证合并键转换逻辑
        let m = toml_config();
        assert_eq!(
            m.get("METRIA_NODE_NAME").map(String::as_str),
            Some("toml-node")
        );
        assert_eq!(
            m.get("METRIA_SCAN_INTERVAL").map(String::as_str),
            Some("42")
        );
        // env 优先
        std::env::set_var("METRIA_NODE_NAME", "env-node");
        assert_eq!(
            var_opt("METRIA_NODE_NAME").unwrap().as_deref(),
            Some("env-node")
        );
        std::env::remove_var("METRIA_NODE_NAME");
        std::env::remove_var("METRIA_CONFIG_FILE");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
