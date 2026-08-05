//! metria-adapter-codex: Codex Sessions/Rollout Adapter。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod build;
pub mod entry;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use metria_adapter_api::types::{
    discover, AdapterCapabilities, DiscoveredSource, DiscoveryContext, ScanBatch, SourceAdapter,
    SourceHealth, TrafficCapabilities,
};
use metria_adapter_api::{pseudo_id, scan_jsonl_file, AdapterError, ScanTolerance};
use metria_core::model::{
    CacheTransportBehavior, ContextTransportMode, ReconstructionQuality, SourceStatus,
};

use build::{entry_time, jsonl_cursor, BuildCtx, SessionBuilder};
use entry::{
    MessagePayload, RawEvent, ReasoningPayload, SessionMeta, TokenCount, ToolCallOutputPayload,
    ToolCallPayload, UserMessagePayload,
};

/// 单行上限。
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// Codex Adapter。
#[derive(Debug, Default, Clone)]
pub struct CodexAdapter;

impl SourceAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
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
            subagents: false,
            project_path: true,
            reported_cost: false,
            model_switching: false,
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
            let sessions = root.join("sessions");
            if sessions.is_dir() {
                collect_rollouts(&sessions, &mut out)?;
            }
            // 兼容扁平夹具布局：根目录下的 *.jsonl（跳过 history.jsonl）
            collect_root_jsonl(root, &mut out)?;
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
        let offset = match cursor {
            Some(metria_core::model::SourceCursor::Jsonl(c)) => c.byte_offset,
            _ => 0,
        };
        let meta = fs::metadata(path)?;
        let inode = meta_file_inode(&meta);
        let size = meta.len() as i64;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let ctx = BuildCtx {
            node_id: identity.node_id.clone(),
            collector_id: pseudo_id(&identity.collector_id),
            source_id: pseudo_id(source.path_hash.as_str()),
            client_id: "codex".into(),
        };

        let mut tolerance = ScanTolerance::default();
        let mut state = ScanState::new();
        let mut entry_warnings: Vec<String> = Vec::new();

        let new_offset = scan_jsonl_file(
            path,
            offset as u64,
            MAX_LINE_BYTES,
            |value| {
                let event: RawEvent = match serde_json::from_value(value.clone()) {
                    Ok(e) => e,
                    Err(e) => {
                        entry_warnings.push(format!("记录解析失败: {e}"));
                        return Ok(());
                    }
                };
                if let Err(e) = process_event(&mut state, &ctx, &event) {
                    entry_warnings.push(e);
                }
                Ok(())
            },
            &mut tolerance,
        )?;
        tolerance.warnings.extend(entry_warnings);

        let mut batches = ScanBatch::default();
        for (_, b) in state.builders {
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
        batches.next_cursor = Some(jsonl_cursor(
            source.path_hash.clone(),
            inode,
            size,
            mtime,
            new_offset as i64,
        ));

        Ok(batches)
    }

    fn health(&self, source: &DiscoveredSource) -> Result<SourceHealth, AdapterError> {
        let path = &source.canonical_path;
        if !path.exists() {
            return Ok(SourceHealth {
                ok: false,
                status: SourceStatus::Missing,
                message: Some("文件不存在".into()),
                last_error: None,
            });
        }
        match fs::metadata(path) {
            Ok(m) if m.is_file() => Ok(SourceHealth {
                ok: true,
                status: SourceStatus::Active,
                message: None,
                last_error: None,
            }),
            Ok(_) => Ok(SourceHealth {
                ok: false,
                status: SourceStatus::Error,
                message: Some("不是普通文件".into()),
                last_error: None,
            }),
            Err(e) => Ok(SourceHealth {
                ok: false,
                status: SourceStatus::Error,
                message: Some(format!("不可读: {e}")),
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

/// 扫描状态：一个 rollout 文件对应一个会话，session_meta 后事件路由到当前会话。
#[derive(Debug, Default)]
struct ScanState {
    builders: HashMap<String, SessionBuilder>,
    current: Option<String>,
}

impl ScanState {
    fn new() -> Self {
        Self::default()
    }

    fn builder<'a>(
        &'a mut self,
        ctx: &BuildCtx,
        sid: &str,
        at: chrono::DateTime<chrono::Utc>,
    ) -> &'a mut SessionBuilder {
        self.current = Some(sid.to_string());
        self.builders
            .entry(sid.to_string())
            .or_insert_with(|| SessionBuilder::new(ctx.clone(), sid.to_string(), at))
    }
}

fn process_event(state: &mut ScanState, ctx: &BuildCtx, event: &RawEvent) -> Result<(), String> {
    if event.event_type == "session_meta" {
        let meta: SessionMeta =
            serde_json::from_value(event.payload.clone().unwrap_or(serde_json::json!({})))
                .map_err(|e| format!("session_meta 解析失败: {e}"))?;
        let Some(sid) = meta.session_id.clone() else {
            return Ok(());
        };
        let at = meta
            .timestamp
            .as_deref()
            .and_then(entry_time)
            .or_else(|| event.timestamp.as_deref().and_then(entry_time))
            .unwrap_or_else(utc_now);
        let builder = state.builder(ctx, &sid, at);
        builder.set_meta(
            meta.cwd.as_deref(),
            meta.cli_version.as_deref(),
            meta.model_provider.as_deref(),
        );
        return Ok(());
    }

    let Some(sid) = state.current.clone() else {
        return Ok(()); // 尚无会话上下文（如未解析到 session_meta）
    };
    let at = event
        .timestamp
        .as_deref()
        .and_then(entry_time)
        .unwrap_or_else(utc_now);
    let builder = state.builder(ctx, &sid, at);

    // 检测 previous_response_id / response_id → stateful reference
    if event_payload_has_ref(event) {
        builder.mark_stateful_reference();
    }

    match event.event_type.as_str() {
        "event_msg" => process_event_msg(builder, event, at),
        "response_item" => process_response_item(builder, event, at),
        _ => Ok(()),
    }
}

fn process_event_msg(
    builder: &mut SessionBuilder,
    event: &RawEvent,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    let payload = event.payload.clone().unwrap_or(serde_json::json!({}));
    let ptype = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match ptype {
        "user_message" => {
            let p: UserMessagePayload = serde_json::from_value(payload)
                .map_err(|e| format!("user_message 解析失败: {e}"))?;
            let turn = builder.new_turn(at);
            if let Some(text) = p.message {
                builder.add_message(turn, "user", "text", Some(text), at);
            }
        }
        "agent_message" => {
            // 注释性消息，记录为 assistant 消息
            let text = payload
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            if !text.is_empty() {
                let turn = builder.ensure_turn(at);
                builder.add_message(turn, "assistant", "text", Some(text), at);
            }
        }
        "token_count" => {
            let p: TokenCount = serde_json::from_value(payload)
                .map_err(|e| format!("token_count 解析失败: {e}"))?;
            if let Some(info) = p.info {
                if let Some(u) = info.last_token_usage {
                    // 全零 usage 视为无实际调用（客户端在会话开始/结束上报的空统计），不产生假调用
                    let zero = u.input_tokens.unwrap_or(0) == 0
                        && u.output_tokens.unwrap_or(0) == 0
                        && u.reasoning_output_tokens.unwrap_or(0) == 0;
                    if zero {
                        return Ok(());
                    }
                    let turn = builder.ensure_turn(at);
                    let response_text = take_turn_response(builder);
                    builder.add_call(
                        turn,
                        at,
                        u.input_tokens,
                        u.output_tokens,
                        u.cached_input_tokens,
                        u.cache_write_input_tokens,
                        u.reasoning_output_tokens,
                        None,
                        response_text,
                    );
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn process_response_item(
    builder: &mut SessionBuilder,
    event: &RawEvent,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    let payload = event.payload.clone().unwrap_or(serde_json::json!({}));
    let ptype = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match ptype {
        "message" => {
            let p: MessagePayload =
                serde_json::from_value(payload).map_err(|e| format!("message 解析失败: {e}"))?;
            let role = p.role.unwrap_or_default();
            let turn = builder.ensure_turn(at);
            if let Some(items) = p.content {
                for item in items {
                    if let Some(text) = item.text {
                        builder.add_message(turn.clone(), &role, &item.item_type, Some(text), at);
                    }
                }
            }
        }
        "reasoning" => {
            let p: ReasoningPayload =
                serde_json::from_value(payload).map_err(|e| format!("reasoning 解析失败: {e}"))?;
            // 摘要可选记录（正文可能加密，不保存）
            if let Some(s) = p.summary {
                if !s.is_empty() {
                    let turn = builder.ensure_turn(at);
                    builder.add_message(turn, "assistant", "reasoning", Some(s), at);
                }
            }
        }
        "custom_tool_call" | "function_call" => {
            let p: ToolCallPayload =
                serde_json::from_value(payload).map_err(|e| format!("tool_call 解析失败: {e}"))?;
            let call_id = p
                .call_id
                .clone()
                .or_else(|| p.id.clone())
                .unwrap_or_default();
            if !call_id.is_empty() {
                let name = p.name.unwrap_or_else(|| "unknown".into());
                let input_len = p
                    .arguments
                    .as_deref()
                    .map(|a| a.len() as i64)
                    .or_else(|| p.input.as_ref().map(|v| v.to_string().len() as i64))
                    .unwrap_or(0);
                builder.add_tool_use(call_id, name, input_len, at);
            }
        }
        "custom_tool_call_output" | "function_call_output" => {
            let p: ToolCallOutputPayload = serde_json::from_value(payload)
                .map_err(|e| format!("tool_call_output 解析失败: {e}"))?;
            if let Some(call_id) = p.call_id {
                let out_len = p
                    .output
                    .as_ref()
                    .map(|v| v.to_string().len() as i64)
                    .unwrap_or(0);
                builder.complete_tool_result(&call_id, p.is_error.unwrap_or(false), out_len, at);
            }
        }
        _ => {}
    }
    Ok(())
}

/// 取出当前回合的 assistant 可见文本（作为本次调用的响应重建输入）。
fn take_turn_response(builder: &mut SessionBuilder) -> Option<String> {
    let text: String = builder
        .messages
        .iter()
        .rev()
        .take_while(|m| m.role == "assistant")
        .filter(|m| m.content_type == "output_text" || m.content_type == "text")
        .filter_map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn event_payload_has_ref(event: &RawEvent) -> bool {
    let Some(payload) = &event.payload else {
        return false;
    };
    let s = payload.to_string();
    s.contains("previous_response_id") || s.contains("\"response_id\"")
}

fn utc_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

/// 收集根目录下兼容夹具布局的 *.jsonl（真实目录仅历史文件被跳过）。
fn collect_root_jsonl(dir: &Path, out: &mut Vec<DiscoveredSource>) -> Result<(), AdapterError> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "history.jsonl" {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let caps = vec!["sessions".into(), "usage".into()];
            out.push(discover::source("codex", path, None, caps));
        }
    }
    Ok(())
}

/// 收集 sessions 树下的 rollout-*.jsonl 文件。
fn collect_rollouts(dir: &Path, out: &mut Vec<DiscoveredSource>) -> Result<(), AdapterError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rollouts(&path, out)?;
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("rollout-") && name.ends_with(".jsonl") {
                let caps = vec![
                    "sessions".into(),
                    "messages".into(),
                    "tool_calls".into(),
                    "usage".into(),
                    "reasoning_tokens".into(),
                    "cache_tokens".into(),
                ];
                out.push(discover::source("codex", path, None, caps));
            }
        }
    }
    Ok(())
}

/// 从文件元数据取 inode（Linux）。
#[cfg(unix)]
fn meta_file_inode(meta: &fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino() as i64
}

#[cfg(not(unix))]
fn meta_file_inode(_meta: &fs::Metadata) -> i64 {
    0
}
