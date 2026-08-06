//! metria-adapter-claude: Claude Code JSONL Adapter。
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
    CacheTransportBehavior, ContextTransportMode, Id, ReconstructionQuality, SourceStatus,
};

use build::{entry_time, jsonl_cursor, BuildCtx, SessionBuilder};
use entry::{is_assistant, is_real_user_prompt, RawContent, RawEntry};

/// 单行上限（工具结果可能很大）。
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// Claude Code Adapter。
#[derive(Debug, Default, Clone)]
pub struct ClaudeCodeAdapter;

impl SourceAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
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
            model_switching: true,
            reasoning_tokens: false,
            cache_tokens: true,
            request_reconstruction: false,
            response_reconstruction: true,
            context_transport_detection: true,
        }
    }

    fn discover(&self, context: &DiscoveryContext) -> Result<Vec<DiscoveredSource>, AdapterError> {
        let mut out = Vec::new();
        for root in &context.root_paths {
            let projects = root.join("projects");
            if projects.is_dir() {
                collect_jsonl(&projects, &mut out)?;
            }
            // 兼容扁平布局（测试夹具等）：根目录下的 *.jsonl
            collect_jsonl(root, &mut out)?;
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
            client_id: "claude-code".into(),
        };

        let mut tolerance = ScanTolerance::default();
        let mut builders: HashMap<String, SessionBuilder> = HashMap::new();
        let mut entry_warnings: Vec<String> = Vec::new();

        let new_offset = scan_jsonl_file(
            path,
            offset as u64,
            MAX_LINE_BYTES,
            |value| {
                let entry: RawEntry = match serde_json::from_value(value.clone()) {
                    Ok(e) => e,
                    Err(e) => {
                        entry_warnings.push(format!("记录解析失败: {e}"));
                        return Ok(());
                    }
                };
                if let Err(e) = process_entry(&mut builders, &ctx, &entry) {
                    entry_warnings.push(e);
                }
                Ok(())
            },
            &mut tolerance,
        )?;
        tolerance.warnings.extend(entry_warnings);

        // 汇总所有构建器
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

fn process_entry(
    builders: &mut HashMap<String, SessionBuilder>,
    ctx: &BuildCtx,
    entry: &RawEntry,
) -> Result<(), String> {
    let Some(sid) = entry.session_id.clone() else {
        return Ok(()); // 无 session 的条目忽略
    };
    let Some(at) = entry_time(entry) else {
        return Ok(()); // 无时间戳忽略
    };

    let builder = builders
        .entry(sid.clone())
        .or_insert_with(|| SessionBuilder::new(ctx.clone(), sid.clone(), at));

    if let Some(cwd) = &entry.cwd {
        builder.set_cwd_hash(cwd);
    }
    match entry.entry_type.as_str() {
        "ai-title" | "title" => {
            if let Some(t) = &entry.title {
                builder.set_title(t.clone());
            }
        }
        "summary" => {
            if let Some(s) = &entry.summary {
                builder.set_title(s.clone());
            }
        }
        _ => {}
    }

    if is_real_user_prompt(entry) {
        let turn = builder.ensure_turn(true, "user", at);
        let msg = entry.message.as_ref().ok_or("user 消息缺失 message")?;
        let source_msg_id = msg.id.clone().or_else(|| entry.uuid.clone());
        match &msg.content {
            Some(RawContent::Text(t)) => {
                builder.add_message(turn, source_msg_id, "user", "text", Some(t.clone()), at);
            }
            Some(RawContent::Blocks(blocks)) => {
                for b in blocks {
                    if let Some(t) = b.visible_text() {
                        builder.add_message(
                            turn.clone(),
                            source_msg_id.clone(),
                            "user",
                            &b.block_type,
                            Some(t),
                            at,
                        );
                    }
                }
            }
            _ => {}
        }
        return Ok(());
    }

    if is_assistant(entry) {
        let turn = builder.ensure_turn(false, "assistant", at);
        let msg = entry.message.as_ref().ok_or("assistant 消息缺失 message")?;
        let source_msg_id = msg.id.clone().or_else(|| entry.uuid.clone());
        let mut response_text = String::new();
        let mut tool_uses = Vec::new();

        if let Some(RawContent::Blocks(blocks)) = &msg.content {
            for b in blocks {
                match b.block_type.as_str() {
                    "text" => {
                        if let Some(t) = &b.text {
                            response_text.push_str(t);
                            response_text.push('\n');
                            builder.add_message(
                                turn.clone(),
                                source_msg_id.clone(),
                                "assistant",
                                "text",
                                Some(t.clone()),
                                at,
                            );
                        }
                    }
                    "thinking" => {
                        if let Some(t) = &b.thinking {
                            builder.add_message(
                                turn.clone(),
                                source_msg_id.clone(),
                                "assistant",
                                "thinking",
                                Some(t.clone()),
                                at,
                            );
                        }
                    }
                    "tool_use" => {
                        if let (Some(id), Some(name)) = (b.id.clone(), b.name.clone()) {
                            builder.add_tool_use(id.clone(), name.clone(), b.input.as_ref(), at);
                            tool_uses.push(id);
                            // Claude 子代理：Task tool_use 的 input 含 leafUuid / sessionId
                            if name == "Task" {
                                if let Some(input) = &b.input {
                                    let leaf =
                                        input.get("leafUuid").and_then(|v| v.as_str()).or_else(
                                            || input.get("sessionId").and_then(|v| v.as_str()),
                                        );
                                    if let Some(leaf) = leaf {
                                        builder.note_subagent_leaf(leaf);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // 模型调用：assistant 消息带 usage 即一次调用
        if let Some(usage) = &msg.usage {
            if !usage.is_empty() {
                let call_id = msg
                    .id
                    .clone()
                    .or_else(|| entry.uuid.clone())
                    .unwrap_or_else(|| Id::new().as_str().to_string());
                let model = msg.model.clone().or_else(|| entry.model_field.clone());
                builder.add_call(
                    turn,
                    call_id,
                    model,
                    at,
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_read_input_tokens,
                    usage
                        .cache_creation_input_tokens
                        .or(usage.cache_write_input_tokens),
                    if response_text.is_empty() {
                        None
                    } else {
                        Some(response_text)
                    },
                );
            }
        }
        let _ = tool_uses;
        return Ok(());
    }

    // user 消息且含 tool_result / 附件：回填工具结果
    if let Some(msg) = &entry.message {
        if msg.role.as_deref() == Some("user") {
            if let Some(RawContent::Blocks(blocks)) = &msg.content {
                let turn = builder.ensure_turn(false, "user", at);
                let source_msg_id = msg.id.clone().or_else(|| entry.uuid.clone());
                for b in blocks {
                    if b.block_type == "tool_result" {
                        if let Some(tid) = &b.tool_use_id {
                            builder.complete_tool_result(
                                tid,
                                b.is_error.unwrap_or(false),
                                b.content.as_ref(),
                                at,
                            );
                        }
                        if let Some(t) = b.visible_text() {
                            builder.add_message(
                                turn.clone(),
                                source_msg_id.clone(),
                                "user",
                                "tool_result",
                                Some(t),
                                at,
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// 递归收集目录下的 JSONL 文件（含子目录）。
fn collect_jsonl(dir: &Path, out: &mut Vec<DiscoveredSource>) -> Result<(), AdapterError> {
    for file in fs::read_dir(dir)? {
        let file = file?;
        let path = file.path();
        if path.is_dir() {
            collect_jsonl(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let version = peek_version(&path);
            let caps = vec![
                "sessions".into(),
                "messages".into(),
                "tool_calls".into(),
                "usage".into(),
                "cache_tokens".into(),
            ];
            out.push(discover::source("claude-code", path, version, caps));
        }
    }
    Ok(())
}

/// 读取文件首行以探测客户端版本。
fn peek_version(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let entry: RawEntry = serde_json::from_str(&line).ok()?;
    entry.version
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
