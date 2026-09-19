//! OpenCode v2（`session_v2` / `session_message`）只读扫描。
//!
//! v2 把内容内嵌在 `session_message.data`，不再写 `part` 表；时间线提供
//! `time.created`（请求开始）、`time.streamed`（首个流式输出）与 `time.completed`。
//! 全程只读、按 rowid 增量、单次扫描行数受限。

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use metria_adapter_api::types::{ScanBatch, ScanIdentity};
use metria_adapter_api::{pseudo_id, AdapterError, ScanTolerance};
use metria_storage::rusqlite::{Connection, OptionalExtension};

use crate::build::{BuildCtx, SessionBuilder};
use crate::entry::{from_millis, parse_session_model, MessageData, SessionRow};

/// v2 单批读取行数：内容内嵌，控制单次扫描内存。
pub(crate) const V2_BATCH_LIMIT: i64 = 2_000;

/// 表是否存在。
pub(crate) fn has_table(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        metria_storage::rusqlite::params![name],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or(0)
        > 0
}

/// v2 schema 必需表检查。
pub(crate) fn ensure_schema_v2(conn: &Connection, path: &Path) -> Result<(), AdapterError> {
    for table in ["session_v2", "session_message"] {
        if !has_table(conn, table) {
            return Err(AdapterError::SchemaDrift(format!(
                "{}: 缺少必需表 `{table}`（v2 schema 不兼容）",
                path.display()
            )));
        }
    }
    Ok(())
}

/// v2 schema 指纹（表结构变化时使游标失效）。
pub(crate) fn schema_signature_v2(conn: &Connection) -> String {
    let mut sig = String::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT name, sql FROM sqlite_master WHERE type='table' AND name IN ('session_v2','session_message') ORDER BY name",
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

/// v2 未完成消息：assistant 且没有 finish/error，且 Token 缺失或全零。
///
/// opencode 先写占位再补真实用量；活跃尾部暂停推进游标等待补全，陈旧未完成则跳过。
fn is_incomplete_v2(kind: &str, d: &MessageData) -> bool {
    if kind != "assistant" || d.finish.is_some() || d.error.is_some() {
        return false;
    }
    let Some(t) = d.tokens.as_ref() else {
        return true;
    };
    let cache = t.cache.as_ref();
    t.input.unwrap_or(0) == 0
        && t.output.unwrap_or(0) == 0
        && t.reasoning.unwrap_or(0) == 0
        && cache.map(|c| c.read.unwrap_or(0)).unwrap_or(0) == 0
        && cache.map(|c| c.write.unwrap_or(0)).unwrap_or(0) == 0
}

/// 扫描 v2 表；返回批次与新的最大 rowid。
pub(crate) fn scan_v2(
    conn: &Connection,
    identity: &ScanIdentity,
    source_path_hash: &str,
    last_rowid: i64,
    tolerance: &mut ScanTolerance,
) -> Result<(ScanBatch, i64), AdapterError> {
    let ctx = BuildCtx {
        node_id: identity.node_id.clone(),
        collector_id: pseudo_id(&identity.collector_id),
        source_id: pseudo_id(source_path_hash),
        client_id: "opencode".into(),
    };
    let mut builders: HashMap<String, SessionBuilder> = HashMap::new();
    let mut session_cache: HashMap<String, SessionRow> = HashMap::new();
    let mut max_rowid = last_rowid;

    let mut stmt = conn
        .prepare(
            "SELECT rowid, id, session_id, type, time_created, data FROM session_message \
             WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
        )
        .map_err(|e| AdapterError::Other(format!("session_message 查询失败: {e}")))?;
    let rows = stmt
        .query_map(
            metria_storage::rusqlite::params![last_rowid, V2_BATCH_LIMIT],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(|e| AdapterError::Other(format!("session_message 读取失败: {e}")))?;

    for row in rows {
        let (rowid, msg_id, session_id, kind, ts_ms, data_json) =
            row.map_err(|e| AdapterError::Other(format!("session_message 行解析失败: {e}")))?;
        let data: MessageData = match serde_json::from_str(&data_json) {
            Ok(d) => d,
            Err(e) => {
                tolerance.record(format!("session_message {msg_id} data 解析失败: {e}"));
                continue;
            }
        };
        let at = from_millis(ts_ms).unwrap_or_else(Utc::now);
        if is_incomplete_v2(&kind, &data) {
            let recent = Utc::now().signed_duration_since(at) < chrono::Duration::minutes(60);
            if recent {
                max_rowid = rowid - 1;
                break;
            }
            continue;
        }
        if rowid > max_rowid {
            max_rowid = rowid;
        }

        let builder = builders.entry(session_id.clone()).or_insert_with(|| {
            let row = session_cache
                .entry(session_id.clone())
                .or_insert_with(|| load_session_v2(conn, &session_id).unwrap_or_default());
            let mut b = SessionBuilder::new(ctx.clone(), session_id.clone(), at);
            let (model, provider) = parse_session_model(row.model.as_deref());
            b.set_meta(
                row.title.clone(),
                row.directory.clone(),
                row.project_id.clone(),
                model,
                provider,
                crate::v2::dollar_to_micro(row.cost),
                row.parent_id.clone(),
            );
            b
        });

        if kind == "assistant" && builder.current_turn_started_at().is_none() {
            if let Some(started_at) =
                load_previous_user_time_v2(conn, &session_id, rowid, tolerance)
            {
                builder.restore_turn_start(started_at);
            }
        }

        process_v2_message(builder, &msg_id, &kind, &data, at, tolerance);
    }

    let batches = crate::finalize_builders(builders, tolerance);
    Ok((batches, max_rowid))
}

fn process_v2_message(
    builder: &mut SessionBuilder,
    msg_id: &str,
    kind: &str,
    data: &MessageData,
    at: chrono::DateTime<Utc>,
    tolerance: &mut ScanTolerance,
) {
    let created = data
        .time
        .as_ref()
        .and_then(|time| time.created)
        .and_then(from_millis)
        .unwrap_or(at);

    if kind == "user" {
        let turn = builder.new_turn(created);
        if let Some(text) = data.text.as_deref().filter(|text| !text.is_empty()) {
            builder.add_message(turn, "user", "text", Some(text.to_string()), created);
        }
        return;
    }
    if kind != "assistant" {
        return;
    }
    let _ = tolerance;
    let turn = builder.ensure_turn(created);

    let mut response_text = String::new();
    if let Some(items) = &data.content {
        for item in items {
            match item.item_type.as_deref() {
                Some("text") => {
                    if let Some(text) = item.text.as_deref().filter(|text| !text.is_empty()) {
                        response_text.push_str(text);
                        response_text.push('\n');
                        builder.add_message(
                            turn.clone(),
                            "assistant",
                            "text",
                            Some(text.to_string()),
                            created,
                        );
                    }
                }
                Some("reasoning") => {
                    if let Some(text) = item.text.as_deref().filter(|text| !text.is_empty()) {
                        builder.add_message(
                            turn.clone(),
                            "assistant",
                            "reasoning",
                            Some(text.to_string()),
                            created,
                        );
                    }
                }
                Some("tool") => {
                    let call_id = item.id.clone().unwrap_or_default();
                    if call_id.is_empty() {
                        continue;
                    }
                    let name = item.name.clone().unwrap_or_else(|| "tool".into());
                    let status = item
                        .state
                        .as_ref()
                        .and_then(|state| state.status.as_deref())
                        .unwrap_or("");
                    let start = item
                        .time
                        .as_ref()
                        .and_then(|time| time.created)
                        .and_then(from_millis)
                        .unwrap_or(created);
                    let input = item.state.as_ref().and_then(|state| state.input.as_ref());
                    let output = item.state.as_ref().and_then(|state| state.output.as_ref());
                    builder.add_tool_use(call_id, name, input, output, status, start);
                }
                _ => {}
            }
        }
    }

    let Some(tokens) = data.tokens.as_ref() else {
        return;
    };
    let has_usage = tokens.input.is_some()
        || tokens.output.is_some()
        || tokens.reasoning.is_some()
        || tokens.cache.is_some();
    if !has_usage {
        return;
    }
    let model = data
        .model
        .as_ref()
        .and_then(|m| m.model_id.clone().or_else(|| m.id.clone()))
        .or_else(|| data.model_id.clone());
    let provider = data
        .model
        .as_ref()
        .and_then(|m| m.provider_id.clone())
        .or_else(|| data.provider_id.clone());
    let cache = tokens.cache.as_ref().map(|c| (c.read, c.write));
    let turn_started_at = builder.current_turn_started_at();
    let first_response_at = data
        .time
        .as_ref()
        .and_then(|time| time.streamed)
        .and_then(from_millis)
        .or_else(|| {
            data.time
                .as_ref()
                .and_then(|time| time.created)
                .and_then(from_millis)
        });
    let completed_at = data
        .time
        .as_ref()
        .and_then(|time| time.completed)
        .and_then(from_millis)
        .or(Some(created));
    let status = match data.finish.as_deref() {
        Some("error") => "error",
        Some("length") => "truncated",
        Some("cancelled") | Some("aborted") | Some("canceled") => "cancelled",
        _ => "success",
    };
    let observed_at = completed_at.unwrap_or(at);
    builder.add_call(
        turn,
        msg_id.to_string(),
        observed_at,
        turn_started_at,
        first_response_at,
        completed_at,
        model.as_deref(),
        provider.as_deref(),
        tokens.input,
        tokens.output,
        cache.and_then(|(read, _)| read),
        cache.and_then(|(_, write)| write),
        tokens.reasoning,
        status,
        dollar_to_micro(data.cost),
        if response_text.is_empty() {
            None
        } else {
            Some(response_text)
        },
    );
}

fn load_previous_user_time_v2(
    conn: &Connection,
    session_id: &str,
    before_rowid: i64,
    tolerance: &mut ScanTolerance,
) -> Option<chrono::DateTime<Utc>> {
    let mut stmt = match conn.prepare(
        "SELECT type, time_created, data FROM session_message \
         WHERE session_id = ?1 AND rowid < ?2 ORDER BY rowid DESC LIMIT 200",
    ) {
        Ok(stmt) => stmt,
        Err(error) => {
            tolerance.record(format!("v2 回合起点查询失败: {error}"));
            return None;
        }
    };
    let rows = match stmt.query_map(
        metria_storage::rusqlite::params![session_id, before_rowid],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    ) {
        Ok(rows) => rows,
        Err(error) => {
            tolerance.record(format!("v2 回合起点读取失败: {error}"));
            return None;
        }
    };
    for row in rows {
        let Ok((kind, ts_ms, data_json)) = row else {
            continue;
        };
        if kind != "user" {
            continue;
        }
        let created = serde_json::from_str::<MessageData>(&data_json)
            .ok()
            .and_then(|data| data.time.and_then(|time| time.created))
            .unwrap_or(ts_ms);
        return from_millis(created);
    }
    None
}

fn load_session_v2(conn: &Connection, session_id: &str) -> Option<SessionRow> {
    conn.query_row(
        "SELECT id, project_id, parent_id, directory, title, cost, tokens_input, \
         tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, model, \
         time_created, time_updated FROM session_v2 WHERE id = ?1",
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

/// 美元 → 微美元（仅正数）。
pub(crate) fn dollar_to_micro(dollars: Option<f64>) -> Option<i64> {
    dollars
        .filter(|d| *d > 0.0)
        .map(|d| (d * 1_000_000.0).round() as i64)
}
