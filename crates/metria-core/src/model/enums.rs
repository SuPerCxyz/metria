//! 领域枚举：状态、数据质量、流量估算来源、内容分类等。

use serde::{Deserialize, Serialize};

/// Node 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Online,
    Offline,
    Degraded,
    Unknown,
}

/// Collector 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectorStatus {
    Online,
    Offline,
    Degraded,
    Unknown,
}

/// Source 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Active,
    Idle,
    Error,
    Disabled,
    Missing,
}

/// Session 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Ended,
    Interrupted,
    Unknown,
}

/// Usage 数据来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// 客户端日志明确提供
    Reported,
    /// 由可信字段推导
    Derived,
    /// 估算
    Estimated,
    /// 缺失
    Missing,
}

/// Usage 粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageGranularity {
    Message,
    Call,
    Turn,
    Session,
    Hour,
    Day,
}

/// Model Call 粒度（禁止将 Session 级统计伪装成单次调用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallGranularity {
    Message,
    Call,
    Turn,
    Session,
}

/// 流量估算来源（优先级从高到低；enum 判别值越大优先级越高）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimationSource {
    /// 数据不足，无法合理估算
    Unavailable,
    /// 用户配置的 bytes-per-token
    UserProfile,
    /// 仅 Token，按 Traffic Profile 估算
    TokenProfile,
    /// 仅内容字节数
    ContentBytes,
    /// 只能部分重建
    PartialReconstruction,
    /// 可完整重建请求/响应 Payload
    ReconstructedPayload,
    /// 日志直接包含请求/响应 Payload 字节数
    ObservedPayloadSize,
}

/// 上下文传输模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTransportMode {
    /// 每次请求重新发送完整或主要上下文
    FullContext,
    /// 通过 previous_response_id 等远端引用传递上下文
    StatefulReference,
    /// 部分发送、部分引用
    Mixed,
    /// 无法确定
    Unknown,
}

/// Cache 传输行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheTransportBehavior {
    /// 缓存内容完整包含在请求 Payload 中
    FullContentSent,
    /// 主要传递缓存引用
    ReferenceOnly,
    /// 部分发送部分引用
    Mixed,
    /// 未知
    Unknown,
}

/// 内容组成分类（bytes-per-token 差异大，用于流量估算与 Profile 匹配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentProfile {
    NaturalLanguageZh,
    NaturalLanguageEn,
    SourceCode,
    Json,
    ToolSchema,
    ToolResult,
    TerminalOutput,
    Log,
    Markdown,
    Xml,
    Base64,
    Mixed,
    Unknown,
}

/// 流量可信度等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficConfidenceLevel {
    High,
    Medium,
    Low,
    Unavailable,
}

/// 价格来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingSource {
    ClientReported,
    UserOverride,
    OpenRouterCatalog,
    LiteLlmCatalog,
    BuiltinCatalog,
    CustomHttpCatalog,
}

/// 价格渠道（OpenRouter 等第三方渠道需标注，不得冒充厂商直连价格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingChannel {
    VendorDirect,
    OpenRouter,
    LiteLlm,
    Custom,
}

/// TrafficProfile 来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficProfileSource {
    Builtin,
    Learned,
    User,
    Adapter,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let src = UsageSource::Reported;
        let json = serde_json::to_string(&src).unwrap();
        assert_eq!(json, "\"reported\"");
        assert_eq!(serde_json::from_str::<UsageSource>(&json).unwrap(), src);
    }

    #[test]
    fn estimation_source_ordering() {
        assert!(EstimationSource::ObservedPayloadSize > EstimationSource::TokenProfile);
        assert!(EstimationSource::TokenProfile > EstimationSource::Unavailable);
    }
}
