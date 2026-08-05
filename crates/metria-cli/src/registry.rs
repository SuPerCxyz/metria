//! Adapter 注册表：按名称返回具体 Adapter。

use metria_adapter_api::SourceAdapter;

/// 按 canonical name 获取 Adapter。
pub fn adapter(name: &str) -> Option<Box<dyn SourceAdapter>> {
    match name {
        "claude" | "claude-code" => Some(Box::new(metria_adapter_claude::ClaudeCodeAdapter)),
        "codex" => Some(Box::new(metria_adapter_codex::CodexAdapter)),
        "opencode" => Some(Box::new(metria_adapter_opencode::OpenCodeAdapter)),
        _ => None,
    }
}

/// 全部已注册 Adapter。
pub fn all() -> Vec<Box<dyn SourceAdapter>> {
    vec![
        Box::new(metria_adapter_claude::ClaudeCodeAdapter),
        Box::new(metria_adapter_codex::CodexAdapter),
        Box::new(metria_adapter_opencode::OpenCodeAdapter),
    ]
}
