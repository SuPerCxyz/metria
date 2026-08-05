//! metria-core: 领域模型、ID、归一化、脱敏、时间、金额与内容分类基础库。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod config;
pub mod error;
pub mod logging;

pub use config::{CommonConfig, ContentMode};
pub use error::{ConfigError, ContentError, ModelError, MoneyError, PrivacyError, TimeError};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
