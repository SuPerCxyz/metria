//! 流量估算领域模型：TrafficEstimate、TrafficProfile、TrafficProfileSample。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{
    CacheTransportBehavior, ContentProfile, ContextTransportMode, EstimationSource,
    TrafficProfileSource,
};
use super::ids::{ContentHash, Id};

/// 请求/响应方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficDirection {
    Request,
    Response,
}

/// 重建质量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconstructionQuality {
    Complete,
    Partial,
    None,
}

/// 单次模型调用的流量估算。
///
/// 流量一律为「估算」，不得标记为实际/精确/网卡/账单流量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrafficEstimate {
    pub id: Id,
    pub model_call_id: Id,
    pub node_id: String,
    pub client_id: String,
    pub session_id: Option<Id>,
    pub turn_id: Option<Id>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub request_payload_bytes: Option<i64>,
    pub response_payload_bytes: Option<i64>,
    pub estimated_request_http_bytes: Option<i64>,
    pub estimated_response_http_bytes: Option<i64>,
    pub estimated_request_wire_bytes: Option<i64>,
    pub estimated_response_wire_bytes: Option<i64>,
    pub estimated_total_wire_bytes: Option<i64>,
    pub lower_bound_bytes: Option<i64>,
    pub upper_bound_bytes: Option<i64>,
    pub estimation_source: EstimationSource,
    pub context_transport_mode: ContextTransportMode,
    pub cache_transport_behavior: CacheTransportBehavior,
    pub request_reconstruction_quality: ReconstructionQuality,
    pub response_reconstruction_quality: ReconstructionQuality,
    pub profile_id: Option<Id>,
    pub profile_version: Option<i64>,
    /// 0.0 ~ 1.0
    pub confidence: Option<f32>,
    pub calculated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl TrafficEstimate {
    /// 是否可视为有意义的估算（来源非 unavailable 且有中值）。
    pub fn is_available(&self) -> bool {
        self.estimation_source != EstimationSource::Unavailable
            && self.estimated_total_wire_bytes.is_some()
    }
}

/// Traffic Profile：bytes-per-token 与固定开销的版本化配置。
///
/// 来源优先级：user 精确 > user 通配 > adapter > learned 精确 > learned 通配 > builtin > unavailable。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrafficProfile {
    pub id: Id,
    pub source: TrafficProfileSource,
    pub client_pattern: String,
    pub client_version_pattern: String,
    pub provider_pattern: String,
    pub model_pattern: String,
    pub content_profile: ContentProfile,
    pub direction: TrafficDirection,
    pub streaming: Option<bool>,
    pub context_transport_mode: ContextTransportMode,
    pub input_bytes_per_token_p50: f32,
    pub input_bytes_per_token_p75: f32,
    pub input_bytes_per_token_p90: f32,
    pub output_bytes_per_token_p50: f32,
    pub output_bytes_per_token_p75: f32,
    pub output_bytes_per_token_p90: f32,
    pub fixed_request_bytes: i64,
    pub fixed_response_bytes: i64,
    pub http_overhead_ratio: f32,
    pub transport_overhead_ratio: f32,
    pub cache_read_transport_factor: f32,
    pub cache_write_transport_factor: f32,
    pub sample_count: u64,
    /// 0.0 ~ 1.0；样本不足时降低
    pub confidence: f32,
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_to: Option<DateTime<Utc>>,
    pub version: i64,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TrafficProfile {
    pub fn builtin(
        id: Id,
        client: &str,
        content_profile: ContentProfile,
        direction: TrafficDirection,
        bytes_per_token: f32,
        fixed_bytes: i64,
    ) -> Self {
        Self {
            id,
            source: TrafficProfileSource::Builtin,
            client_pattern: client.to_string(),
            client_version_pattern: "*".to_string(),
            provider_pattern: "*".to_string(),
            model_pattern: "*".to_string(),
            content_profile,
            direction,
            streaming: None,
            context_transport_mode: ContextTransportMode::Unknown,
            input_bytes_per_token_p50: bytes_per_token,
            input_bytes_per_token_p75: bytes_per_token * 1.15,
            input_bytes_per_token_p90: bytes_per_token * 1.3,
            output_bytes_per_token_p50: bytes_per_token,
            output_bytes_per_token_p75: bytes_per_token * 1.15,
            output_bytes_per_token_p90: bytes_per_token * 1.3,
            fixed_request_bytes: fixed_bytes,
            fixed_response_bytes: 64,
            http_overhead_ratio: 0.05,
            transport_overhead_ratio: 0.10,
            cache_read_transport_factor: 0.8,
            cache_write_transport_factor: 1.0,
            sample_count: 0,
            confidence: 0.3,
            effective_from: None,
            effective_to: None,
            version: 1,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// 是否在 `at` 时间生效。
    pub fn effective_at(&self, at: DateTime<Utc>) -> bool {
        self.enabled
            && self.effective_from.is_none_or(|f| at >= f)
            && self.effective_to.is_none_or(|t| at <= t)
    }
}

/// Traffic Profile 自动学习样本。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrafficProfileSample {
    pub id: Id,
    pub client: String,
    pub client_version: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub content_profile: ContentProfile,
    pub direction: TrafficDirection,
    pub token_count: u64,
    pub payload_bytes: i64,
    pub bytes_per_token: f32,
    pub reconstruction_quality: ReconstructionQuality,
    /// 源内容哈希（仅用于去重，不保存正文）
    pub source_hash: ContentHash,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_must_hold_range() {
        let e = TrafficEstimate {
            id: Id::new(),
            model_call_id: Id::new(),
            node_id: "n".into(),
            client_id: "c".into(),
            session_id: None,
            turn_id: None,
            provider: None,
            model: None,
            request_payload_bytes: Some(1000),
            response_payload_bytes: Some(2000),
            estimated_request_http_bytes: Some(1100),
            estimated_response_http_bytes: Some(2200),
            estimated_request_wire_bytes: Some(1200),
            estimated_response_wire_bytes: Some(2400),
            estimated_total_wire_bytes: Some(3600),
            lower_bound_bytes: Some(3000),
            upper_bound_bytes: Some(4500),
            estimation_source: EstimationSource::TokenProfile,
            context_transport_mode: ContextTransportMode::Unknown,
            cache_transport_behavior: CacheTransportBehavior::Unknown,
            request_reconstruction_quality: ReconstructionQuality::None,
            response_reconstruction_quality: ReconstructionQuality::Partial,
            profile_id: None,
            profile_version: None,
            confidence: Some(0.5),
            calculated_at: Utc::now(),
            created_at: Utc::now(),
        };
        assert!(e.is_available());
        let (lo, hi, mid) = (
            e.lower_bound_bytes.unwrap(),
            e.upper_bound_bytes.unwrap(),
            e.estimated_total_wire_bytes.unwrap(),
        );
        assert!(lo < mid && mid < hi, "禁止生成下界=中值=上界");
    }

    #[test]
    fn builtin_profile_versioned() {
        let p = TrafficProfile::builtin(
            Id::new(),
            "claude-code",
            ContentProfile::NaturalLanguageEn,
            TrafficDirection::Request,
            4.0,
            1000,
        );
        assert_eq!(p.version, 1);
        assert!(p.effective_at(Utc::now()));
    }
}
