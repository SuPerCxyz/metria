//! Claude Code Adapter golden / malformed / cursor 测试。

use std::path::PathBuf;

use metria_adapter_api::testutil::{assert_golden_basics, scan_fixture};
use metria_adapter_api::{ScanIdentity, SourceAdapter};

use metria_adapter_claude::ClaudeCodeAdapter;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude")
}

fn golden_source(adapter: &ClaudeCodeAdapter) -> metria_adapter_api::DiscoveredSource {
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
    let adapter = ClaudeCodeAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "golden_full.jsonl");
    assert_golden_basics(&s);
    assert_eq!(s.batch.sessions.len(), 1);

    let session = &s.batch.sessions[0];
    assert_eq!(
        session.source_session_id,
        "e0f1a2b3-1111-4a5b-8c9d-000000000001"
    );
    assert_eq!(session.model_call_count, 3);
    assert_eq!(session.message_count, 7);
    assert_eq!(session.tool_call_count, 2);
    assert_eq!(session.input_tokens.unwrap(), 101200);
    assert_eq!(session.output_tokens.unwrap(), 2720);
    assert_eq!(session.cache_read_tokens.unwrap(), 65000);
    assert_eq!(session.cache_write_tokens.unwrap(), 1000);
    assert_eq!(
        session.primary_model_raw.as_deref(),
        Some("claude-sonnet-4-5")
    );
    assert_eq!(
        session.primary_model_normalized.as_deref(),
        Some("claude-sonnet-4.5")
    );
    assert!(session.title.is_some(), "summary 应作为标题");

    assert_eq!(s.batch.model_calls.len(), 3);
    assert_eq!(s.batch.usage_events.len(), 3);
    assert_eq!(s.batch.tool_events.len(), 2);
    assert_eq!(s.batch.messages.len(), 7);

    // 流量估算：每条调用都有估算（部分重建 / token profile）
    assert_eq!(s.batch.traffic_estimates.len(), 3);
    for te in &s.batch.traffic_estimates {
        assert!(te.estimated_total_wire_bytes.is_some(), "应产生估算流量");
        let (lo, mid, hi) = (
            te.lower_bound_bytes.unwrap(),
            te.estimated_total_wire_bytes.unwrap(),
            te.upper_bound_bytes.unwrap(),
        );
        assert!(lo < mid && mid < hi, "禁止下界=中值=上界");
        assert!(te.confidence.is_some());
    }
    assert!(s.new_cursor.is_some());
}

#[test]
fn golden_full_cursor_is_incremental() {
    let adapter = ClaudeCodeAdapter;
    let first = scan_fixture(&adapter, &fixture_dir(), "golden_full.jsonl");
    assert_eq!(first.batch.usage_events.len(), 3);

    let source = golden_source(&adapter);
    let identity = ScanIdentity::test();
    // 从游标继续扫描：无新增
    let second = adapter
        .scan(&source, first.new_cursor.as_ref(), &identity)
        .unwrap();
    assert!(second.usage_events.is_empty());
    assert!(second.sessions.is_empty());
    assert!(second.messages.is_empty());
}

#[test]
fn missing_usage_yields_no_calls() {
    let adapter = ClaudeCodeAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "missing_usage.jsonl");
    // 无 usage → 无 model call / usage event，但仍解析消息
    assert!(s.batch.usage_events.is_empty());
    assert!(s.batch.model_calls.is_empty());
    assert_eq!(s.batch.sessions.len(), 1);
    assert_eq!(s.batch.messages.len(), 4);
}

#[test]
fn malformed_tolerated() {
    let adapter = ClaudeCodeAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "malformed.jsonl");
    assert!(!s.batch.warnings.is_empty(), "应有解析警告");
    assert!(!s.batch.usage_events.is_empty(), "正常记录仍应解析");
    assert!(s.batch.usage_events.len() >= 2);
    assert_eq!(s.batch.tool_events.len(), 1);
    assert_eq!(s.batch.sessions.len(), 1);
}

#[test]
fn truncated_tail_not_consumed() {
    let adapter = ClaudeCodeAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "truncated_tail.jsonl");
    if let Some(metria_core::model::SourceCursor::Jsonl(c)) = &s.new_cursor {
        assert_eq!(c.byte_offset, 0, "未完成的末尾行不应被消费");
    } else {
        panic!("应有 jsonl 游标");
    }
    assert!(s.batch.sessions.is_empty());
}

#[test]
fn oversized_line_skipped() {
    let adapter = ClaudeCodeAdapter;
    // 在临时目录生成超长行夹具（避免向 git 提交超大文件）
    let dir = std::env::temp_dir().join(format!("metria-claude-ov-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.jsonl");
    let mut f = std::fs::File::create(&path).unwrap();
    use std::io::Write;
    f.write_all(
        b"{\"type\":\"user\",\"sessionId\":\"big-1\",\"timestamp\":\"2026-08-05T03:00:01.000Z\",\"message\":{\"role\":\"user\",\"content\":\"ok\"}}\n",
    )
    .unwrap();
    // 2MB 行（超过 16KB 测试上限由 MAX_LINE 控制；这里超过 64KB 即触发跳过逻辑）
    let _ = writeln!(f, "{{\"x\":\"{}\"}}", "y".repeat(3 * 1024 * 1024));
    drop(f);

    let ctx = metria_adapter_api::DiscoveryContext {
        node_id: "test-node".into(),
        collector_id: "test-collector".into(),
        root_paths: vec![dir.clone()],
    };
    let discovered = adapter.discover(&ctx).unwrap();
    let source = discovered
        .iter()
        .find(|d| d.canonical_path.ends_with("big.jsonl"))
        .cloned()
        .expect("发现 big.jsonl");
    let batch = adapter.scan(&source, None, &ScanIdentity::test()).unwrap();
    assert!(!batch.warnings.is_empty(), "超长行应产生警告");
    assert_eq!(batch.sessions.len(), 1, "第一条正常行仍解析");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn non_utf8_line_skipped() {
    let adapter = ClaudeCodeAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "non_utf8.jsonl");
    assert!(!s.batch.warnings.is_empty(), "非 UTF-8 应产生警告");
}

#[test]
fn identity_flow_into_events() {
    let adapter = ClaudeCodeAdapter;
    let source = golden_source(&adapter);
    let identity = ScanIdentity {
        node_id: "node-99".into(),
        collector_id: "collector-99".into(),
    };
    let batch = adapter.scan(&source, None, &identity).expect("scan 应成功");
    assert!(!batch.sessions.is_empty());
    for s in &batch.sessions {
        assert_eq!(s.node_id, "node-99");
    }
    for u in &batch.usage_events {
        assert_eq!(u.node_id, "node-99");
    }
}

#[test]
fn task_tool_use_emits_subagent_relation() {
    let adapter = ClaudeCodeAdapter;
    let s = scan_fixture(&adapter, &fixture_dir(), "subagent.jsonl");
    assert_eq!(s.batch.sessions.len(), 1);
    let session = &s.batch.sessions[0];
    assert_eq!(session.subagent_count, 1, "Task tool_use 应计数子代理");
    assert_eq!(s.batch.subagent_relations.len(), 1);
    let rel = &s.batch.subagent_relations[0];
    assert_eq!(rel.relation, "task");
    assert_eq!(rel.child_session_id.as_str(), "sub-child-0002");
    assert_eq!(rel.session_id, session.id);
    assert_eq!(session.model_call_count, 1);
}
