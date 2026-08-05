//! OpenCode Adapter golden / cursor / schema drift 测试。

use std::path::{Path, PathBuf};

use metria_adapter_api::testutil::{assert_golden_basics, scan_source};
use metria_adapter_api::{DiscoveredSource, ScanIdentity, SourceAdapter};
use metria_adapter_opencode::OpenCodeAdapter;
use metria_storage::rusqlite::Connection;

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

    // usage + tools + traffic
    assert_eq!(batch.usage_events.len(), 2);
    assert_eq!(batch.tool_events.len(), 1);
    assert_eq!(batch.tool_events[0].name, "grep");
    assert!(!batch.traffic_estimates.is_empty());
    let te = &batch.traffic_estimates[0];
    assert!(te.estimated_total_wire_bytes.is_some());
    assert!(te.lower_bound_bytes.unwrap() < te.estimated_total_wire_bytes.unwrap());
    assert!(te.upper_bound_bytes.unwrap() > te.estimated_total_wire_bytes.unwrap());

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
