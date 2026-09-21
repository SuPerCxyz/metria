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

/// v2 单窗读取行数上限：内容内嵌，控制单窗内存。
pub(crate) const V2_BATCH_LIMIT: i64 = 2_000;

/// 首次补采时优先覆盖的最新行数（保证最近数据先出现）。
pub(crate) const V2_TAIL_BOOTSTRAP: i64 = 8_000;

/// 单窗行数据字节预算：窗口按字节提前结束，避免单窗内存随行数增长。
pub(crate) const V2_WINDOW_BYTES: usize = 8 * 1024 * 1024;

/// 单轮扫描（尾部 + 历史）总字节预算：跨轮续扫，避免单轮内存峰值。
pub(crate) const V2_SCAN_BYTES: usize = 16 * 1024 * 1024;

/// 双游标：`frontier` 为历史回补进度，`tail_marker`/`tail_floor` 覆盖最新区间。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct V2Cursor {
    pub frontier: i64,
    pub tail_marker: i64,
    pub tail_floor: i64,
}

impl V2Cursor {
    /// 编码进 `SqliteCursor.last_primary_key`。
    pub(crate) fn encode(&self) -> String {
        format!("tail={};floor={}", self.tail_marker, self.tail_floor)
    }

    /// 从游标字段解析；缺失或损坏时返回 None（重新引导）。
    pub(crate) fn decode(frontier: i64, primary_key: Option<&str>) -> Option<Self> {
        let key = primary_key?;
        let mut tail = None;
        let mut floor = None;
        for part in key.split(';') {
            if let Some(value) = part.strip_prefix("tail=") {
                tail = value.parse::<i64>().ok();
            } else if let Some(value) = part.strip_prefix("floor=") {
                floor = value.parse::<i64>().ok();
            }
        }
        Some(Self {
            frontier,
            tail_marker: tail?,
            tail_floor: floor?,
        })
    }
}

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

/// 扫描 v2 表：先补最新区间（tail），再按 rowid 回补历史，两者在同一批返回。
pub(crate) fn scan_v2(
    conn: &Connection,
    identity: &ScanIdentity,
    source_path_hash: &str,
    cursor: Option<V2Cursor>,
    frontier_hint: i64,
    tolerance: &mut ScanTolerance,
) -> Result<(ScanBatch, V2Cursor), AdapterError> {
    scan_v2_with_limits(
        conn,
        identity,
        source_path_hash,
        cursor,
        frontier_hint,
        V2_TAIL_BOOTSTRAP,
        V2_BATCH_LIMIT,
        V2_BATCH_LIMIT,
        V2_WINDOW_BYTES,
        V2_SCAN_BYTES,
        tolerance,
    )
}

/// 可配置窗口的扫描入口（测试用）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn scan_v2_with_limits(
    conn: &Connection,
    identity: &ScanIdentity,
    source_path_hash: &str,
    cursor: Option<V2Cursor>,
    frontier_hint: i64,
    tail_bootstrap: i64,
    tail_limit: i64,
    history_limit: i64,
    window_bytes: usize,
    scan_bytes: usize,
    tolerance: &mut ScanTolerance,
) -> Result<(ScanBatch, V2Cursor), AdapterError> {
    let max_rowid: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM session_message",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let mut state = match cursor {
        Some(state) => state,
        None => {
            // 首次补采：尾部窗口先覆盖最近 tail_bootstrap 行（不早于已有历史进度），
            // 历史从 frontier_hint 向上追。
            let frontier = frontier_hint.max(0);
            let floor = max_rowid.saturating_sub(tail_bootstrap).max(frontier);
            V2Cursor {
                frontier,
                tail_marker: floor,
                tail_floor: floor,
            }
        }
    };
    let ctx = BuildCtx {
        node_id: identity.node_id.clone(),
        collector_id: pseudo_id(&identity.collector_id),
        source_id: pseudo_id(source_path_hash),
        client_id: "opencode".into(),
    };
    let mut builders: HashMap<String, SessionBuilder> = HashMap::new();
    let mut session_cache: HashMap<String, SessionRow> = HashMap::new();

    // 单轮总字节预算：尾部先消费，剩余额度再给历史窗口；不足时留待下一轮。
    let mut consumed = 0usize;
    state.tail_marker = scan_window(
        conn,
        &ctx,
        state.tail_marker,
        None,
        tail_limit,
        &mut builders,
        &mut session_cache,
        window_bytes,
        scan_bytes,
        &mut consumed,
        tolerance,
    )?;
    if state.frontier < state.tail_floor && consumed < scan_bytes {
        state.frontier = scan_window(
            conn,
            &ctx,
            state.frontier,
            Some(state.tail_floor),
            history_limit,
            &mut builders,
            &mut session_cache,
            window_bytes,
            scan_bytes,
            &mut consumed,
            tolerance,
        )?;
    }

    let batches = crate::finalize_builders(builders, tolerance);
    Ok((batches, state))
}

/// 扫描一个 rowid 窗口：`from < rowid <= min(cap, ...)`，返回新的标记。
///
/// `consumed` 为单轮累计行数据字节（跨窗口共享）；`window_bytes` 限制单窗、
/// `scan_bytes` 限制单轮总量，达到预算时在当前行之前停止，标记停在上一完整行。
#[allow(clippy::too_many_arguments)]
fn scan_window(
    conn: &Connection,
    ctx: &BuildCtx,
    from_rowid: i64,
    cap_rowid: Option<i64>,
    limit: i64,
    builders: &mut HashMap<String, SessionBuilder>,
    session_cache: &mut HashMap<String, SessionRow>,
    window_bytes: usize,
    scan_bytes: usize,
    consumed: &mut usize,
    tolerance: &mut ScanTolerance,
) -> Result<i64, AdapterError> {
    let mut marker = from_rowid;
    let mut window_consumed = 0usize;
    let cap = cap_rowid.unwrap_or(i64::MAX);
    let mut stmt = conn
        .prepare(
            "SELECT rowid, id, session_id, type, time_created, data FROM session_message \
             WHERE rowid > ?1 AND rowid <= ?2 ORDER BY rowid LIMIT ?3",
        )
        .map_err(|e| AdapterError::Other(format!("session_message 查询失败: {e}")))?;
    let rows = stmt
        .query_map(
            metria_storage::rusqlite::params![from_rowid, cap, limit],
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
        // 字节预算：达到后在本行之前停止，已处理的完整行全部生效，下一轮从此继续。
        let over_window = window_consumed > 0 && window_consumed >= window_bytes;
        let over_scan = *consumed > 0 && *consumed >= scan_bytes;
        if over_window || over_scan {
            break;
        }
        window_consumed += data_json.len();
        *consumed += data_json.len();
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
                marker = rowid - 1;
                break;
            }
            continue;
        }
        if rowid > marker {
            marker = rowid;
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
                dollar_to_micro(row.cost),
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
    Ok(marker)
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

#[cfg(test)]
mod tests {
    use super::*;
    use metria_storage::rusqlite::Connection;

    fn setup(conn: &Connection, rows: i64) {
        conn.execute_batch(
            r#"
            CREATE TABLE session_v2 (
              id TEXT PRIMARY KEY, project_id TEXT NOT NULL, parent_id TEXT, directory TEXT,
              title TEXT, cost REAL DEFAULT 0 NOT NULL, tokens_input INTEGER DEFAULT 0,
              tokens_output INTEGER DEFAULT 0, tokens_reasoning INTEGER DEFAULT 0,
              tokens_cache_read INTEGER DEFAULT 0, tokens_cache_write INTEGER DEFAULT 0,
              model TEXT, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL
            );
            CREATE TABLE session_message (
              id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL, seq INTEGER,
              time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_v2 (id, project_id, directory, title, time_created, time_updated) VALUES ('s','p','/tmp/x','t',0,0)",
            [],
        )
        .unwrap();
        for i in 1..=rows {
            let ts = 1_700_000_000_000i64 + i;
            conn.execute(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES (?1,'s','user',?2,?3,?3,?4)",
                metria_storage::rusqlite::params![
                    format!("u{i}"),
                    i,
                    ts,
                    format!(r#"{{"time":{{"created":{ts}}},"text":"m{i}"}}"#)
                ],
            )
            .unwrap();
        }
    }

    fn test_identity() -> ScanIdentity {
        ScanIdentity {
            node_id: "n".into(),
            collector_id: "c".into(),
        }
    }

    #[test]
    fn child_session_records_parent_id() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn, 2);
        // 子会话（parent_id 指向 's'）及其消息
        conn.execute(
            "INSERT INTO session_v2 (id, project_id, parent_id, directory, title, time_created, time_updated) VALUES ('child','p','s','/tmp/x','child',0,0)",
            [],
        )
        .unwrap();
        let ts = 1_700_000_000_500i64;
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES ('c1','child','user',1,?1,?1,?2)",
            metria_storage::rusqlite::params![
                ts,
                format!(r#"{{"time":{{"created":{ts}}},"text":"child"}}"#)
            ],
        )
        .unwrap();

        let mut tolerance = ScanTolerance::default();
        let (batch, _) = scan_v2_with_limits(
            &conn,
            &test_identity(),
            "hash",
            None,
            0,
            100,
            100,
            100,
            V2_WINDOW_BYTES,
            V2_SCAN_BYTES,
            &mut tolerance,
        )
        .unwrap();
        let child = batch
            .sessions
            .iter()
            .find(|s| s.source_session_id == "child")
            .expect("应扫描到子会话");
        assert_eq!(
            child.parent_session_id.as_ref().map(|id| id.as_str()),
            Some("s"),
            "子会话必须记录父会话 id"
        );
        assert!(
            batch
                .subagent_relations
                .iter()
                .any(|r| r.child_session_id.as_str() == "child"),
            "同批次的父子会话应产生子代理关系"
        );

        // 重复扫描（模拟每轮轮询）：关系 id 必须稳定，Hub 才能幂等去重
        let (batch2, _) = scan_v2_with_limits(
            &conn,
            &test_identity(),
            "hash",
            None,
            0,
            100,
            100,
            100,
            V2_WINDOW_BYTES,
            V2_SCAN_BYTES,
            &mut tolerance,
        )
        .unwrap();
        let ids1: Vec<String> = batch
            .subagent_relations
            .iter()
            .map(|r| r.id.as_str().to_string())
            .collect();
        let ids2: Vec<String> = batch2
            .subagent_relations
            .iter()
            .map(|r| r.id.as_str().to_string())
            .collect();
        assert_eq!(ids1, ids2, "重复扫描的关系 id 必须稳定（幂等去重前提）");
    }

    #[test]
    fn dual_cursor_covers_tail_first_then_history() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn, 20);
        let mut tolerance = ScanTolerance::default();
        let mut cursor: Option<V2Cursor> = None;
        let mut total_messages = 0usize;
        let mut first_batch_newest = false;

        for round in 0..12 {
            let (batch, next) = scan_v2_with_limits(
                &conn,
                &test_identity(),
                "hash",
                cursor.clone(),
                0,
                5,
                3,
                4,
                V2_WINDOW_BYTES,
                V2_SCAN_BYTES,
                &mut tolerance,
            )
            .unwrap();
            if round == 0 {
                let contents: Vec<String> = batch
                    .messages
                    .iter()
                    .filter_map(|m| m.content.clone())
                    .collect();
                // 首轮必须覆盖最新写入的行（m20 附近），证明尾部优先。
                first_batch_newest = contents
                    .iter()
                    .any(|c| c == "m20" || c == "m19" || c == "m18");
            }
            total_messages += batch.messages.len();
            cursor = Some(next);
            let state = cursor.as_ref().unwrap();
            if state.frontier >= state.tail_floor && state.tail_marker >= 20 {
                break;
            }
        }

        assert!(first_batch_newest, "首轮应优先覆盖最新消息");
        assert_eq!(
            total_messages, 20,
            "所有消息应恰好处理一次（无重复/无遗漏）"
        );
    }

    #[test]
    fn byte_budget_splits_scan_and_never_regresses() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn, 30);
        let mut tolerance = ScanTolerance::default();
        let mut cursor: Option<V2Cursor> = None;
        let mut last_progress = (-1i64, -1i64);
        let mut rounds = 0usize;
        let mut total_messages = 0usize;

        for _ in 0..40 {
            let (batch, next) = scan_v2_with_limits(
                &conn,
                &test_identity(),
                "hash",
                cursor.clone(),
                0,
                6,
                10,
                10,
                150, // 单窗字节预算：小到每轮只能处理几行
                300, // 单轮总预算
                &mut tolerance,
            )
            .unwrap();
            total_messages += batch.messages.len();
            assert!(
                (next.frontier, next.tail_marker) >= last_progress,
                "预算分窗不得回退进度: {:?} -> {:?}",
                last_progress,
                (next.frontier, next.tail_marker)
            );
            last_progress = (next.frontier, next.tail_marker);
            let done = next.frontier >= next.tail_floor && next.tail_marker >= 30;
            cursor = Some(next);
            rounds += 1;
            if done {
                break;
            }
        }

        assert!(rounds > 1, "小预算应触发多轮续扫");
        // 预算分轮后必须恰好消费全部 30 行（不重复不遗漏）
        assert_eq!(total_messages, 30, "所有消息应恰好处理一次");
        // 最终必须消费完所有 30 行：再用一次空扫描确认进度已追上
        let (batch, next) = scan_v2_with_limits(
            &conn,
            &test_identity(),
            "hash",
            cursor.clone(),
            0,
            6,
            10,
            10,
            150,
            300,
            &mut tolerance,
        )
        .unwrap();
        assert!(
            batch.messages.is_empty(),
            "预算分轮后应恰好消费全部行，不重复不遗漏"
        );
        assert_eq!(next.tail_marker, 30);
    }
}
