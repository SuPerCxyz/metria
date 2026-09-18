//! metria-agent: Metria Agent（Collector）。
//!
//! 阻塞栈：notify + rusqlite + ureq + zstd + blake3，无 tokio（控制空闲 RSS）。

#![warn(missing_debug_implementations, rust_2018_idioms)]
#![recursion_limit = "256"]

pub mod config;
pub mod error;
pub mod observer;
mod observer_metrics;
mod observer_proxy;
pub mod pullserver;
pub mod runner;
pub mod scanner;
pub mod spool;
pub mod stateless;
pub mod wire;

pub use config::AgentConfig;
pub use error::{AgentError, Result};
pub use runner::run;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
