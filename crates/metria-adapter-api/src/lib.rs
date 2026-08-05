//! metria-adapter-api: 客户端 Adapter 接口、ScanBatch、Cursor、能力与错误定义。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod error;
pub mod parse;
pub mod testutil;
pub mod types;

pub use error::{AdapterError, ScanTolerance};
pub use parse::scan_jsonl_file;
pub use types::{
    discover, pseudo_id, AdapterCapabilities, DiscoveredSource, DiscoveryContext, ScanBatch,
    ScanIdentity, SourceAdapter, SourceHealth, TrafficCapabilities,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
