//! metria-cli: Metria 命令行入口（二进制入口见 main.rs）。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod backup;
pub mod doctor;
pub mod import;
pub mod mcp;
pub mod registry;

/// 初始化全局日志（幂等）。供 main 调用。
pub fn init() {
    let filter = std::env::var("METRIA_LOG").unwrap_or_else(|_| "info".to_string());
    metria_core::logging::init_logging(&filter);
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
