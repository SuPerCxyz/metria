//! metria-adapter-opencode: OpenCode SQLite Adapter。
//!
//! 只读访问：SQLITE_OPEN_READ_ONLY + busy_timeout + query_only；
//! 不执行 Migration，不修改第三方数据库 PRAGMA。

#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod build;
pub mod entry;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use metria_adapter_api::types::{
    discover, AdapterCapabilities, DiscoveredSource, DiscoveryContext, ScanBatch, SourceAdapter,
    SourceHealth, TrafficCapabilities,
};
use metria_adapter_api::{pseudo_id, AdapterError, ScanTolerance};
use metria_core::model::{
    CacheTransportBehavior, ContextTransportMode, Id, ReconstructionQuality, SourceStatus,
};
use metria_storage::rusqlite::{Connection, OptionalExtension};

use build::{sqlite_cursor, BuildCtx, SessionBuilder};
use entry::{from_millis, parse_session_model, MessageData, PartData, SessionRow};

/// 单批次读取行数上限（防止单次扫描过大）。
const BATCH_LIMIT: i64 = 100_000;

/// OpenCode Adapter。
#[derive(Debug, Default, Clone)]
pub struct OpenCodeAdapter;

impl SourceAdapter for OpenCodeAdapter {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            session_usage: true,
            call_usage: true,
            turn_usage: true,
            message_usage: true,
            message_content: true,
            tool_calls: true,
            tool_results: true,
            subagents: true,
            project_path: true,
            reported_cost: true,
            model_switching: true,
            reasoning_tokens: true,
            cache_tokens: true,
            request_reconstruction: false,
            response_reconstruction: true,
            context_transport_detection: true,
        }
    }

    fn discover(&self, context: &DiscoveryContext) -> Result<Vec<DiscoveredSource>, AdapterError> {
        let mut out = Vec::new();
        for root in &context.root_paths {
            // 布局 A：全局数据库
            let global = root.join("opencode.db");
            if global.is_file() {
                out.push(discover::source(
                    "opencode",
                    global,
                    None,
                    vec!["sessions".into(), "usage".into()],
                ));
            }
            // 布局 B：project/<slug>/storage/**/*.db
            let projects = root.join("project");
            if projects.is_dir() {
                collect_dbs(&projects, &mut out)?;
            }
        }
        Ok(out)
    }

    fn scan(
        &self,
        source: &DiscoveredSource,
        cursor: Option<&metria_core::model::SourceCursor>,
        identity: &metria_adapter_api::ScanIdentity,
    ) -> Result<ScanBatch, AdapterError> {
        let path = &source.canonical_path;
        let conn = metria_storage::open_readonly(path).map_err(|e| AdapterError::NotReadable {
            path: path.display().to_string(),
            source: std::io::Error::other(e.to_string()),
        })?;

        // Schema 检查：必需表必须存在
        ensure_schema(&conn, path)?;
        let schema_signature = schema_signature(&conn);

        let (last_rowid, expected_fp) = match cursor {
            Some(metria_core::model::SourceCursor::Sqlite(c)) => {
                (c.last_rowid, Some(c.database_fingerprint.clone()))
            }
            _ => (0, None),
        };
        let fingerprint = metria_core::privacy::hash_path(&format!(
            "{}:{schema_signature}",
            source.path_hash.as_str()
        ));
        if let Some(expected) = expected_fp {
            if expected != fingerprint {
                // schema 或文件变化：从 0 重新扫描（游标失效但数据仍可读）
                return Err(AdapterError::CursorInvalid(
                    "数据库指纹变化，游标失效".into(),
                ));
            }
        }

        let ctx = BuildCtx {
            node_id: identity.node_id.clone(),
            collector_id: pseudo_id(&identity.collector_id),
            source_id: pseudo_id(source.path_hash.as_str()),
            client_id: "opencode".into(),
        };

        let mut tolerance = ScanTolerance::default();
        let mut builders: HashMap<String, SessionBuilder> = HashMap::new();
        let mut session_cache: HashMap<String, SessionRow> = HashMap::new();
        let mut max_rowid = last_rowid;

        let mut stmt = conn
            .prepare(
                "SELECT rowid, id, session_id, time_created, data FROM message \
                 WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
            )
            .map_err(|e| AdapterError::Other(format!("message 查询失败: {e}")))?;
        let rows = stmt
            .query_map(
                metria_storage::rusqlite::params![last_rowid, BATCH_LIMIT],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .map_err(|e| AdapterError::Other(format!("message 读取失败: {e}")))?;

        for row in rows {
            let (rowid, msg_id, session_id, ts_ms, data_json) =
                row.map_err(|e| AdapterError::Other(format!("message 行解析失败: {e}")))?;
            if rowid > max_rowid {
                max_rowid = rowid;
            }
            let at = from_millis(ts_ms).unwrap_or_else(Utc::now);
            let data: MessageData = match serde_json::from_str(&data_json) {
                Ok(d) => d,
                Err(e) => {
                    tolerance.record(format!("message {msg_id} data 解析失败: {e}"));
                    continue;
                }
            };

            let builder = builders.entry(session_id.clone()).or_insert_with(|| {
                let row = session_cache
                    .entry(session_id.clone())
                    .or_insert_with(|| load_session(&conn, &session_id).unwrap_or_default());
                let mut b = SessionBuilder::new(ctx.clone(), session_id.clone(), at);
                let (model, provider) = parse_session_model(row.model.as_deref());
                let cost = dollar_to_micro(row.cost);
                b.set_meta(
                    row.title.clone(),
                    row.directory.clone(),
                    row.project_id.clone(),
                    model,
                    provider,
                    cost,
                    row.parent_id.clone(),
                );
                b
            });

            process_message(&conn, builder, &msg_id, &data, at, &mut tolerance);
        }

        // 子代理关系：child.parent_source_id -> parent builder
        let parent_by_source: HashMap<String, Id> = builders
            .iter()
            .map(|(sid, b)| (sid.clone(), b.session.id.clone()))
            .collect();
        let children: Vec<(Id, Option<String>)> = builders
            .values()
            .map(|b| (b.session.id.clone(), b.parent_source_id.clone()))
            .filter(|(_, p)| p.is_some())
            .collect();
        for (child_id, parent_src) in children {
            if let Some(parent_src) = parent_src {
                if let Some(parent_id) = parent_by_source.get(&parent_src) {
                    if let Some(parent_builder) = builders.get_mut(&parent_src) {
                        parent_builder
                            .subagents
                            .push(metria_core::model::SubagentRelation {
                                id: Id::new(),
                                session_id: parent_id.clone(),
                                parent_model_call_id: None,
                                child_session_id: child_id.clone(),
                                relation: "subagent".into(),
                                created_at: Utc::now(),
                            });
                    }
                }
            }
        }

        let mut batches = ScanBatch::default();
        for (_, b) in builders {
            let (s, turns, messages, calls, usage, tools, subagents, traffic) = b.finish();
            batches.sessions.push(s);
            batches.turns.extend(turns);
            batches.messages.extend(messages);
            batches.model_calls.extend(calls);
            batches.usage_events.extend(usage);
            batches.tool_events.extend(tools);
            batches.subagent_relations.extend(subagents);
            batches.traffic_estimates.extend(traffic);
        }
        batches.warnings = tolerance.warnings;
        batches.next_cursor = Some(sqlite_cursor(
            fingerprint,
            Some(schema_signature),
            max_rowid,
        ));

        Ok(batches)
    }

    fn health(&self, source: &DiscoveredSource) -> Result<SourceHealth, AdapterError> {
        let path = &source.canonical_path;
        if !path.exists() {
            return Ok(SourceHealth {
                ok: false,
                status: SourceStatus::Missing,
                message: Some("数据库文件不存在".into()),
                last_error: None,
            });
        }
        match metria_storage::open_readonly(path) {
            Ok(conn) => match ensure_schema(&conn, path) {
                Ok(()) => Ok(SourceHealth {
                    ok: true,
                    status: SourceStatus::Active,
                    message: None,
                    last_error: None,
                }),
                Err(e) => Ok(SourceHealth {
                    ok: false,
                    status: SourceStatus::Error,
                    message: Some(e.to_string()),
                    last_error: Some(e.to_string()),
                }),
            },
            Err(e) => Ok(SourceHealth {
                ok: false,
                status: SourceStatus::Error,
                message: Some(format!("数据库不可读（可能被锁）: {e}")),
                last_error: Some(e.to_string()),
            }),
        }
    }

    fn traffic_capabilities(&self, _source: &DiscoveredSource) -> TrafficCapabilities {
        TrafficCapabilities {
            context_transport_mode: ContextTransportMode::FullContext,
            cache_transport_behavior: CacheTransportBehavior::FullContentSent,
            request_reconstruction_quality: ReconstructionQuality::Partial,
            response_reconstruction_quality: ReconstructionQuality::Complete,
        }
    }
}

fn process_message(
    conn: &Connection,
    builder: &mut SessionBuilder,
    msg_id: &str,
    data: &MessageData,
    at: DateTime<Utc>,
    tolerance: &mut ScanTolerance,
) {
    let role = data.role.as_deref().unwrap_or("");
    let turn = match role {
        "user" => builder.new_turn(at),
        _ => builder.ensure_turn(at),
    };

    // 加载该消息的 parts（text/reasoning/tool）
    let mut response_text = String::new();
    let mut user_text = String::new();
    match load_parts(conn, msg_id) {
        Ok(parts) => {
            for part in parts {
                match part.part_type.as_deref() {
                    Some("text") => {
                        if let Some(t) = &part.text {
                            if role == "user" {
                                user_text.push_str(t);
                            } else {
                                response_text.push_str(t);
                                response_text.push('\n');
                            }
                            builder.add_message(turn.clone(), role, "text", Some(t.clone()), at);
                        }
                    }
                    Some("reasoning") => {
                        if let Some(t) = &part.text {
                            builder.add_message(
                                turn.clone(),
                                role,
                                "reasoning",
                                Some(t.clone()),
                                at,
                            );
                        }
                    }
                    Some("tool") => {
                        let call_id = part.call_id.clone().unwrap_or_default();
                        let name = part.tool.clone().unwrap_or_else(|| "tool".into());
                        let status = part
                            .state
                            .as_ref()
                            .and_then(|s| s.status.as_deref())
                            .unwrap_or("");
                        let start = part
                            .state
                            .as_ref()
                            .and_then(|s| s.time.as_ref())
                            .and_then(|t| t.start)
                            .and_then(from_millis)
                            .unwrap_or(at);
                        let input = part.state.as_ref().and_then(|s| s.input.as_ref());
                        let output = part.state.as_ref().and_then(|s| s.output.as_ref());
                        builder.add_tool_use(call_id, name, input, output, status, start);
                    }
                    _ => {}
                }
            }
        }
        Err(e) => tolerance.record(format!("parts 读取失败: {e}")),
    }

    // assistant 消息：记录模型调用
    if role == "assistant" {
        let tokens = data.tokens.as_ref();
        if let Some(tokens) = tokens {
            let has_usage = tokens.input.is_some()
                || tokens.output.is_some()
                || tokens.reasoning.is_some()
                || tokens.cache.is_some();
            if has_usage {
                let model = data
                    .model
                    .as_ref()
                    .and_then(|m| m.model_id.clone())
                    .or_else(|| data.model_id.clone());
                let provider = data
                    .model
                    .as_ref()
                    .and_then(|m| m.provider_id.clone())
                    .or_else(|| data.provider_id.clone());
                let cache = tokens.cache.as_ref().map(|c| (c.read, c.write));
                builder.add_call(
                    turn,
                    msg_id.to_string(),
                    at,
                    model.as_deref(),
                    provider.as_deref(),
                    tokens.input,
                    tokens.output,
                    cache.and_then(|(r, _)| r),
                    cache.and_then(|(_, w)| w),
                    tokens.reasoning,
                    if response_text.is_empty() {
                        None
                    } else {
                        Some(response_text)
                    },
                );
                return;
            }
        }
        if !response_text.is_empty() {
            builder.add_message(turn, "assistant", "text", Some(response_text), at);
        }
    } else if role == "user" && !user_text.is_empty() {
        // 无 parts 时的兜底（已有 text part 时避免重复）
    }
}

fn load_parts(conn: &Connection, msg_id: &str) -> Result<Vec<PartData>, String> {
    let mut stmt = conn
        .prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY time_created")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(metria_storage::rusqlite::params![msg_id], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let data_json = row.map_err(|e| e.to_string())?;
        match serde_json::from_str::<PartData>(&data_json) {
            Ok(p) => out.push(p),
            Err(e) => out.push(PartData {
                part_type: None,
                text: Some(format!("[未解析 part: {e}]")),
                ..Default::default()
            }),
        }
    }
    Ok(out)
}

fn load_session(conn: &Connection, session_id: &str) -> Option<SessionRow> {
    conn.query_row(
        "SELECT id, project_id, parent_id, directory, title, cost, tokens_input, \
         tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, model, \
         time_created, time_updated FROM session WHERE id = ?1",
        metria_storage::rusqlite::params![session_id],
        |r| {
            Ok(SessionRow {
                id: r.get(0).ok(),
                project_id: r.get(1).ok(),
                parent_id: r.get(2).ok(),
                directory: r.get(3).ok(),
                title: r.get(4).ok(),
                cost: r.get(5).ok(),
                tokens_input: r.get(6).ok(),
                tokens_output: r.get(7).ok(),
                tokens_reasoning: r.get(8).ok(),
                tokens_cache_read: r.get(9).ok(),
                tokens_cache_write: r.get(10).ok(),
                model: r.get(11).ok(),
                time_created: r.get(12).ok(),
                time_updated: r.get(13).ok(),
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

fn ensure_schema(conn: &Connection, path: &Path) -> Result<(), AdapterError> {
    for table in ["session", "message", "part"] {
        let exists: Option<i64> = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                metria_storage::rusqlite::params![table],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        if exists.unwrap_or(0) == 0 {
            return Err(AdapterError::SchemaDrift(format!(
                "{}: 缺少必需表 `{table}`（schema 不兼容）",
                path.display()
            )));
        }
    }
    Ok(())
}

fn schema_signature(conn: &Connection) -> String {
    let mut sig = String::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT name, sql FROM sqlite_master WHERE type='table' AND name IN ('session','message','part') ORDER BY name",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            for row in rows.flatten() {
                sig.push_str(&format!("{}:{};", row.0, row.1));
            }
        }
    }
    sig
}

fn dollar_to_micro(dollars: Option<f64>) -> Option<i64> {
    dollars
        .filter(|d| *d > 0.0)
        .map(|d| (d * 1_000_000.0).round() as i64)
}

/// 收集 project 树下的 *.db。
fn collect_dbs(dir: &Path, out: &mut Vec<DiscoveredSource>) -> Result<(), AdapterError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_dbs(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("db") {
            out.push(discover::source(
                "opencode",
                path,
                None,
                vec!["sessions".into(), "usage".into()],
            ));
        }
    }
    Ok(())
}

/// 测试辅助：从游标行读取 last_rowid。
#[allow(dead_code)]
fn cursor_rowid(c: &metria_core::model::SourceCursor) -> Option<i64> {
    match c {
        metria_core::model::SourceCursor::Sqlite(s) => Some(s.last_rowid),
        _ => None,
    }
}
