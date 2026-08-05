//! Adapter 接口与核心类型。

use std::path::PathBuf;

use metria_core::model::{
    CacheTransportBehavior, ContentHash, ContextTransportMode, Message, ModelCall,
    ReconstructionQuality, Session, SourceCursor, SourceError, SourceStatus, SubagentRelation,
    ToolEvent, TrafficEstimate, TrafficProfileSample, Turn, UsageEvent,
};

use crate::error::AdapterError;

/// 发现上下文：节点信息与客户端挂载根路径。
#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub node_id: String,
    /// 采集器 ID（由 Agent 提供，注册后稳定）。
    pub collector_id: String,
    /// 该客户端允许探测的根路径（如 /sources/claude）。
    pub root_paths: Vec<PathBuf>,
}

/// 扫描身份：node 与 collector 的稳定标识，写入扫描产出的事件。
#[derive(Debug, Clone)]
pub struct ScanIdentity {
    pub node_id: String,
    pub collector_id: String,
}

impl ScanIdentity {
    pub fn test() -> Self {
        Self {
            node_id: "test-node".into(),
            collector_id: "test-collector".into(),
        }
    }
}

/// 由字符串种子生成确定性 Id（用于 source/collector 伪 ID）。
pub fn pseudo_id(seed: &str) -> metria_core::model::Id {
    metria_core::model::Id::parse(metria_core::privacy::hash_path(seed).as_str())
        .unwrap_or_default()
}

/// 发现到的数据源。
#[derive(Debug, Clone)]
pub struct DiscoveredSource {
    pub adapter_id: String,
    /// 稳定指纹（用于识别同一来源，如目录路径规范化）。
    pub source_fingerprint: String,
    /// 规范化后的完整路径（仅本地使用，上传前哈希）。
    pub canonical_path: PathBuf,
    pub path_hash: ContentHash,
    pub client_version: Option<String>,
    pub capabilities: Vec<String>,
}

/// 一次扫描的增量结果。
#[derive(Debug, Default)]
pub struct ScanBatch {
    pub sessions: Vec<Session>,
    pub turns: Vec<Turn>,
    pub messages: Vec<Message>,
    pub model_calls: Vec<ModelCall>,
    pub usage_events: Vec<UsageEvent>,
    pub tool_events: Vec<ToolEvent>,
    pub subagent_relations: Vec<SubagentRelation>,
    pub traffic_estimates: Vec<TrafficEstimate>,
    pub traffic_profile_samples: Vec<TrafficProfileSample>,
    /// 下一次扫描的游标；为 None 表示本次未消费完毕或来源已结束
    pub next_cursor: Option<SourceCursor>,
    pub warnings: Vec<String>,
    pub source_errors: Vec<SourceError>,
}

/// Adapter 能力声明。
#[derive(Debug, Clone, Default)]
pub struct AdapterCapabilities {
    pub session_usage: bool,
    pub call_usage: bool,
    pub turn_usage: bool,
    pub message_usage: bool,
    pub message_content: bool,
    pub tool_calls: bool,
    pub tool_results: bool,
    pub subagents: bool,
    pub project_path: bool,
    pub reported_cost: bool,
    pub model_switching: bool,
    pub reasoning_tokens: bool,
    pub cache_tokens: bool,
    pub request_reconstruction: bool,
    pub response_reconstruction: bool,
    pub context_transport_detection: bool,
}

/// 流量估算能力声明。
#[derive(Debug, Clone)]
pub struct TrafficCapabilities {
    pub context_transport_mode: ContextTransportMode,
    pub cache_transport_behavior: CacheTransportBehavior,
    pub request_reconstruction_quality: ReconstructionQuality,
    pub response_reconstruction_quality: ReconstructionQuality,
}

/// 来源健康状态。
#[derive(Debug, Clone)]
pub struct SourceHealth {
    pub ok: bool,
    pub status: SourceStatus,
    pub message: Option<String>,
    pub last_error: Option<String>,
}

/// 客户端数据源 Adapter 接口。
pub trait SourceAdapter: Send + Sync {
    /// 稳定 ID，如 `claude-code`。
    fn id(&self) -> &'static str;

    /// 展示名，如 `Claude Code`。
    fn display_name(&self) -> &'static str;

    /// Adapter 版本。
    fn version(&self) -> &'static str;

    /// 能力声明。
    fn capabilities(&self) -> AdapterCapabilities;

    /// 在根路径下发现数据源。
    fn discover(&self, context: &DiscoveryContext) -> Result<Vec<DiscoveredSource>, AdapterError>;

    /// 增量扫描：从游标处解析新数据。
    ///
    /// 实现要求：容忍未知字段与坏记录（warning + continue），
    /// 不得把完整大文件加载进内存，不得修改客户端文件。
    fn scan(
        &self,
        source: &DiscoveredSource,
        cursor: Option<&SourceCursor>,
        identity: &ScanIdentity,
    ) -> Result<ScanBatch, AdapterError>;

    /// 来源健康检查（路径存在、可读、数据库可用等）。
    fn health(&self, source: &DiscoveredSource) -> Result<SourceHealth, AdapterError>;

    /// 来源的流量估算能力（协议与内容可得性相关）。
    fn traffic_capabilities(&self, source: &DiscoveredSource) -> TrafficCapabilities;
}

/// 便捷的默认实现：提供统一的已发现来源结构构造。
pub mod discover {
    use super::*;

    /// 构建 DiscoveredSource（自动生成路径哈希）。
    pub fn source(
        adapter_id: &str,
        canonical_path: PathBuf,
        client_version: Option<String>,
        capabilities: Vec<String>,
    ) -> DiscoveredSource {
        let canonical = canonical_path.canonicalize().unwrap_or(canonical_path);
        let path_str = canonical.to_string_lossy().to_string();
        DiscoveredSource {
            adapter_id: adapter_id.to_string(),
            source_fingerprint: format!(
                "{adapter_id}:{}",
                metria_core::privacy::hash_path(&path_str)
            ),
            canonical_path: canonical,
            path_hash: metria_core::privacy::hash_path(&path_str),
            client_version,
            capabilities,
        }
    }
}
