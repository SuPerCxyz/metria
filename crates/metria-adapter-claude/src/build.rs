//! 会话构建器：将条目流聚合为 Session/Turn/Message/ModelCall/UsageEvent/ToolEvent/SubagentRelation。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use metria_core::model::{
    CallGranularity, Id, Message, ModelCall, Session, SessionStatus, SourceCursor,
    SubagentRelation, ToolEvent, TrafficEstimate, Turn, UsageEvent, UsageGranularity, UsageSource,
};
use metria_core::normalize::normalize_model;
use metria_traffic::{estimate, EstimateInput};

use crate::entry::RawEntry;

/// 会话构建上下文（来源相关固定信息）。
#[derive(Debug, Clone)]
pub struct BuildCtx {
    pub node_id: String,
    pub collector_id: Id,
    pub source_id: Id,
    pub client_id: String,
}

/// 累积正文上限（请求重建近似，防止 O(n^2)）。
const RUNNING_TEXT_CAP: usize = 512 * 1024;

/// 单个会话的构建状态。
#[derive(Debug)]
pub struct SessionBuilder {
    ctx: BuildCtx,
    pub session: Session,
    pub turns: Vec<Turn>,
    pub messages: Vec<Message>,
    pub calls: Vec<ModelCall>,
    pub usage: Vec<UsageEvent>,
    pub tools: Vec<ToolEvent>,
    pub subagents: Vec<SubagentRelation>,
    pub traffic: Vec<TrafficEstimate>,
    current_turn: Option<Id>,
    tool_map: HashMap<String, usize>,
    running_text: String,
    running_bytes: usize,
    last_activity: Option<DateTime<Utc>>,
    message_seq: i64,
    turn_seq: i64,
    pub warnings: Vec<String>,
}

impl SessionBuilder {
    pub fn new(ctx: BuildCtx, source_session_id: String, started_at: DateTime<Utc>) -> Self {
        let session = Session {
            id: Id::new(),
            source_session_id,
            node_id: ctx.node_id.clone(),
            collector_id: ctx.collector_id.clone(),
            source_id: ctx.source_id.clone(),
            client_id: ctx.client_id.clone(),
            project_id: None,
            parent_session_id: None,
            title: None,
            working_directory_hash: None,
            started_at,
            ended_at: None,
            last_activity_at: Some(started_at),
            provider_raw: None,
            provider_normalized: None,
            primary_model_raw: None,
            primary_model_normalized: None,
            status: SessionStatus::Active,
            message_count: 0,
            tool_call_count: 0,
            subagent_count: 0,
            model_call_count: 0,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            reported_cost_micro_usd: None,
            calculated_cost_micro_usd: None,
            estimated_cost_micro_usd: None,
            estimated_request_bytes: None,
            estimated_response_bytes: None,
            estimated_total_bytes: None,
            traffic_confidence: None,
            content_available: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        Self {
            ctx,
            session,
            turns: Vec::new(),
            messages: Vec::new(),
            calls: Vec::new(),
            usage: Vec::new(),
            tools: Vec::new(),
            subagents: Vec::new(),
            traffic: Vec::new(),
            current_turn: None,
            tool_map: HashMap::new(),
            running_text: String::new(),
            running_bytes: 0,
            last_activity: Some(started_at),
            message_seq: 0,
            turn_seq: 0,
            warnings: Vec::new(),
        }
    }

    pub fn set_title(&mut self, title: String) {
        if self.session.title.is_none() {
            self.session.title = Some(title);
        }
    }

    pub fn set_cwd_hash(&mut self, cwd: &str) {
        if self.session.working_directory_hash.is_none() {
            self.session.working_directory_hash = Some(metria_core::privacy::hash_path(cwd));
        }
    }

    /// 打开（或复用）当前回合：新用户请求（非 tool_result）开新回合。
    pub fn ensure_turn(&mut self, is_new_user_prompt: bool, role: &str, at: DateTime<Utc>) -> Id {
        if is_new_user_prompt {
            return self.open_turn(role, at);
        }
        if let Some(t) = self.current_turn.clone() {
            return t;
        }
        self.open_turn(role, at)
    }

    fn open_turn(&mut self, role: &str, at: DateTime<Utc>) -> Id {
        self.turn_seq += 1;
        let turn = Turn {
            id: Id::new(),
            session_id: self.session.id.clone(),
            source_turn_id: None,
            sequence: self.turn_seq,
            role: role.to_string(),
            started_at: at,
            ended_at: None,
            provider_raw: None,
            provider_normalized: None,
            model_raw: None,
            model_normalized: None,
            finish_reason: None,
            usage_source: UsageSource::Missing,
            usage_granularity: UsageGranularity::Turn,
            usage_confidence: None,
            created_at: Utc::now(),
        };
        let id = turn.id.clone();
        self.turns.push(turn);
        self.current_turn = Some(id.clone());
        id
    }

    pub fn add_message(
        &mut self,
        turn_id: Id,
        source_message_id: Option<String>,
        role: &str,
        content_type: &str,
        content: Option<String>,
        at: DateTime<Utc>,
    ) {
        self.message_seq += 1;
        let (content_length, utf8_bytes, content_hash) = match &content {
            Some(c) => {
                let bytes = c.len() as i64;
                let hash = metria_core::privacy::hash_path(c);
                (c.chars().count() as i64, bytes, Some(hash))
            }
            None => (0, 0, None),
        };
        if let Some(c) = &content {
            if self.running_bytes < RUNNING_TEXT_CAP {
                let add = c.len().min(RUNNING_TEXT_CAP - self.running_bytes);
                self.running_text.push_str(&c[..add]);
                self.running_bytes += add;
            }
        }
        let msg = Message {
            id: Id::new(),
            turn_id: Some(turn_id),
            session_id: self.session.id.clone(),
            source_message_id,
            sequence: self.message_seq,
            role: role.to_string(),
            content_type: content_type.to_string(),
            content,
            content_hash,
            content_length,
            utf8_bytes,
            created_at: at,
            redacted: false,
        };
        self.session.message_count += 1;
        self.messages.push(msg);
        self.last_activity = Some(at);
    }

    /// 记录一次模型调用（含 UsageEvent 与 TrafficEstimate）。
    #[allow(clippy::too_many_arguments)]
    pub fn add_call(
        &mut self,
        turn_id: Id,
        source_call_id: String,
        model_raw: Option<String>,
        at: DateTime<Utc>,
        input: Option<i64>,
        output: Option<i64>,
        cache_read: Option<i64>,
        cache_write: Option<i64>,
        response_text: Option<String>,
    ) {
        let model_norm = model_raw.as_deref().map(normalize_model);
        let call = ModelCall {
            id: Id::new(),
            source_call_id: Some(source_call_id),
            node_id: self.ctx.node_id.clone(),
            collector_id: self.ctx.collector_id.clone(),
            client_id: self.ctx.client_id.clone(),
            source_id: self.ctx.source_id.clone(),
            project_id: None,
            session_id: self.session.id.clone(),
            turn_id: Some(turn_id.clone()),
            provider_raw: None,
            provider_normalized: None,
            model_raw: model_raw.clone(),
            model_normalized: model_norm.clone(),
            started_at: at,
            first_response_at: Some(at),
            completed_at: Some(at),
            duration_ms: None,
            status: "success".into(),
            status_code: Some(200),
            streaming: false,
            stream_completed: Some(true),
            client_aborted: false,
            retry_count: 0,
            call_granularity: CallGranularity::Call,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            reasoning_tokens: None,
            reported_cost_micro_usd: None,
            calculated_cost_micro_usd: None,
            estimated_cost_micro_usd: None,
            usage_event_id: None,
            traffic_estimate_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        if self.session.primary_model_raw.is_none() {
            self.session.primary_model_raw = model_raw.clone();
            self.session.primary_model_normalized = model_norm.clone();
        }

        let usage_event = UsageEvent {
            schema_version: 1,
            event_id: metria_core::model::EventId::from_content("placeholder"),
            node_id: self.ctx.node_id.clone(),
            collector_id: self.ctx.collector_id.as_str().to_string(),
            source_id: self.ctx.source_id.as_str().to_string(),
            client_id: self.ctx.client_id.clone(),
            adapter_id: "claude-code".into(),
            adapter_version: metria_adapter_api::VERSION.into(),
            session_id: Some(self.session.source_session_id.clone()),
            turn_id: Some(turn_id.as_str().to_string()),
            model_call_id: Some(call.id.as_str().to_string()),
            timestamp: at,
            provider_raw: None,
            provider_normalized: None,
            model_raw: model_raw.clone(),
            model_normalized: model_norm.clone(),
            usage: metria_core::model::Usage {
                input,
                output,
                cache_read,
                cache_write,
                reasoning: None,
            },
            cost: Default::default(),
            quality: metria_core::model::Quality {
                usage_source: "reported".into(),
                granularity: UsageGranularity::Call,
                confidence: Some(1.0),
            },
        };
        let usage_event = match usage_event.finalize() {
            Ok(e) => e,
            Err(e) => {
                self.warnings.push(format!("usage 事件校验失败: {e}"));
                return;
            }
        };

        let request_text = self.running_text.clone();
        let est_input = EstimateInput {
            client: &self.ctx.client_id,
            provider: None,
            model: model_raw.as_deref(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            reasoning_tokens: None,
            streaming: false,
            request_text: if request_text.is_empty() {
                None
            } else {
                Some(&request_text)
            },
            response_text: response_text.as_deref(),
            request_reconstruction_quality: metria_core::model::ReconstructionQuality::Partial,
            response_reconstruction_quality: metria_core::model::ReconstructionQuality::Partial,
            context_transport_mode: metria_core::model::ContextTransportMode::FullContext,
            cache_transport_behavior: metria_core::model::CacheTransportBehavior::FullContentSent,
        };

        let out = estimate(&est_input).unwrap_or_else(|e| {
            self.warnings.push(format!("流量估算失败: {e}"));
            metria_traffic::EstimateOutput {
                request_payload_bytes: None,
                response_payload_bytes: None,
                estimated_request_wire_bytes: None,
                estimated_response_wire_bytes: None,
                estimated_total_wire_bytes: None,
                lower_bound_bytes: None,
                upper_bound_bytes: None,
                estimation_source: metria_core::model::EstimationSource::Unavailable,
                confidence: None,
                notes: vec![],
            }
        });

        let traffic = TrafficEstimate {
            id: Id::new(),
            model_call_id: call.id.clone(),
            node_id: self.ctx.node_id.clone(),
            client_id: self.ctx.client_id.clone(),
            session_id: Some(self.session.id.clone()),
            turn_id: Some(turn_id),
            provider: None,
            model: model_raw.clone(),
            request_payload_bytes: out.request_payload_bytes,
            response_payload_bytes: out.response_payload_bytes,
            estimated_request_http_bytes: out.estimated_request_wire_bytes,
            estimated_response_http_bytes: out.estimated_response_wire_bytes,
            estimated_request_wire_bytes: out.estimated_request_wire_bytes,
            estimated_response_wire_bytes: out.estimated_response_wire_bytes,
            estimated_total_wire_bytes: out.estimated_total_wire_bytes,
            lower_bound_bytes: out.lower_bound_bytes,
            upper_bound_bytes: out.upper_bound_bytes,
            estimation_source: out.estimation_source,
            context_transport_mode: metria_core::model::ContextTransportMode::FullContext,
            cache_transport_behavior: metria_core::model::CacheTransportBehavior::FullContentSent,
            request_reconstruction_quality: metria_core::model::ReconstructionQuality::Partial,
            response_reconstruction_quality: metria_core::model::ReconstructionQuality::Partial,
            profile_id: None,
            profile_version: None,
            confidence: out.confidence,
            calculated_at: Utc::now(),
            created_at: Utc::now(),
        };

        let traffic_id = traffic.id.clone();
        let usage_event_id = usage_event.event_id.as_str().to_string();
        let mut c = call;
        c.usage_event_id = Some(usage_event_id);
        c.traffic_estimate_id = Some(traffic_id);

        self.session.model_call_count += 1;
        self.session.input_tokens = sum_opt(self.session.input_tokens, input);
        self.session.output_tokens = sum_opt(self.session.output_tokens, output);
        self.session.cache_read_tokens = sum_opt(self.session.cache_read_tokens, cache_read);
        self.session.cache_write_tokens = sum_opt(self.session.cache_write_tokens, cache_write);
        self.session.estimated_request_bytes = sum_opt(
            self.session.estimated_request_bytes,
            out.estimated_request_wire_bytes,
        );
        self.session.estimated_response_bytes = sum_opt(
            self.session.estimated_response_bytes,
            out.estimated_response_wire_bytes,
        );
        self.session.estimated_total_bytes = sum_opt(
            self.session.estimated_total_bytes,
            out.estimated_total_wire_bytes,
        );
        self.session.traffic_confidence = Some(
            self.session
                .traffic_confidence
                .unwrap_or(0.0)
                .max(out.confidence.unwrap_or(0.0)),
        );
        self.session.content_available = true;

        self.usage.push(usage_event);
        self.calls.push(c);
        self.traffic.push(traffic);
        self.last_activity = Some(at);
    }

    /// 记录 tool_use；返回是否新记录。
    pub fn add_tool_use(
        &mut self,
        tool_use_id: String,
        name: String,
        input: Option<&serde_json::Value>,
        at: DateTime<Utc>,
    ) {
        let input_len = input.map(|v| v.to_string().len() as i64).unwrap_or(0);
        let tool = ToolEvent {
            id: Id::new(),
            session_id: self.session.id.clone(),
            model_call_id: None,
            turn_id: None,
            source_tool_id: Some(tool_use_id.clone()),
            name: name.clone(),
            tool_type: name.clone(),
            status: "running".into(),
            input_content_hash: input.map(|v| metria_core::privacy::hash_path(&v.to_string())),
            output_content_hash: None,
            input_length: input_len,
            output_length: 0,
            started_at: at,
            completed_at: None,
            duration_ms: None,
            error: None,
            created_at: Utc::now(),
        };
        let idx = self.tools.len();
        self.tools.push(tool);
        self.session.tool_call_count += 1;
        self.tool_map.insert(tool_use_id, idx);
    }

    /// 回填 tool_result。
    pub fn complete_tool_result(
        &mut self,
        tool_use_id: &str,
        is_error: bool,
        content: Option<&serde_json::Value>,
        at: DateTime<Utc>,
    ) {
        if let Some(idx) = self.tool_map.get(tool_use_id).copied() {
            if let Some(tool) = self.tools.get_mut(idx) {
                tool.completed_at = Some(at);
                tool.status = if is_error { "error" } else { "success" }.into();
                tool.error = if is_error {
                    Some("tool 返回错误".into())
                } else {
                    None
                };
                tool.output_content_hash =
                    content.map(|v| metria_core::privacy::hash_path(&v.to_string()));
                tool.output_length = content.map(|v| v.to_string().len() as i64).unwrap_or(0);
            }
        }
    }

    /// 结束构建。
    #[allow(clippy::type_complexity)]
    pub fn finish(
        mut self,
    ) -> (
        Session,
        Vec<Turn>,
        Vec<Message>,
        Vec<ModelCall>,
        Vec<UsageEvent>,
        Vec<ToolEvent>,
        Vec<SubagentRelation>,
        Vec<TrafficEstimate>,
    ) {
        self.session.ended_at = self.last_activity;
        if self.session.model_call_count > 0 {
            self.session.status = SessionStatus::Ended;
        }
        for t in &mut self.turns {
            if t.ended_at.is_none() {
                t.ended_at = self.last_activity;
            }
        }
        (
            self.session,
            self.turns,
            self.messages,
            self.calls,
            self.usage,
            self.tools,
            self.subagents,
            self.traffic,
        )
    }
}

fn sum_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

/// 从 RawEntry 解析事件时间。
pub fn entry_time(entry: &RawEntry) -> Option<DateTime<Utc>> {
    entry.timestamp.as_deref().and_then(parse_ts)
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// 构建 JSONL 游标。
pub fn jsonl_cursor(
    path_hash: metria_core::model::ContentHash,
    inode: i64,
    size: i64,
    mtime: i64,
    offset: i64,
) -> SourceCursor {
    use metria_core::model::JsonlCursor;
    SourceCursor::Jsonl(JsonlCursor {
        canonical_path_hash: path_hash,
        file_identity: format!("inode:{inode}"),
        inode,
        size,
        mtime,
        byte_offset: offset,
        last_event_hash: None,
        last_scan_at: Some(Utc::now()),
    })
}
