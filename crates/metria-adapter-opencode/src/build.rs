//! OpenCode 会话构建器。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use metria_core::model::{
    CacheTransportBehavior, CallGranularity, ContextTransportMode, Id, Message, ModelCall, Session,
    SessionStatus, SubagentRelation, ToolEvent, TrafficEstimate, Turn, UsageEvent,
    UsageGranularity, UsageSource,
};
use metria_core::normalize::normalize_model;
use metria_traffic::{estimate, EstimateInput};

/// 会话构建上下文。
#[derive(Debug, Clone)]
pub struct BuildCtx {
    pub node_id: String,
    pub collector_id: Id,
    pub source_id: Id,
    pub client_id: String,
}

/// 累积正文上限。
const RUNNING_TEXT_CAP: usize = 512 * 1024;

/// OpenCode 会话构建状态。
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
    pub parent_source_id: Option<String>,
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
            parent_source_id: None,
            tool_map: HashMap::new(),
            running_text: String::new(),
            running_bytes: 0,
            last_activity: Some(started_at),
            message_seq: 0,
            turn_seq: 0,
            warnings: Vec::new(),
        }
    }

    /// 设置会话元数据。
    #[allow(clippy::too_many_arguments)]
    pub fn set_meta(
        &mut self,
        title: Option<String>,
        directory: Option<String>,
        project_id: Option<String>,
        model: Option<String>,
        provider: Option<String>,
        reported_cost_micro_usd: Option<i64>,
        parent_source_id: Option<String>,
    ) {
        if self.session.title.is_none() {
            self.session.title = title;
        }
        if self.session.working_directory_hash.is_none() {
            if let Some(d) = directory {
                self.session.working_directory_hash = Some(metria_core::privacy::hash_path(&d));
            }
        }
        if self.session.project_id.is_none() {
            self.session.project_id = project_id;
        }
        if self.session.primary_model_raw.is_none() {
            self.session.primary_model_raw = model.clone();
            self.session.primary_model_normalized = model.as_deref().map(normalize_model);
        }
        if self.session.provider_raw.is_none() {
            self.session.provider_raw = provider.clone();
            self.session.provider_normalized = provider
                .as_deref()
                .map(metria_core::normalize::normalize_provider);
        }
        if self.session.reported_cost_micro_usd.is_none() {
            self.session.reported_cost_micro_usd = reported_cost_micro_usd;
        }
        if self.parent_source_id.is_none() {
            self.parent_source_id = parent_source_id;
        }
    }

    /// 用户消息 → 开新回合。
    pub fn new_turn(&mut self, at: DateTime<Utc>) -> Id {
        self.turn_seq += 1;
        let turn = Turn {
            id: Id::new(),
            session_id: self.session.id.clone(),
            source_turn_id: None,
            sequence: self.turn_seq,
            role: "user".into(),
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

    pub fn ensure_turn(&mut self, at: DateTime<Utc>) -> Id {
        if let Some(t) = self.current_turn.clone() {
            return t;
        }
        self.turn_seq += 1;
        let turn = Turn {
            id: Id::new(),
            session_id: self.session.id.clone(),
            source_turn_id: None,
            sequence: self.turn_seq,
            role: "assistant".into(),
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
        role: &str,
        content_type: &str,
        content: Option<String>,
        at: DateTime<Utc>,
    ) {
        self.message_seq += 1;
        let (content_length, utf8_bytes, content_hash) = match &content {
            Some(c) => {
                let bytes = c.len() as i64;
                (
                    c.chars().count() as i64,
                    bytes,
                    Some(metria_core::privacy::hash_path(c)),
                )
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
        self.session.message_count += 1;
        self.messages.push(Message {
            id: Id::new(),
            turn_id: Some(turn_id),
            session_id: self.session.id.clone(),
            source_message_id: None,
            sequence: self.message_seq,
            role: role.to_string(),
            content_type: content_type.to_string(),
            content,
            content_hash,
            content_length,
            utf8_bytes,
            created_at: at,
            redacted: false,
        });
        self.last_activity = Some(at);
    }

    /// 记录一次模型调用（assistant 消息带 tokens）。
    #[allow(clippy::too_many_arguments)]
    pub fn add_call(
        &mut self,
        turn_id: Id,
        source_call_id: String,
        at: DateTime<Utc>,
        model: Option<&str>,
        provider: Option<&str>,
        input: Option<i64>,
        output: Option<i64>,
        cache_read: Option<i64>,
        cache_write: Option<i64>,
        reasoning: Option<i64>,
        response_text: Option<String>,
    ) {
        let model_norm = model.map(normalize_model);
        let call = ModelCall {
            id: Id::new(),
            source_call_id: Some(source_call_id),
            node_id: self.ctx.node_id.clone(),
            collector_id: self.ctx.collector_id.clone(),
            client_id: self.ctx.client_id.clone(),
            source_id: self.ctx.source_id.clone(),
            project_id: self.session.project_id.clone(),
            session_id: self.session.id.clone(),
            turn_id: Some(turn_id.clone()),
            provider_raw: provider.map(|s| s.to_string()),
            provider_normalized: provider.map(metria_core::normalize::normalize_provider),
            model_raw: model.map(|s| s.to_string()),
            model_normalized: model_norm.clone(),
            started_at: at,
            first_response_at: Some(at),
            completed_at: Some(at),
            duration_ms: None,
            status: "success".into(),
            status_code: Some(200),
            streaming: true,
            stream_completed: Some(true),
            client_aborted: false,
            retry_count: 0,
            call_granularity: CallGranularity::Message,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            reasoning_tokens: reasoning,
            reported_cost_micro_usd: None,
            calculated_cost_micro_usd: None,
            estimated_cost_micro_usd: None,
            usage_event_id: None,
            traffic_estimate_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let usage_event = UsageEvent {
            schema_version: 1,
            event_id: metria_core::model::EventId::from_content("placeholder"),
            node_id: self.ctx.node_id.clone(),
            collector_id: self.ctx.collector_id.as_str().to_string(),
            source_id: self.ctx.source_id.as_str().to_string(),
            client_id: self.ctx.client_id.clone(),
            adapter_id: "opencode".into(),
            adapter_version: metria_adapter_api::VERSION.into(),
            session_id: Some(self.session.source_session_id.clone()),
            turn_id: Some(turn_id.as_str().to_string()),
            model_call_id: Some(call.id.as_str().to_string()),
            timestamp: at,
            provider_raw: call.provider_raw.clone(),
            provider_normalized: call.provider_normalized.clone(),
            model_raw: call.model_raw.clone(),
            model_normalized: model_norm.clone(),
            usage: metria_core::model::Usage {
                input,
                output,
                cache_read,
                cache_write,
                reasoning,
            },
            cost: Default::default(),
            quality: metria_core::model::Quality {
                usage_source: "reported".into(),
                granularity: UsageGranularity::Message,
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
            provider,
            model,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            reasoning_tokens: reasoning,
            streaming: true,
            request_text: if request_text.is_empty() {
                None
            } else {
                Some(&request_text)
            },
            response_text: response_text.as_deref(),
            request_reconstruction_quality: metria_core::model::ReconstructionQuality::Partial,
            response_reconstruction_quality: metria_core::model::ReconstructionQuality::Partial,
            context_transport_mode: ContextTransportMode::FullContext,
            cache_transport_behavior: CacheTransportBehavior::FullContentSent,
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
            provider: call.provider_raw.clone(),
            model: call.model_raw.clone(),
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
            context_transport_mode: ContextTransportMode::FullContext,
            cache_transport_behavior: CacheTransportBehavior::FullContentSent,
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
        self.session.reasoning_tokens = sum_opt(self.session.reasoning_tokens, reasoning);
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

    pub fn add_tool_use(
        &mut self,
        call_id: String,
        name: String,
        input: Option<&serde_json::Value>,
        output: Option<&serde_json::Value>,
        status: &str,
        at: DateTime<Utc>,
    ) {
        let input_len = input.map(|v| v.to_string().len() as i64).unwrap_or(0);
        let output_len = output.map(|v| v.to_string().len() as i64).unwrap_or(0);
        let tool = ToolEvent {
            id: Id::new(),
            session_id: self.session.id.clone(),
            model_call_id: None,
            turn_id: None,
            source_tool_id: Some(call_id.clone()),
            name: name.clone(),
            tool_type: name,
            status: status.to_string(),
            input_content_hash: input.map(|v| metria_core::privacy::hash_path(&v.to_string())),
            output_content_hash: output.map(|v| metria_core::privacy::hash_path(&v.to_string())),
            input_length: input_len,
            output_length: output_len,
            started_at: at,
            completed_at: Some(at),
            duration_ms: None,
            error: None,
            created_at: Utc::now(),
        };
        let idx = self.tools.len();
        self.tools.push(tool);
        self.session.tool_call_count += 1;
        self.tool_map.insert(call_id, idx);
    }

    pub fn add_subagent(&mut self, parent_session_id: Id, child_session_id: Id) {
        self.session.subagent_count += 1;
        self.subagents.push(SubagentRelation {
            id: Id::new(),
            session_id: parent_session_id,
            parent_model_call_id: None,
            child_session_id,
            relation: "subagent".into(),
            created_at: Utc::now(),
        });
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

/// 构建 SQLite 游标。
pub fn sqlite_cursor(
    database_fingerprint: metria_core::model::ContentHash,
    schema_version: Option<String>,
    last_rowid: i64,
) -> metria_core::model::SourceCursor {
    use metria_core::model::SqliteCursor;
    metria_core::model::SourceCursor::Sqlite(SqliteCursor {
        database_fingerprint,
        schema_version,
        table_name: "message".into(),
        last_rowid,
        last_updated_at: None,
        last_primary_key: None,
        last_scan_at: Some(Utc::now()),
    })
}
