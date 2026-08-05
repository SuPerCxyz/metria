//! 扫描器：发现来源 → 增量扫描 → 归一化 → 定价 → 写 Spool。

use metria_adapter_api::{
    DiscoveredSource, DiscoveryContext, ScanBatch, ScanIdentity, SourceAdapter,
};
use metria_core::{
    config::ContentMode,
    model::{EventId, SourceCursor},
};
use metria_pricing::PricingEngine;

use crate::config::AgentConfig;
use crate::error::{AgentError, Result};
use crate::spool::{CursorUpdate, PendingEvent, Spool};

/// 扫描汇总。
#[derive(Debug, Default)]
pub struct ScanTotals {
    pub sessions: usize,
    pub calls: usize,
    pub usage: usize,
    pub traffic: usize,
    pub sources: usize,
    pub errors: usize,
    pub skipped_full: usize,
}

/// 扫描器。
pub struct Scanner {
    adapters: Vec<(&'static str, Box<dyn SourceAdapter>)>,
    identity: ScanIdentity,
    pricing: PricingEngine,
    content_mode: ContentMode,
    cfg: AgentConfig,
}

impl std::fmt::Debug for Scanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scanner")
            .field(
                "adapters",
                &self.adapters.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            )
            .field("node", &self.identity.node_id)
            .field("content_mode", &self.content_mode)
            .finish()
    }
}

impl Scanner {
    pub fn new(cfg: AgentConfig, identity: ScanIdentity) -> Self {
        let adapters: Vec<(&'static str, Box<dyn SourceAdapter>)> = vec![
            (
                "claude-code",
                Box::new(metria_adapter_claude::ClaudeCodeAdapter),
            ),
            ("codex", Box::new(metria_adapter_codex::CodexAdapter)),
            (
                "opencode",
                Box::new(metria_adapter_opencode::OpenCodeAdapter),
            ),
        ];
        Self {
            adapters,
            identity,
            pricing: PricingEngine::new(),
            content_mode: cfg.content_mode,
            cfg,
        }
    }

    /// 全量扫描一次（发现 + 增量扫描所有来源）。
    pub fn scan_all(&self, spool: &mut Spool) -> ScanTotals {
        let mut totals = ScanTotals::default();
        for (client, adapter) in &self.adapters {
            let Some(root) = self.cfg.client_root(client) else {
                continue;
            };
            if !root.is_dir() {
                continue;
            }
            let ctx = DiscoveryContext {
                node_id: self.identity.node_id.clone(),
                collector_id: self.identity.collector_id.clone(),
                root_paths: vec![root],
            };
            let sources = match adapter.discover(&ctx) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("{client} 发现失败: {e}");
                    continue;
                }
            };
            for source in sources {
                let t = self.scan_source(spool, adapter.as_ref(), &source);
                totals.sources += 1;
                match t {
                    Ok(st) => {
                        totals.sessions += st.sessions;
                        totals.calls += st.calls;
                        totals.usage += st.usage;
                        totals.traffic += st.traffic;
                        if st.skipped_full {
                            totals.skipped_full += 1;
                        }
                    }
                    Err(e) => {
                        totals.errors += 1;
                        tracing::warn!("扫描 {} 失败: {e}", source.canonical_path.display());
                    }
                }
            }
        }
        totals
    }

    fn scan_source(
        &self,
        spool: &mut Spool,
        adapter: &dyn SourceAdapter,
        source: &DiscoveredSource,
    ) -> Result<SourceScan> {
        let source_id = source.path_hash.as_str().to_string();
        // 来源注册事件（不含完整路径，仅指纹/哈希）
        let src_event = PendingEvent {
            event_id: EventId::from_content(&format!("source:{source_id}"))
                .as_str()
                .to_string(),
            kind: "source".into(),
            payload: serde_json::json!({
                "id": source_id,
                "node_id": self.identity.node_id,
                "collector_id": self.identity.collector_id,
                "client_id": source.adapter_id,
                "adapter_id": source.adapter_id,
                "adapter_version": adapter.version(),
                "source_fingerprint": source.source_fingerprint,
                "source_path_hash": source.path_hash,
                "client_version": source.client_version,
                "capabilities": source.capabilities,
                "status": "active",
            }),
        };
        let cursor_json = spool.get_cursor(&source_id);
        let cursor: Option<SourceCursor> = cursor_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok());

        let batch = adapter.scan(source, cursor.as_ref(), &self.identity)?;
        let events = normalize_batch(&batch, self.content_mode, &self.pricing, source_id.as_str());

        let cursor_update = match &batch.next_cursor {
            Some(c) => vec![CursorUpdate {
                source_id: source_id.clone(),
                cursor_json: serde_json::to_string(c)
                    .map_err(|e| AgentError::Serde(e.to_string()))?,
            }],
            None => vec![],
        };
        let mut all_events = Vec::with_capacity(events.len() + 1);
        all_events.push(src_event);
        all_events.extend(events);
        let ok = spool.insert_batch(&all_events, &cursor_update)?;
        spool.update_source_health(&source_id, true, None, &source.adapter_id)?;
        let skipped_full = !ok;

        Ok(SourceScan {
            sessions: batch.sessions.len(),
            calls: batch.model_calls.len(),
            usage: batch.usage_events.len(),
            traffic: batch.traffic_estimates.len(),
            skipped_full,
        })
    }
}

/// 单来源扫描结果。
#[derive(Debug, Default)]
pub struct SourceScan {
    pub sessions: usize,
    pub calls: usize,
    pub usage: usize,
    pub traffic: usize,
    pub skipped_full: bool,
}

/// 将 ScanBatch 归一化为待上传事件（含定价与隐私处理）。
pub fn normalize_batch(
    batch: &ScanBatch,
    content_mode: ContentMode,
    pricing: &PricingEngine,
    source_id: &str,
) -> Vec<PendingEvent> {
    let mut out = Vec::new();

    for s in &batch.sessions {
        let event_id =
            EventId::from_content(&format!("session:{}:{}", s.source_session_id, s.node_id));
        out.push(PendingEvent {
            event_id: event_id.as_str().to_string(),
            kind: "session".into(),
            payload: serde_json::to_value(s).unwrap_or_default(),
        });
    }

    for m in &batch.messages {
        let event_id = EventId::from_content(&format!("message:{}", m.id.as_str()));
        let mut payload = serde_json::to_value(m).unwrap_or_default();
        // 隐私：按 content_mode 剥离或脱敏正文
        match content_mode {
            ContentMode::Full => {
                if let Some(c) = payload.get_mut("content").and_then(|c| c.as_str()) {
                    let redacted = metria_core::privacy::redact_text(c);
                    payload["content"] = serde_json::Value::String(redacted);
                }
            }
            ContentMode::Metadata | ContentMode::None => {
                payload["content"] = serde_json::Value::Null;
                payload["redacted"] = serde_json::Value::Bool(true);
            }
        }
        out.push(PendingEvent {
            event_id: event_id.as_str().to_string(),
            kind: "message".into(),
            payload,
        });
    }

    for c in &batch.model_calls {
        let event_id = EventId::from_content(&format!("call:{}", c.id.as_str()));
        out.push(PendingEvent {
            event_id: event_id.as_str().to_string(),
            kind: "call".into(),
            payload: serde_json::to_value(c).unwrap_or_default(),
        });
    }

    for u in &batch.usage_events {
        // 定价：reported 优先，否则按规则计算
        let cost = pricing
            .compute(
                &u.usage,
                u.model_normalized.as_deref(),
                u.provider_normalized.as_deref(),
                u.timestamp,
                u.cost.reported_micro_usd,
            )
            .unwrap_or_default();
        let mut payload = serde_json::to_value(u).unwrap_or_default();
        payload["cost"]["reported_micro_usd"] = cost
            .reported_micro_usd
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null);
        payload["cost"]["calculated_micro_usd"] = cost
            .calculated_micro_usd
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null);
        payload["cost"]["estimated_micro_usd"] = cost
            .estimated_micro_usd
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null);
        payload["cost"]["pricing_rule_id"] = cost
            .rule_id
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null);
        out.push(PendingEvent {
            event_id: u.event_id.as_str().to_string(),
            kind: "usage".into(),
            payload,
        });
    }

    for t in &batch.traffic_estimates {
        let event_id = EventId::from_content(&format!("traffic:{}", t.id.as_str()));
        out.push(PendingEvent {
            event_id: event_id.as_str().to_string(),
            kind: "traffic".into(),
            payload: serde_json::to_value(t).unwrap_or_default(),
        });
    }

    // Traffic 自动学习样本：调用同时有 token 与 payload 字节时生成
    for s in &batch.traffic_estimates {
        let Some(call) = batch.model_calls.iter().find(|c| c.id == s.model_call_id) else {
            continue;
        };
        if let (Some(in_tok), Some(req_bytes)) = (call.input_tokens, s.request_payload_bytes) {
            // 仅从完整重建生成学习样本（partial 重建会系统性低估字节）
            if in_tok > 0
                && req_bytes > 0
                && s.request_reconstruction_quality
                    == metria_core::model::ReconstructionQuality::Complete
            {
                let bpt = (req_bytes as f64 / in_tok as f64 * 100.0).round() / 100.0;
                let event_id = EventId::from_content(&format!(
                    "tps:{client}|{provider:?}|{model:?}|request|{in_tok}|{req_bytes}",
                    client = s.client_id,
                    provider = s.provider,
                    model = s.model,
                ));
                out.push(PendingEvent {
                    event_id: event_id.as_str().to_string(),
                    kind: "traffic_sample".into(),
                    payload: serde_json::json!({
                        "id": event_id.as_str(),
                        "client": s.client_id,
                        "provider": s.provider,
                        "model": s.model,
                        "content_profile": "unknown",
                        "direction": "request",
                        "token_count": in_tok,
                        "payload_bytes": req_bytes,
                        "bytes_per_token": bpt,
                        "reconstruction_quality": format!("{:?}", s.request_reconstruction_quality).to_ascii_lowercase(),
                        "source_hash": event_id.as_str(),
                    }),
                });
            }
        }
        if let (Some(out_tok), Some(resp_bytes)) = (call.output_tokens, s.response_payload_bytes) {
            if out_tok > 0
                && resp_bytes > 0
                && s.response_reconstruction_quality
                    == metria_core::model::ReconstructionQuality::Complete
            {
                let bpt = (resp_bytes as f64 / out_tok as f64 * 100.0).round() / 100.0;
                let event_id = EventId::from_content(&format!(
                    "tps:{client}|{provider:?}|{model:?}|response|{out_tok}|{resp_bytes}",
                    client = s.client_id,
                    provider = s.provider,
                    model = s.model,
                ));
                out.push(PendingEvent {
                    event_id: event_id.as_str().to_string(),
                    kind: "traffic_sample".into(),
                    payload: serde_json::json!({
                        "id": event_id.as_str(),
                        "client": s.client_id,
                        "provider": s.provider,
                        "model": s.model,
                        "content_profile": "unknown",
                        "direction": "response",
                        "token_count": out_tok,
                        "payload_bytes": resp_bytes,
                        "bytes_per_token": bpt,
                        "reconstruction_quality": format!("{:?}", s.response_reconstruction_quality).to_ascii_lowercase(),
                        "source_hash": event_id.as_str(),
                    }),
                });
            }
        }
    }

    for t in &batch.tool_events {
        let event_id = EventId::from_content(&format!("tool:{}", t.id.as_str()));
        out.push(PendingEvent {
            event_id: event_id.as_str().to_string(),
            kind: "tool".into(),
            payload: serde_json::to_value(t).unwrap_or_default(),
        });
    }

    for r in &batch.subagent_relations {
        let event_id = EventId::from_content(&format!("subagent:{}", r.id.as_str()));
        out.push(PendingEvent {
            event_id: event_id.as_str().to_string(),
            kind: "subagent".into(),
            payload: serde_json::to_value(r).unwrap_or_default(),
        });
    }

    // source_id 附注（用于 hub 关联；payload 内已有）
    let _ = source_id;
    out
}
