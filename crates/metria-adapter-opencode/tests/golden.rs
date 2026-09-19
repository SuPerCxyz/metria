//! OpenCode Adapter golden / cursor / schema drift 测试。

use std::path::{Path, PathBuf};

use metria_adapter_api::testutil::{assert_golden_basics, scan_source};
use metria_adapter_api::{DiscoveredSource, ScanIdentity, SourceAdapter};
use metria_adapter_opencode::OpenCodeAdapter;
use metria_storage::rusqlite::Connection;

fn ts(ms: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(ms).unwrap()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("metria-opencode-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 创建与真实 opencode.db 一致的 schema。
fn create_schema(conn: &Connection) {
    conn.execute_batch(
        r#"
        CREATE TABLE session (
          id TEXT PRIMARY KEY, project_id TEXT NOT NULL, workspace_id TEXT, parent_id TEXT,
          slug TEXT NOT NULL, directory TEXT NOT NULL, path TEXT, title TEXT NOT NULL,
          version TEXT NOT NULL, share_url TEXT, metadata TEXT, cost REAL DEFAULT 0 NOT NULL,
          tokens_input INTEGER DEFAULT 0, tokens_output INTEGER DEFAULT 0,
          tokens_reasoning INTEGER DEFAULT 0, tokens_cache_read INTEGER DEFAULT 0,
          tokens_cache_write INTEGER DEFAULT 0, agent TEXT, model TEXT,
          time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL
        );
        CREATE TABLE message (
          id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL,
          time_updated INTEGER NOT NULL, data TEXT NOT NULL
        );
        CREATE TABLE part (
          id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL,
          time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL
        );
        CREATE TABLE project (
          id TEXT PRIMARY KEY, worktree TEXT NOT NULL, vcs TEXT, name TEXT,
          time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL
        );
        "#,
    )
    .unwrap();
}

fn insert_golden_data(conn: &Connection) {
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, slug, directory, title, version, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, agent, model, time_created, time_updated) VALUES \
         ('s1','global',NULL,'s1','/home/alice/projects/m', '修复登录 bug', '1.0', 0.00125, 5000, 200, 50, 1000, 0, 'build', '{\"id\":\"deepseek-v4\",\"providerID\":\"opencode\"}', 1783137427000, 1783137437000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, slug, directory, title, version, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, agent, model, time_created, time_updated) VALUES \
         ('child1','global','s1','child1','/home/alice/projects/m','子代理任务', '1.0', 0.0, 100, 10, 5, 0, 0, 'build', '{\"id\":\"deepseek-v4\",\"providerID\":\"opencode\"}', 1783137438000, 1783137440000)",
        [],
    )
    .unwrap();
    // 会话 s1 消息
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m1','s1',1783137427100,1783137427100, ?1)",
        [r#"{"role":"user","time":{"created":1783137427100},"agent":"build"}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m2','s1',1783137437000,1783137437000, ?1)",
        [r#"{"role":"assistant","agent":"build","cost":0,"tokens":{"total":35585,"input":35000,"output":200,"reasoning":50,"cache":{"write":0,"read":1000}},"modelID":"deepseek-v4","providerID":"opencode","time":{"created":1783137437000,"completed":1783137438000},"finish":"end-turn"}"#],
    )
    .unwrap();
    // parts
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('p1','m1','s1',1783137427100,1783137427100, ?1)",
        [r#"{"type":"text","text":"请修复登录 bug"}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('p2','m2','s1',1783137437100,1783137437100, ?1)",
        [r#"{"type":"text","text":"我先查看登录页代码。"}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('p3','m2','s1',1783137437200,1783137437200, ?1)",
        [r#"{"type":"tool","tool":"grep","callID":"call_01","state":{"status":"completed","input":{"pattern":"login"},"output":"Login.tsx","time":{"start":1783137437200,"end":1783137437500}}}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('p4','m2','s1',1783137437300,1783137437300, ?1)",
        [r#"{"type":"reasoning","text":"分析登录流程"}"#],
    )
    .unwrap();
    // 子代理会话至少一条消息，确保 builder 存在并可建立关系
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m3','child1',1783137439000,1783137439000, ?1)",
        [r#"{"role":"assistant","time":{"created":1783137439000,"completed":1783137440000},"modelID":"deepseek-v4","providerID":"opencode","tokens":{"total":120,"input":100,"output":10,"reasoning":5,"cache":{"write":0,"read":0}}}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('p5','m3','child1',1783137439100,1783137439100, ?1)",
        [r#"{"type":"text","text":"子代理完成"}"#],
    )
    .unwrap();
}

fn open_adapter(dir: &Path) -> (OpenCodeAdapter, DiscoveredSource) {
    let adapter = OpenCodeAdapter;
    let ctx = metria_adapter_api::DiscoveryContext {
        node_id: "test-node".into(),
        collector_id: "test-collector".into(),
        root_paths: vec![dir.to_path_buf()],
    };
    let discovered = adapter.discover(&ctx).unwrap();
    let source = discovered
        .iter()
        .find(|s| s.canonical_path.ends_with("opencode.db"))
        .cloned()
        .expect("应发现 opencode.db");
    (adapter, source)
}

#[test]
fn golden_full_reads_session_usage_tools_subagents() {
    let dir = temp_dir("golden");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema(&conn);
    insert_golden_data(&conn);
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let summary = scan_source(&adapter, &source);
    assert_golden_basics(&summary);

    let batch = &summary.batch;
    assert_eq!(batch.sessions.len(), 2);
    let s1 = batch
        .sessions
        .iter()
        .find(|s| s.source_session_id == "s1")
        .unwrap();
    assert_eq!(s1.title.as_deref(), Some("修复登录 bug"));
    assert_eq!(s1.model_call_count, 1);
    assert_eq!(s1.input_tokens.unwrap(), 35000);
    assert_eq!(s1.output_tokens.unwrap(), 200);
    assert_eq!(s1.reasoning_tokens.unwrap(), 50);
    assert_eq!(s1.cache_read_tokens.unwrap(), 1000);
    assert!(
        s1.reported_cost_micro_usd.is_some(),
        "session.cost 应转为 reported cost"
    );
    assert!(s1.working_directory_hash.is_some());
    assert_eq!(s1.primary_model_raw.as_deref(), Some("deepseek-v4"));

    // 子代理关系
    let rel = batch
        .subagent_relations
        .iter()
        .find(|r| r.relation == "subagent")
        .expect("应有 subagent 关系");
    assert_eq!(
        rel.child_session_id.as_str(),
        batch
            .sessions
            .iter()
            .find(|s| s.source_session_id == "child1")
            .unwrap()
            .id
            .as_str()
    );

    // usage + tools；普通采集不生成估算流量
    assert_eq!(batch.usage_events.len(), 2);
    assert_eq!(batch.tool_events.len(), 1);
    assert_eq!(batch.tool_events[0].name, "grep");
    assert!(batch.traffic_estimates.is_empty());

    // OpenCode assistant.created 是首个可观察输出；耗时从对应 user turn 起点计算。
    let call = batch
        .model_calls
        .iter()
        .find(|c| c.source_call_id.as_deref() == Some("m2"))
        .expect("应有 m2 的模型调用");
    assert_eq!(call.started_at, ts(1783137427100));
    assert_eq!(call.first_response_at, Some(ts(1783137437000)));
    assert_eq!(call.completed_at, Some(ts(1783137438000)));
    assert_eq!(call.duration_ms, Some(10_900));
    assert_eq!(
        call.timing_source.as_deref(),
        Some("opencode_message_timestamps")
    );
    assert_eq!(call.timing_quality.as_deref(), Some("observed"));
    assert_ne!(call.started_at, call.first_response_at.unwrap());
    assert_ne!(call.first_response_at, call.completed_at);
    assert_eq!(call.status, "success");
    assert_eq!(call.status_code, Some(200));

    // 游标增量
    let second = adapter
        .scan(&source, summary.new_cursor.as_ref(), &ScanIdentity::test())
        .unwrap();
    assert!(second.usage_events.is_empty());
}

#[test]
fn schema_drift_detected() {
    let dir = temp_dir("drift");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    // 缺 part 表 → schema 不兼容
    conn.execute_batch("CREATE TABLE session(id TEXT PRIMARY KEY, time_created INTEGER); CREATE TABLE message(id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);")
        .unwrap();
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let health = adapter.health(&source).unwrap();
    assert!(!health.ok);
    assert!(health.message.unwrap_or_default().contains("缺少必需表"));
    // 扫描也应报 schema 错误而非崩溃
    assert!(adapter.scan(&source, None, &ScanIdentity::test()).is_err());
}

#[test]
fn missing_usage_no_calls() {
    let dir = temp_dir("nousage");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema(&conn);
    conn.execute(
        "INSERT INTO session (id, project_id, slug, directory, title, version, cost, time_created, time_updated) VALUES ('s1','global','s1','/tmp/x','无 usage','1.0',0.0,1000,2000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m1','s1',1000,1000, ?1)",
        [r#"{"role":"user","time":{"created":1000}}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m2','s1',2000,2000, ?1)",
        [r#"{"role":"assistant","time":{"created":2000,"completed":2500},"modelID":"x"}"#],
    )
    .unwrap();
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let summary = scan_source(&adapter, &source);
    assert!(summary.batch.usage_events.is_empty());
    assert!(summary.batch.model_calls.is_empty());
    assert_eq!(summary.batch.sessions.len(), 1);
}

#[test]
fn db_lock_tolerated() {
    let dir = temp_dir("lock");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema(&conn);
    drop(conn);

    // 以可写方式持有独占锁（模拟来源应用正在写入）
    let lock_conn = Connection::open(&db).unwrap();
    lock_conn.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let (adapter, source) = open_adapter(&dir);
    let health = adapter.health(&source).unwrap();
    // 只读打开在 EXCLUSIVE 锁下可能失败或成功取决于 WAL 状态；不得 panic
    let _ = health;
    let _ = adapter.scan(&source, None, &ScanIdentity::test());
    lock_conn.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn error_finish_maps_to_error_status() {
    let dir = temp_dir("err-finish");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema(&conn);
    conn.execute(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('s1','global','s1','/p','err', '1.0', 1000, 2000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m1','s1',1000,1000, ?1)",
        [r#"{"role":"user","time":{"created":1000}}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m2','s1',2000,2000, ?1)",
        [r#"{"role":"assistant","time":{"created":2000,"completed":3500},"modelID":"deepseek-v4","providerID":"opencode","tokens":{"total":120,"input":100,"output":20,"cache":{"write":0,"read":0}},"finish":"error"}"#],
    )
    .unwrap();
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let summary = scan_source(&adapter, &source);
    let call = summary
        .batch
        .model_calls
        .iter()
        .find(|c| c.source_call_id.as_deref() == Some("m2"))
        .expect("应有 m2 的模型调用");
    assert_eq!(call.status, "error");
    assert_eq!(call.status_code, Some(400));
    assert_eq!(call.duration_ms, Some(2500));
}

#[test]
fn missing_assistant_created_keeps_first_response_unavailable() {
    let dir = temp_dir("missing-created");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema(&conn);
    conn.execute(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('s1','global','s1','/p','timing', '1.0', 1000, 3000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m1','s1',1000,1000, ?1)",
        [r#"{"role":"user","time":{"created":1000}}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m2','s1',2000,3000, ?1)",
        [r#"{"role":"assistant","time":{"completed":3000},"modelID":"x","tokens":{"input":10,"output":2},"finish":"end-turn"}"#],
    )
    .unwrap();
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let summary = scan_source(&adapter, &source);
    let call = &summary.batch.model_calls[0];
    assert_eq!(call.started_at, ts(1000));
    assert_eq!(call.first_response_at, None);
    assert_eq!(call.completed_at, Some(ts(3000)));
    assert_eq!(call.duration_ms, Some(2000));
}

#[test]
fn incremental_assistant_restores_corresponding_user_turn_start() {
    let dir = temp_dir("incremental-timing");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema(&conn);
    conn.execute(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('s1','global','s1','/p','timing', '1.0', 1000, 3000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m1','s1',1000,1000, ?1)",
        [r#"{"role":"user","time":{"created":1000}}"#],
    )
    .unwrap();
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let identity = ScanIdentity::test();
    let first = adapter.scan(&source, None, &identity).unwrap();

    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('m2','s1',2000,3000, ?1)",
        [r#"{"role":"assistant","time":{"created":2000,"completed":3000},"modelID":"x","tokens":{"input":10,"output":2},"finish":"end-turn"}"#],
    )
    .unwrap();
    drop(conn);

    let second = adapter
        .scan(&source, first.next_cursor.as_ref(), &identity)
        .unwrap();
    let call = &second.model_calls[0];
    assert_eq!(call.started_at, ts(1000));
    assert_eq!(call.first_response_at, Some(ts(2000)));
    assert_eq!(call.completed_at, Some(ts(3000)));
    assert_eq!(call.duration_ms, Some(2000));
}

// ===== v2（session_v2 / session_message）=====

/// v2 schema：内容内嵌在 session_message.data。
fn create_schema_v2(conn: &Connection) {
    conn.execute_batch(
        r#"
        CREATE TABLE session_v2 (
          id TEXT PRIMARY KEY, project_id TEXT NOT NULL, workspace_id TEXT, parent_id TEXT,
          fork_session_id TEXT, fork_boundary TEXT, slug TEXT, directory TEXT, path TEXT,
          title TEXT, version TEXT, share_url TEXT, summary_additions INTEGER,
          summary_deletions INTEGER, summary_files INTEGER, summary_diffs TEXT, metadata TEXT,
          cost REAL DEFAULT 0 NOT NULL, tokens_input INTEGER DEFAULT 0,
          tokens_output INTEGER DEFAULT 0, tokens_reasoning INTEGER DEFAULT 0,
          tokens_cache_read INTEGER DEFAULT 0, tokens_cache_write INTEGER DEFAULT 0,
          revert TEXT, permission TEXT, agent TEXT, model TEXT,
          time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL,
          time_compacting INTEGER, time_archived INTEGER, time_suspended INTEGER,
          resume_attempts INTEGER, time_idle INTEGER, time_viewed INTEGER, idle_outcome TEXT
        );
        CREATE TABLE session_message (
          id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL, seq INTEGER,
          time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL
        );
        "#,
    )
    .unwrap();
}

fn insert_golden_v2(conn: &Connection) {
    conn.execute(
        "INSERT INTO session_v2 (id, project_id, parent_id, slug, directory, title, version, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, model, time_created, time_updated) VALUES \
         ('v2s1','proj-hash',NULL,'v2s1','/home/alice/projects/v2','v2 会话','1.18.18',0.0025,150,60,10,500,0,'{\"id\":\"commandcode/deepseek/deepseek-v4.1-flash\",\"providerID\":\"command-code\"}',1000,6000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES ('v2u1','v2s1','user',1,1000,1000, ?1)",
        [r#"{"time":{"created":1000},"text":"帮我看看构建"}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES ('v2a1','v2s1','assistant',2,2000,6000, ?1)",
        [r#"{"time":{"created":2000,"streamed":2500,"completed":6000},"agent":"general","model":{"id":"commandcode/deepseek/deepseek-v4.1-flash","providerID":"command-code","variant":"max"},"tokens":{"input":150,"output":60,"reasoning":10,"cache":{"read":500,"write":0}},"cost":0.0025,"finish":"end-turn","content":[{"type":"reasoning","text":"先看仓库"},{"type":"text","text":"构建通过。"},{"type":"tool","id":"call_1","name":"shell","state":{"status":"completed","input":{"command":"npm build"},"output":"ok"},"time":{"created":3000,"ran":3100,"completed":4000}}]}"#],
    )
    .unwrap();
}

#[test]
fn v2_reads_session_call_usage_and_tool() {
    let dir = temp_dir("v2-golden");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema_v2(&conn);
    insert_golden_v2(&conn);
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let summary = scan_source(&adapter, &source);
    assert_golden_basics(&summary);
    let batch = &summary.batch;

    assert_eq!(batch.sessions.len(), 1);
    let session = &batch.sessions[0];
    assert_eq!(session.source_session_id, "v2s1");
    assert_eq!(session.title.as_deref(), Some("v2 会话"));
    assert_eq!(
        session.reported_cost_micro_usd,
        Some(2_500),
        "session_v2.cost 应转为 reported 微美元"
    );
    assert!(session.working_directory_hash.is_some());

    assert_eq!(batch.model_calls.len(), 1);
    let call = &batch.model_calls[0];
    assert_eq!(
        call.model_raw.as_deref(),
        Some("commandcode/deepseek/deepseek-v4.1-flash")
    );
    assert_eq!(call.provider_raw.as_deref(), Some("command-code"));
    assert_eq!(call.input_tokens, Some(150));
    assert_eq!(call.output_tokens, Some(60));
    assert_eq!(call.reasoning_tokens, Some(10));
    assert_eq!(call.cache_read_tokens, Some(500));
    assert_eq!(
        call.reported_cost_micro_usd,
        Some(2_500),
        "v2 单条消息 cost 应作为 reported cost"
    );
    assert_eq!(call.started_at, ts(1000), "回合起点来自 user 消息");
    assert_eq!(
        call.first_response_at,
        Some(ts(2500)),
        "首个可观察输出来自 time.streamed"
    );
    assert_eq!(call.completed_at, Some(ts(6000)));

    assert_eq!(batch.usage_events.len(), 1);
    assert_eq!(batch.messages.len(), 3, "user + assistant text + reasoning");
    assert_eq!(batch.tool_events.len(), 1);
    assert_eq!(batch.tool_events[0].name, "shell");
    assert_eq!(
        batch.tool_events[0].source_tool_id.as_deref(),
        Some("call_1")
    );
    assert!(batch.traffic_estimates.is_empty());
}

#[test]
fn v2_cursor_is_incremental() {
    let dir = temp_dir("v2-cursor");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema_v2(&conn);
    insert_golden_v2(&conn);
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let identity = ScanIdentity::test();
    let first = adapter.scan(&source, None, &identity).unwrap();
    assert_eq!(first.model_calls.len(), 1);

    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES ('v2u2','v2s1','user',3,7000,7000, ?1)",
        [r#"{"time":{"created":7000},"text":"再跑一次"}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES ('v2a2','v2s1','assistant',4,8000,9000, ?1)",
        [r#"{"time":{"created":8000,"streamed":8200,"completed":9000},"model":{"id":"commandcode/deepseek/deepseek-v4.1-flash","providerID":"command-code"},"tokens":{"input":10,"output":5},"finish":"end-turn","content":[{"type":"text","text":"完成"}]}"#],
    )
    .unwrap();
    drop(conn);

    let second = adapter
        .scan(&source, first.next_cursor.as_ref(), &identity)
        .unwrap();
    assert_eq!(second.model_calls.len(), 1, "只应产出新增调用");
    let call = &second.model_calls[0];
    assert_eq!(call.started_at, ts(7000));
    assert_eq!(call.first_response_at, Some(ts(8200)));
    assert_eq!(call.completed_at, Some(ts(9000)));

    let third = adapter
        .scan(&source, second.next_cursor.as_ref(), &identity)
        .unwrap();
    assert!(third.model_calls.is_empty());
}

#[test]
fn v2_migration_rescans_after_schema_change() {
    // 旧库先按 v1 扫描，再出现 v2 表：指纹变化时应从 0 重扫而不是报错卡住。
    let dir = temp_dir("v2-migration");
    let db = dir.join("opencode.db");
    let conn = Connection::open(&db).unwrap();
    create_schema(&conn);
    insert_golden_data(&conn);
    drop(conn);

    let (adapter, source) = open_adapter(&dir);
    let identity = ScanIdentity::test();
    let first = adapter.scan(&source, None, &identity).unwrap();
    assert_eq!(first.sessions.len(), 2);

    let conn = Connection::open(&db).unwrap();
    create_schema_v2(&conn);
    insert_golden_v2(&conn);
    drop(conn);

    let second = adapter
        .scan(&source, first.next_cursor.as_ref(), &identity)
        .unwrap();
    assert_eq!(second.sessions.len(), 1, "v2 会话应被扫描到");
    assert_eq!(second.model_calls.len(), 1);
    assert!(
        second.warnings.iter().any(|w| w.contains("指纹变化")),
        "应记录指纹变化并重扫：{:?}",
        second.warnings
    );
}
