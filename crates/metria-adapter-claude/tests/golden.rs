//! Claude Code Adapter golden / malformed / cursor 测试。

use std::path::PathBuf;

use metria_adapter_api::testutil::{assert_golden_basics, scan_fixture};
use metria_adapter_api::{ScanIdentity, SourceAdapter};

use metria_adapter_claude::ClaudeCodeAdapter;

fn ts(value: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&chrono::Utc)
}

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

    // Claude Code JSONL 只有每行 timestamp、没有请求发起时间 → 拿不到 per-message 起点：
    // 时长诚实置空（不回退成 turn 跨度）；tool_result 不是新 turn 起点。
    let durations: Vec<Option<i64>> = s.batch.model_calls.iter().map(|c| c.duration_ms).collect();
    assert_eq!(durations, vec![None, None, None]);
    for c in &s.batch.model_calls {
        assert_eq!(c.started_at, ts("2026-08-05T01:00:01Z"));
        assert_eq!(c.first_response_at, None);
        assert!(c
            .completed_at
            .is_some_and(|completed| completed > c.started_at));
        assert_eq!(
            c.timing_source.as_deref(),
            Some("claude_message_timestamps")
        );
        assert_eq!(c.timing_quality.as_deref(), Some("unavailable"));
        assert_eq!(c.status, "success");
        assert_eq!(c.status_code, Some(200));
    }

    // 普通 Agent 不再生成估算流量；实时字节只来自原生临时观测。
    assert!(s.batch.traffic_estimates.is_empty());
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

#[test]
fn assistant_without_real_user_has_no_latency_sample() {
    let dir = std::env::temp_dir().join(format!(
        "metria-claude-no-user-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("no-user.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"assistant","sessionId":"no-user","timestamp":"2026-09-01T00:00:02Z","message":{"id":"a1","role":"assistant","model":"claude-test","usage":{"input_tokens":10,"output_tokens":2},"content":[{"type":"text","text":"done"}]}}"#,
            "\n"
        ),
    )
    .unwrap();

    let adapter = ClaudeCodeAdapter;
    let context = metria_adapter_api::DiscoveryContext {
        node_id: "test-node".into(),
        collector_id: "test-collector".into(),
        root_paths: vec![dir.clone()],
    };
    let source = adapter
        .discover(&context)
        .unwrap()
        .into_iter()
        .find(|source| source.canonical_path == path)
        .unwrap();
    let batch = adapter.scan(&source, None, &ScanIdentity::test()).unwrap();
    let call = &batch.model_calls[0];
    assert_eq!(call.first_response_at, None);
    assert_eq!(call.duration_ms, None);
    assert_eq!(call.started_at, call.completed_at.unwrap());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn incremental_assistant_restores_real_user_start() {
    use std::io::Write;

    let dir = std::env::temp_dir().join(format!(
        "metria-claude-timing-incremental-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("incremental.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"user","sessionId":"incremental","timestamp":"2026-09-01T00:00:01Z","message":{"id":"u1","role":"user","content":"go"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let adapter = ClaudeCodeAdapter;
    let context = metria_adapter_api::DiscoveryContext {
        node_id: "test-node".into(),
        collector_id: "test-collector".into(),
        root_paths: vec![dir.clone()],
    };
    let source = adapter
        .discover(&context)
        .unwrap()
        .into_iter()
        .find(|source| source.canonical_path == path)
        .unwrap();
    let identity = ScanIdentity::test();
    let first = adapter.scan(&source, None, &identity).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","sessionId":"incremental","timestamp":"2026-09-01T00:00:04Z","message":{{"id":"a1","role":"assistant","model":"claude-test","usage":{{"input_tokens":10,"output_tokens":2}},"content":[{{"type":"text","text":"done"}}]}}}}"#
    )
    .unwrap();
    drop(file);

    let second = adapter
        .scan(&source, first.next_cursor.as_ref(), &identity)
        .unwrap();
    let call = &second.model_calls[0];
    assert_eq!(call.started_at, ts("2026-09-01T00:00:01Z"));
    assert_eq!(call.completed_at, Some(ts("2026-09-01T00:00:04Z")));
    // 无请求发起时间 → 时长不可得，诚实置空并标注不可用（回合起点仍可得，故 started_at 保留）。
    assert_eq!(call.duration_ms, None);
    assert_eq!(call.timing_quality.as_deref(), Some("unavailable"));
    assert_eq!(call.first_response_at, None);

    let _ = std::fs::remove_dir_all(dir);
}

/// 快照恢复与回溯恢复必须为增量 assistant 调用给出一致的回合起点。
#[test]
fn snapshot_restore_matches_lookback() {
    use std::io::Write;

    let dir = std::env::temp_dir().join(format!(
        "metria-claude-snapshot-eq-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("snapshot.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"user","sessionId":"snap","timestamp":"2026-09-02T00:00:01Z","message":{"id":"u1","role":"user","content":"go"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let adapter = ClaudeCodeAdapter;
    let context = metria_adapter_api::DiscoveryContext {
        node_id: "test-node".into(),
        collector_id: "test-collector".into(),
        root_paths: vec![dir.clone()],
    };
    let source = adapter
        .discover(&context)
        .unwrap()
        .into_iter()
        .find(|source| source.canonical_path == path)
        .unwrap();
    let identity = ScanIdentity::test();
    let first = adapter.scan(&source, None, &identity).unwrap();
    match first.next_cursor.as_ref().unwrap() {
        metria_core::model::SourceCursor::Jsonl(c) => {
            assert!(c.adapter_state.is_some(), "游标应携带解析上下文快照")
        }
        _ => panic!("expected jsonl cursor"),
    }

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","sessionId":"snap","timestamp":"2026-09-02T00:00:08Z","message":{{"id":"a1","role":"assistant","model":"claude-test","usage":{{"input_tokens":10,"output_tokens":2}},"content":[{{"type":"text","text":"done"}}]}}}}"#
    )
    .unwrap();
    drop(file);

    // 路径 A：Hub JSON 往返后使用快照
    let cursor_json = serde_json::to_string(first.next_cursor.as_ref().unwrap()).unwrap();
    let roundtrip: metria_core::model::SourceCursor = serde_json::from_str(&cursor_json).unwrap();
    let with_snapshot = adapter.scan(&source, Some(&roundtrip), &identity).unwrap();

    // 路径 B：去掉快照强制回溯
    let mut no_state = roundtrip.clone();
    match &mut no_state {
        metria_core::model::SourceCursor::Jsonl(c) => c.adapter_state = None,
        _ => unreachable!(),
    }
    let with_lookback = adapter.scan(&source, Some(&no_state), &identity).unwrap();

    assert_eq!(with_snapshot.model_calls.len(), 1);
    assert_eq!(with_lookback.model_calls.len(), 1);
    assert_eq!(
        with_snapshot.model_calls[0].started_at,
        with_lookback.model_calls[0].started_at
    );
    assert_eq!(
        with_snapshot.model_calls[0].started_at,
        ts("2026-09-02T00:00:01Z")
    );
    assert_eq!(with_snapshot.messages.len(), with_lookback.messages.len());

    let _ = std::fs::remove_dir_all(dir);
}

/// 未变化的 JSONL 不应被重新读取：文件不可读时扫描仍成功且为空批次。
#[cfg(unix)]
#[test]
fn unchanged_file_is_not_reread() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!(
        "metria-claude-unchanged-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("unchanged.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"user","sessionId":"unchanged","timestamp":"2026-09-03T00:00:01Z","message":{"id":"u1","role":"user","content":"go"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let adapter = ClaudeCodeAdapter;
    let context = metria_adapter_api::DiscoveryContext {
        node_id: "test-node".into(),
        collector_id: "test-collector".into(),
        root_paths: vec![dir.clone()],
    };
    let source = adapter
        .discover(&context)
        .unwrap()
        .into_iter()
        .find(|source| source.canonical_path == path)
        .unwrap();
    let identity = ScanIdentity::test();
    let first = adapter.scan(&source, None, &identity).unwrap();
    let cursor = first.next_cursor.unwrap();
    let before = match &cursor {
        metria_core::model::SourceCursor::Jsonl(c) => c.clone(),
        _ => panic!("expected jsonl cursor"),
    };

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let second = adapter.scan(&source, Some(&cursor), &identity).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(
        second.messages.is_empty() && second.model_calls.is_empty(),
        "未变化文件不应产生事件"
    );
    let after = match second.next_cursor.as_ref().unwrap() {
        metria_core::model::SourceCursor::Jsonl(c) => c,
        _ => panic!("expected jsonl cursor"),
    };
    assert_eq!(after.byte_offset, before.byte_offset);
    assert_eq!(after.adapter_state, before.adapter_state);

    let _ = std::fs::remove_dir_all(dir);
}
