//! 真机冒烟测试（默认忽略，需手动运行）：
//! `cargo test -p metria-adapter-codex --test realscan -- --ignored`
//!
//! 验证真实 Codex 目录的发现与解析，不修改任何源文件（只读）。

use std::path::PathBuf;

use metria_adapter_api::{ScanIdentity, SourceAdapter};
use metria_adapter_codex::CodexAdapter;

#[test]
#[ignore]
fn real_codex_smoke() {
    let root = std::env::var("CODEX_PATH").unwrap_or_else(|_| "/home/superc/.codex".into());
    if !PathBuf::from(&root).join("sessions").is_dir() {
        eprintln!("跳过：{root}/sessions 不存在");
        return;
    }
    let a = CodexAdapter;
    let ctx = metria_adapter_api::DiscoveryContext {
        node_id: "smoke".into(),
        collector_id: "smoke".into(),
        root_paths: vec![PathBuf::from(&root)],
    };
    let sources = a.discover(&ctx).expect("discover 失败");
    println!("发现 {} 个 codex 来源", sources.len());
    assert!(!sources.is_empty(), "真实目录应发现 rollout 文件");

    let mut total_calls = 0usize;
    let mut calls_with_first_output = 0usize;
    let mut calls_with_duration = 0usize;
    for s in sources.iter().take(2) {
        let batch = a.scan(s, None, &ScanIdentity::test()).expect("scan 失败");
        total_calls += batch.model_calls.len();
        calls_with_first_output += batch
            .model_calls
            .iter()
            .filter(|c| c.first_response_at.is_some())
            .count();
        calls_with_duration += batch
            .model_calls
            .iter()
            .filter(|c| c.duration_ms.is_some_and(|d| d > 0))
            .count();
        println!(
            "{}: sessions={} calls={} usage={} tools={} warnings={}",
            s.canonical_path.display(),
            batch.sessions.len(),
            batch.model_calls.len(),
            batch.usage_events.len(),
            batch.tool_events.len(),
            batch.warnings.len()
        );
        for warning in batch.warnings.iter().take(3) {
            println!("  warning: {warning}");
        }
    }
    assert!(total_calls > 0, "真实 rollout 应包含模型调用（usage）");
    println!(
        "OK: 共 {total_calls} 次调用，其中 {calls_with_first_output} 次有首输出、{calls_with_duration} 次时长>0"
    );
}
