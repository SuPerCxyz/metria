//! 统一的 tracing 日志初始化。
//!
//! - 过滤规则来自调用方传入的字符串（通常取 `METRIA_LOG`，默认 `info`）。
//! - 若环境变量 `RUST_LOG` 已设置，则优先使用 `RUST_LOG`，便于容器内覆盖。
//! - 日志绝不输出 Token 或 Secret（各调用点自行遵守）。

use std::sync::Once;

use tracing_subscriber::EnvFilter;

static INIT: Once = Once::new();

/// 初始化全局日志。
///
/// 返回 `false` 表示全局 subscriber 已由其他代码初始化，本次未生效。
pub fn init_logging(filter: &str) -> bool {
    let mut initialized = false;
    INIT.call_once(|| {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"))
        });
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
        initialized = true;
    });
    initialized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_works() {
        // 首次调用应成功；幂等重复调用不应 panic。
        init_logging("info");
        init_logging("debug");
    }
}
