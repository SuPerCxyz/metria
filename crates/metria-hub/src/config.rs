//! Hub 配置。
//!
//! S0 阶段提供最小可运行配置；S2 将扩展数据库、认证、价格目录等配置项。

use std::net::SocketAddr;
use std::path::PathBuf;

use metria_core::config::{parse_timezone, var_opt, ContentMode};
use metria_core::error::ConfigError;

/// OIDC 单用户登录配置（Authorization Code Flow）。
///
/// 仅允许一个白名单账号（按 email 或 subject 匹配）通过 OIDC 登录，
/// 项目不支持多用户。
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// IdP Issuer URL，如 `https://idp.example.com/realms/metria`。
    pub issuer: String,
    /// OIDC Client ID。
    pub client_id: String,
    /// OIDC Client Secret。
    pub client_secret: String,
    /// 允许登录的邮箱（与 ID Token / userinfo 的 email 忽略大小写匹配）。
    pub allowed_email: Option<String>,
    /// 允许登录的 subject（与 userinfo 的 sub 精确匹配）。
    pub allowed_subject: Option<String>,
    /// 显式回调地址；默认从请求 Host / X-Forwarded-* 推导。
    pub redirect_url: Option<String>,
    /// 禁用本地密码登录（仅保留 OIDC 入口）。
    pub disable_password_login: bool,
}

impl OidcConfig {
    /// 从 `METRIA_OIDC_*` 环境变量解析；未配置返回 `Ok(None)`。
    ///
    /// 配置不完整（如设置了 issuer 但缺 client_id）或缺少白名单账号时报错。
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        let issuer = var_opt("METRIA_OIDC_ISSUER")?.unwrap_or_default();
        let client_id = var_opt("METRIA_OIDC_CLIENT_ID")?.unwrap_or_default();
        let client_secret = var_opt("METRIA_OIDC_CLIENT_SECRET")?.unwrap_or_default();
        let set = |v: &str| !v.trim().is_empty();
        let any_set = set(&issuer) || set(&client_id) || set(&client_secret);
        let all_set = set(&issuer) && set(&client_id) && set(&client_secret);
        if any_set && !all_set {
            return Err(ConfigError::Invalid {
                name: "METRIA_OIDC_*".to_string(),
                message: "OIDC 需同时设置 METRIA_OIDC_ISSUER / CLIENT_ID / CLIENT_SECRET"
                    .to_string(),
            });
        }
        if !all_set {
            return Ok(None);
        }
        let allowed_email = var_opt("METRIA_OIDC_ALLOWED_EMAIL")?
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.trim().to_string());
        let allowed_subject = var_opt("METRIA_OIDC_ALLOWED_SUBJECT")?
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.trim().to_string());
        if allowed_email.is_none() && allowed_subject.is_none() {
            return Err(ConfigError::Invalid {
                name: "METRIA_OIDC_*".to_string(),
                message:
                    "单用户模式要求设置 METRIA_OIDC_ALLOWED_EMAIL 或 METRIA_OIDC_ALLOWED_SUBJECT"
                        .to_string(),
            });
        }
        Ok(Some(Self {
            issuer: issuer.trim().trim_end_matches('/').to_string(),
            client_id: client_id.trim().to_string(),
            client_secret: client_secret.trim().to_string(),
            allowed_email,
            allowed_subject,
            redirect_url: var_opt("METRIA_OIDC_REDIRECT_URL")?
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.trim().to_string()),
            disable_password_login: var_opt("METRIA_OIDC_DISABLE_PASSWORD_LOGIN")?
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        }))
    }
}

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
    /// Demo 模式：启动时生成合成数据。
    pub demo: bool,
    /// OIDC 单用户登录；未配置则保持密码登录。
    pub oidc: Option<OidcConfig>,
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
            demo: false,
            oidc: None,
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
        let demo = var_opt("METRIA_DEMO")?
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let oidc = OidcConfig::from_env()?;

        Ok(Self {
            listen,
            data_dir,
            database_url,
            content_mode,
            timezone,
            log_filter,
            demo,
            oidc,
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
