//! Codex Adapter golden / malformed / cursor 测试。

use std::path::PathBuf;

use metria_adapter_api::testutil::{assert_golden_basics, scan_fixture};
use metria_adapter_api::{ScanIdentity, SourceAdapter};

use metria_adapter_codex::CodexAdapter;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex")
}

fn golden_source(adapter: &CodexAdapter) -> metria_adapter_api::DiscoveredSource {
    let ctx = metria_adapter_api::DiscoveryContext {
        node_id: "test-node".into(),
        collector_id: "test-collector".into(),
        root_paths: vec![fixture_dir()],
    };
    let all = adapter.discover(&ctx).unwrap();
    all.iter()
        .find(|d| d.canonical_path.ends_with("golden_full.jsonl"))
        .cloned()
        .expect("应发现 golden_full.jsonl")
}

#[test]
fn golden_full_parses_session_events() {
    let adapter = CodexAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "golden_full.jsonl");
    assert_golden_basics(&s);
    assert_eq!(s.batch.sessions.len(), 1);

    let session = &s.batch.sessions[0];
    assert_eq!(
        session.source_session_id,
        "019fc636-5bf9-73b3-859f-b835fe86b564"
    );
    assert_eq!(session.model_call_count, 2);
    assert_eq!(session.tool_call_count, 2);
    assert_eq!(session.input_tokens.unwrap(), 21154 + 34200);
    assert_eq!(session.output_tokens.unwrap(), 370 + 820);
    assert_eq!(session.cache_read_tokens.unwrap(), 21000);
    assert_eq!(session.cache_write_tokens.unwrap(), 1500);
    assert_eq!(session.reasoning_tokens.unwrap(), 107 + 260);
    assert!(session.working_directory_hash.is_some());
    assert!(session.content_available);

    assert_eq!(s.batch.model_calls.len(), 2);
    assert_eq!(s.batch.usage_events.len(), 2);
    assert_eq!(s.batch.tool_events.len(), 2);
    assert_eq!(s.batch.messages.len(), 6);
    assert!(!s.batch.traffic_estimates.is_empty());

    // 所有调用 reasoning 已知
    for c in &s.batch.model_calls {
        assert!(c.reasoning_tokens.is_some());
    }
}

#[test]
fn golden_full_token_count_dedup() {
    // 同一 last_token_usage 的重复 token_count 只记一次调用
    let adapter = CodexAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "golden_full.jsonl");
    assert_eq!(s.batch.model_calls.len(), 2, "重复 token_count 应去重");
}

#[test]
fn missing_usage_no_calls_but_session() {
    let adapter = CodexAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "missing_usage.jsonl");
    // 0-token 的 token_count 不产生调用（无有效 usage）
    assert!(s.batch.usage_events.is_empty());
    assert!(s.batch.model_calls.is_empty());
    assert_eq!(s.batch.sessions.len(), 1);
    assert_eq!(s.batch.messages.len(), 2);
}

#[test]
fn malformed_tolerated() {
    let adapter = CodexAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "malformed.jsonl");
    assert!(!s.batch.warnings.is_empty(), "应有解析警告");
    assert!(!s.batch.usage_events.is_empty(), "正常记录仍解析");
    assert_eq!(s.batch.tool_events.len(), 1);
    assert_eq!(s.batch.sessions.len(), 1);
}

#[test]
fn cursor_is_incremental() {
    let adapter = CodexAdapter;
    let first = scan_fixture(&adapter, &fixture_dir(), "golden_full.jsonl");
    assert_eq!(first.batch.usage_events.len(), 2);
    let source = golden_source(&adapter);
    let second = adapter
        .scan(&source, first.new_cursor.as_ref(), &ScanIdentity::test())
        .unwrap();
    assert!(second.usage_events.is_empty());
}

#[test]
fn identity_flow_into_events() {
    let adapter = CodexAdapter;
    let source = golden_source(&adapter);
    let identity = ScanIdentity {
        node_id: "node-codex-7".into(),
        collector_id: "collector-7".into(),
    };
    let batch = adapter.scan(&source, None, &identity).expect("scan 应成功");
    for s in &batch.sessions {
        assert_eq!(s.node_id, "node-codex-7");
    }
    for u in &batch.usage_events {
        assert_eq!(u.node_id, "node-codex-7");
        assert_eq!(u.adapter_id, "codex");
    }
}
