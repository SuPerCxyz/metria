//! Codex Adapter golden / malformed / cursor 测试。

use std::fs::OpenOptions;
use std::io::Write;
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

#[test]
fn appended_usage_restores_session_and_model_context() {
    let dir = std::env::temp_dir().join(format!(
        "codex-incremental-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rollout.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"timestamp":"2026-08-13T01:00:00Z","type":"session_meta","payload":{"session_id":"incremental-session","timestamp":"2026-08-13T01:00:00Z","cwd":"/tmp/project","model_provider":"openai"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let adapter = CodexAdapter;
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
    assert_eq!(first.sessions.len(), 1);
    assert!(first.model_calls.is_empty());

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"2026-08-13T01:01:00Z","type":"event_msg","payload":{{"type":"user_message","message":"incremental"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"2026-08-13T01:01:01Z","type":"turn_context","payload":{{"model":"gpt-5.6-sol"}}}}"#
    )
    .unwrap();
    file.flush().unwrap();

    let second = adapter
        .scan(&source, first.next_cursor.as_ref(), &identity)
        .unwrap();
    assert!(second.model_calls.is_empty());
    assert_eq!(second.messages.len(), 1);
    assert_eq!(
        second.sessions[0].title.as_deref(),
        Some("incremental"),
        "首条用户消息应作为会话标题"
    );

    writeln!(
        file,
        r#"{{"timestamp":"2026-08-13T01:01:02Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":1200,"cached_input_tokens":900,"output_tokens":80,"reasoning_output_tokens":20,"total_tokens":1280}}}}}}}}"#
    )
    .unwrap();
    drop(file);

    let third = adapter
        .scan(&source, second.next_cursor.as_ref(), &identity)
        .unwrap();
    assert_eq!(third.model_calls.len(), 1);
    assert_eq!(third.usage_events.len(), 1);
    assert_eq!(
        third.model_calls[0].session_id.as_str(),
        "incremental-session"
    );
    assert_eq!(
        third.model_calls[0].model_raw.as_deref(),
        Some("gpt-5.6-sol")
    );
    assert_eq!(third.model_calls[0].input_tokens, Some(1200));

    let fourth = adapter
        .scan(&source, third.next_cursor.as_ref(), &identity)
        .unwrap();
    assert!(fourth.model_calls.is_empty());
    assert!(fourth.usage_events.is_empty());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn incremental_context_lookback_beyond_initial_window() {
    // 增量扫描时，若游标前的 turn_context 距游标超过初始 1MiB 回看窗口
    // （长会话尾部大量 token_count 无新 turn_context），仍应回溯找到模型。
    let dir = std::env::temp_dir().join(format!(
        "codex-lookback-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rollout.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"timestamp":"2026-08-13T01:00:00Z","type":"session_meta","payload":{"session_id":"lookback-session","timestamp":"2026-08-13T01:00:00Z","cwd":"/tmp/project","model_provider":"custom"}}"#,
            "\n",
            r#"{"timestamp":"2026-08-13T01:00:01Z","type":"turn_context","payload":{"model":"gpt-5.6-sol"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let adapter = CodexAdapter;
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
    assert!(first.model_calls.is_empty());

    // 写入超过 1MiB 的填充行（不产生调用的 agent_message），把游标推远。
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    let filler = r#"{"timestamp":"2026-08-13T01:00:10Z","type":"event_msg","payload":{"type":"agent_message","message":"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}}"#;
    let mut written = 0u64;
    while written < 2 * 1024 * 1024 {
        writeln!(file, "{filler}").unwrap();
        written += filler.len() as u64 + 1;
    }
    writeln!(
        file,
        r#"{{"timestamp":"2026-08-13T01:01:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":1200,"cached_input_tokens":900,"output_tokens":80,"reasoning_output_tokens":20,"total_tokens":1280}}}}}}}}"#
    )
    .unwrap();
    drop(file);

    let second = adapter
        .scan(&source, first.next_cursor.as_ref(), &identity)
        .unwrap();
    assert_eq!(
        second.model_calls.len(),
        1,
        "游标后的 token_count 应产生一次调用"
    );
    assert_eq!(second.usage_events.len(), 1);
    assert_eq!(
        second.model_calls[0].session_id.as_str(),
        "lookback-session"
    );
    assert_eq!(
        second.model_calls[0].model_raw.as_deref(),
        Some("gpt-5.6-sol"),
        "超过 1MiB 窗口的 turn_context 也应被回溯恢复"
    );
    assert_eq!(
        second.model_calls[0].model_normalized.as_deref(),
        Some("gpt-5.6-sol")
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn title_skips_system_injection_uses_first_real_user_message() {
    // 会话开头常注入 AGENTS.md/系统指令，不应作为标题；
    // 之后的第一条真实用户消息应作为标题。
    let dir = std::env::temp_dir().join(format!(
        "codex-title-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rollout.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"timestamp\":\"2026-08-13T01:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"session_id\":\"title-session\",\"timestamp\":\"2026-08-13T01:00:00Z\",\"cwd\":\"/tmp/project\",\"model_provider\":\"custom\"}}",
            "\n",
            "{\"timestamp\":\"2026-08-13T01:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"# AGENTS.md instructions for /tmp/project\\n\\n<INSTRUCTIONS>\\n 全局规则\\n\"}]}}",
            "\n",
            "{\"timestamp\":\"2026-08-13T01:00:02Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<environment_context>\\n  <cwd>/tmp/project</cwd>\\n</environment_context>\"}]}}",
            "\n",
            "{\"timestamp\":\"2026-08-13T01:00:03Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"网页右上角的刷新按钮点击后时间没有更新\"}]}}",
            "\n"
        ),
    )
    .unwrap();

    let adapter = CodexAdapter;
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
    assert_eq!(batch.sessions.len(), 1);
    assert_eq!(
        batch.sessions[0].title.as_deref(),
        Some("网页右上角的刷新按钮点击后时间没有更新"),
        "AGENTS.md 注入应被跳过，真实用户消息作为标题"
    );

    let _ = std::fs::remove_dir_all(dir);
}
