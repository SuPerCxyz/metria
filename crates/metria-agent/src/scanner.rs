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
pub use crate::spool::PendingEvent;
use crate::spool::{CursorUpdate, Spool};

/// 扫描汇总。
#[derive(Debug, Default)]
pub struct ScanTotals {
    pub sessions: usize,
    pub calls: usize,
    pub usage: usize,
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
        // 根目录缺失只会静默跳过；这里显式警告，避免配置丢失导致空转无感知
        let missing: Vec<&str> = adapters
            .iter()
            .filter(|(client, _)| cfg.client_root(client).is_none())
            .map(|(client, _)| *client)
            .collect();
        for client in &missing {
            let var = match *client {
                "claude-code" => "METRIA_CLAUDE_PATH",
                "codex" => "METRIA_CODEX_PATH",
                "opencode" => "METRIA_OPENCODE_PATH",
                _ => "对应 METRIA_*_PATH",
            };
            tracing::warn!("未配置 {client} 采集目录（{var}），将跳过该客户端采集");
        }
        if missing.len() == adapters.len() {
            tracing::error!(
                "未配置任何采集目录（{missing:?}），Agent 不会采集任何数据，请检查 METRIA_*_PATH 环境变量"
            );
        }
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
        let cursor_json = spool.get_cursor(&source_id);
        let cursor: Option<SourceCursor> = cursor_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok());

        let (events, next_cursor) = self.scan_source_events(adapter, source, cursor.as_ref())?;

        let cursor_update = match &next_cursor {
            Some(c) => vec![CursorUpdate {
                source_id: source_id.clone(),
                cursor_json: serde_json::to_string(c)
                    .map_err(|e| AgentError::Serde(e.to_string()))?,
            }],
            None => vec![],
        };
        let ok = spool.insert_batch(&events, &cursor_update)?;
        spool.update_source_health(&source_id, true, None, &source.adapter_id)?;
        let skipped_full = !ok;

        Ok(SourceScan {
            sessions: events.iter().filter(|e| e.kind == "session").count(),
            calls: events.iter().filter(|e| e.kind == "call").count(),
            usage: events.iter().filter(|e| e.kind == "usage").count(),
            skipped_full,
        })
    }

    /// 无状态采集：按给定游标扫描并归一化（不写本地 spool）。
    ///
    /// 返回 (待上传事件[含 source 注册事件], 下一游标)。
    pub fn scan_source_events(
        &self,
        adapter: &dyn SourceAdapter,
        source: &DiscoveredSource,
        cursor: Option<&SourceCursor>,
    ) -> Result<(Vec<PendingEvent>, Option<SourceCursor>)> {
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

        let batch = adapter.scan(source, cursor, &self.identity)?;
        let events = normalize_batch(&batch, self.content_mode, &self.pricing, source_id.as_str());
        let mut all_events = Vec::with_capacity(events.len() + 1);
        all_events.push(src_event);
        all_events.extend(events);
        Ok((all_events, batch.next_cursor))
    }

    /// 发现某客户端的全部 Source（无状态轮询用）；客户端未配置或发现失败返回 None。
    pub fn discover_sources(
        &self,
        client: &str,
        adapter: &dyn SourceAdapter,
    ) -> Option<Vec<DiscoveredSource>> {
        let root = self.cfg.client_root(client)?;
        if !root.is_dir() {
            return None;
        }
        let ctx = DiscoveryContext {
            node_id: self.identity.node_id.clone(),
            collector_id: self.identity.collector_id.clone(),
            root_paths: vec![root],
        };
        match adapter.discover(&ctx) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("{client} 发现失败: {e}");
                None
            }
        }
    }

    /// 遍历已装配的适配器（无状态轮询用）。
    pub fn iter_adapters(&self) -> impl Iterator<Item = (&'static str, &dyn SourceAdapter)> {
        self.adapters.iter().map(|(n, a)| (*n, a.as_ref()))
    }
}

/// 单来源扫描结果。
#[derive(Debug, Default)]
pub struct SourceScan {
    pub sessions: usize,
    pub calls: usize,
    pub usage: usize,
    pub skipped_full: bool,
}

/// 将 ScanBatch 归一化为待上传事件（含定价）。
///
/// 精简模式：只上传 session 概要（聚合计数，无正文）、call、usage；
/// 不采集 message/tool/subagent 等会话详细内容。
pub fn normalize_batch(
    batch: &ScanBatch,
    _content_mode: ContentMode,
    pricing: &PricingEngine,
    source_id: &str,
) -> Vec<PendingEvent> {
    let mut out = Vec::new();

    for s in &batch.sessions {
        let event_id =
            EventId::from_content(&format!("session:{}:{}", s.source_session_id, s.node_id));
        let mut payload = serde_json::to_value(s).unwrap_or_default();
        // 历史 Session 模型仍保留估算流量列以兼容旧库，但新 Agent 不再
        // 计算、上传或展示这些字段。
        clear_legacy_traffic_fields(&mut payload);
        out.push(PendingEvent {
            event_id: event_id.as_str().to_string(),
            kind: "session".into(),
            payload,
        });
    }

    for c in &batch.model_calls {
        let event_id = EventId::from_content(&format!("call:{}", c.id.as_str()));
        let mut payload = serde_json::to_value(c).unwrap_or_default();
        clear_legacy_traffic_fields(&mut payload);
        out.push(PendingEvent {
            event_id: event_id.as_str().to_string(),
            kind: "call".into(),
            payload,
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

    // source_id 附注（用于 hub 关联；payload 内已有）
    let _ = source_id;
    out
}

fn clear_legacy_traffic_fields(payload: &mut serde_json::Value) {
    for key in [
        "estimated_request_bytes",
        "estimated_response_bytes",
        "estimated_total_bytes",
        "traffic_confidence",
        "traffic_estimate_id",
    ] {
        payload[key] = serde_json::Value::Null;
    }
}

#[cfg(test)]
mod tests {
    use super::clear_legacy_traffic_fields;
    use serde_json::json;

    #[test]
    fn ordinary_agent_never_emits_estimated_traffic_fields() {
        let mut payload = json!({
            "estimated_request_bytes": 1,
            "estimated_response_bytes": 2,
            "estimated_total_bytes": 3,
            "traffic_confidence": 0.9,
            "traffic_estimate_id": "legacy"
        });
        clear_legacy_traffic_fields(&mut payload);
        for key in [
            "estimated_request_bytes",
            "estimated_response_bytes",
            "estimated_total_bytes",
            "traffic_confidence",
            "traffic_estimate_id",
        ] {
            assert!(payload[key].is_null(), "{key} must be null");
        }
    }
}
