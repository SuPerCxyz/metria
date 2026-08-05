//! metria-core: 领域模型、ID、归一化、脱敏、时间、金额与内容分类基础库。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod config;
pub mod content;
pub mod error;
pub mod logging;
pub mod model;
pub mod money;
pub mod normalize;
pub mod privacy;
pub mod time;

pub use config::{CommonConfig, ContentMode};
pub use error::{ConfigError, ContentError, ModelError, MoneyError, PrivacyError, TimeError};
pub use money::MicroUsd;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
