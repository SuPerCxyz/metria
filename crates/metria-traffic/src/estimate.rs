//! 零侵入流量估算核心。
//!
//! 原则：
//! - 流量一律为「估算」，禁止标为实际/网卡/账单流量。
//! - 禁止生成下界=中值=上界；缺数据时 `unavailable`，不硬造。
//! - Cache Token 不直接等同网络字节；Reasoning Token 不全量换算响应字节。
//! - 系数全部来自版本化 Traffic Profile。

use chrono::Utc;
use metria_core::model::{
    CacheTransportBehavior, ContentProfile, ContextTransportMode, EstimationSource,
    ReconstructionQuality,
};

use crate::error::Result;
use crate::profile::{self, MatchRequest, ProfileMatch};

/// 估算输入。
#[derive(Debug)]
pub struct EstimateInput<'a> {
    pub client: &'a str,
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub streaming: bool,
    /// 可重建的请求正文（或日志直接给出的 payload）
    pub request_text: Option<&'a str>,
    pub response_text: Option<&'a str>,
    pub request_reconstruction_quality: ReconstructionQuality,
    pub response_reconstruction_quality: ReconstructionQuality,
    pub context_transport_mode: ContextTransportMode,
    pub cache_transport_behavior: CacheTransportBehavior,
}

/// 估算输出。
#[derive(Debug)]
pub struct EstimateOutput {
    pub request_payload_bytes: Option<i64>,
    pub response_payload_bytes: Option<i64>,
    pub estimated_request_wire_bytes: Option<i64>,
    pub estimated_response_wire_bytes: Option<i64>,
    pub estimated_total_wire_bytes: Option<i64>,
    pub lower_bound_bytes: Option<i64>,
    pub upper_bound_bytes: Option<i64>,
    pub estimation_source: EstimationSource,
    pub confidence: Option<f32>,
    pub notes: Vec<String>,
}

/// 对流式 SSE 的额外开销比例（每条事件有 data:/event: 包装）。
const STREAM_EVENT_OVERHEAD_RATIO: f64 = 0.20;
/// 重建 JSON 时，正文之外的键/引号开销比例。
const JSON_ENVELOPE_RATIO: f64 = 0.10;

/// 对一次调用执行请求与响应流量估算（使用内置候选 Profile）。
pub fn estimate<'a>(input: &EstimateInput<'a>) -> Result<EstimateOutput> {
    let candidates = crate::profile::builtin_candidates(input.client);
    estimate_with_candidates(input, &candidates)
}

/// 使用指定候选 Profile 集执行估算（用于历史重新估算等场景）。
pub fn estimate_with_candidates<'a>(
    input: &EstimateInput<'a>,
    candidates: &[metria_core::model::TrafficProfile],
) -> Result<EstimateOutput> {
    let mut notes = Vec::new();

    // ---------- 请求 ----------
    let (request_payload, request_source, req_profile_match) =
        estimate_request(input, candidates, &mut notes);
    // ---------- 响应 ----------
    let (response_payload, response_source, resp_profile_match) =
        estimate_response(input, candidates, &mut notes);

    let source = pick_source(request_source, response_source);
    let profile_match = req_profile_match.or(resp_profile_match);

    // ---------- wire 与区间 ----------
    let http = profile_match
        .as_ref()
        .map(|m| f64::from(m.profile.http_overhead_ratio))
        .unwrap_or(0.05);
    let transport = profile_match
        .as_ref()
        .map(|m| f64::from(m.profile.transport_overhead_ratio))
        .unwrap_or(0.10);

    let request_wire =
        request_payload.map(|b| (b as f64 * (1.0 + http) * (1.0 + transport)).round() as i64);
    let response_wire =
        response_payload.map(|b| (b as f64 * (1.0 + http) * (1.0 + transport)).round() as i64);
    let total = match (request_wire, response_wire) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    let confidence = compute_confidence(source, profile_match.as_ref(), input);
    let (lower, upper) = bounds(total, confidence, profile_match.as_ref(), &mut notes);

    Ok(EstimateOutput {
        request_payload_bytes: request_payload,
        response_payload_bytes: response_payload,
        estimated_request_wire_bytes: request_wire,
        estimated_response_wire_bytes: response_wire,
        estimated_total_wire_bytes: total,
        lower_bound_bytes: lower,
        upper_bound_bytes: upper,
        estimation_source: source,
        confidence,
        notes,
    })
}

fn estimate_request<'a>(
    input: &EstimateInput<'a>,
    candidates: &[metria_core::model::TrafficProfile],
    notes: &mut Vec<String>,
) -> (Option<i64>, EstimationSource, Option<ProfileMatch>) {
    // 1. 完整重建 / 直接 payload
    if let Some(text) = input.request_text {
        let bytes = text.len() as i64;
        match input.request_reconstruction_quality {
            ReconstructionQuality::Complete => {
                let payload = (bytes as f64 * (1.0 + JSON_ENVELOPE_RATIO)).round() as i64;
                notes.push("请求：完整重建".into());
                return (Some(payload), EstimationSource::ReconstructedPayload, None);
            }
            ReconstructionQuality::Partial => {
                let payload = (bytes as f64 * 1.15).round() as i64;
                notes.push("请求：部分重建，隐藏内容未知，+15% 补偿并降低置信度".into());
                return (Some(payload), EstimationSource::PartialReconstruction, None);
            }
            ReconstructionQuality::None => {}
        }
    }

    // 2. Token Profile
    let input_tokens = match input.input_tokens {
        Some(t) if t >= 0 => t,
        _ => {
            notes.push("请求：无 token 与正文，无法估算".into());
            return (None, EstimationSource::Unavailable, None);
        }
    };

    let profile_match = profile::best_profile(
        candidates,
        &MatchRequest {
            client: input.client.to_string(),
            client_version: None,
            provider: input.provider.map(|s| s.to_string()),
            model: input.model.map(|s| s.to_string()),
            content_profile: ContentProfile::Unknown,
            direction: metria_core::model::TrafficDirection::Request,
            streaming: Some(input.streaming),
            at: Utc::now(),
        },
    );

    let p = profile_match.as_ref();
    let bpt = p
        .map(|m| m.profile.input_bytes_per_token_p50)
        .unwrap_or(3.6) as f64;
    let cache_read = input.cache_read_tokens.unwrap_or(0).max(0) as f64;
    let cache_write = input.cache_write_tokens.unwrap_or(0).max(0) as f64;
    let uncached = (input_tokens
        - input.cache_read_tokens.unwrap_or(0)
        - input.cache_write_tokens.unwrap_or(0))
    .max(0) as f64;

    let cache_read_factor = p
        .map(|m| f64::from(m.profile.cache_read_transport_factor))
        .unwrap_or(0.8);
    let cache_write_factor = p
        .map(|m| f64::from(m.profile.cache_write_transport_factor))
        .unwrap_or(1.0);
    let fixed = p.map(|m| m.profile.fixed_request_bytes).unwrap_or(1024) as f64;

    let (payload, source) = match input.context_transport_mode {
        ContextTransportMode::FullContext => {
            let b = uncached * bpt
                + cache_read * cache_read_factor
                + cache_write * cache_write_factor
                + fixed;
            (b, EstimationSource::TokenProfile)
        }
        ContextTransportMode::StatefulReference => {
            // 本次上传主要为新上下文与写缓存内容；缓存读不重复上传
            let b = uncached * bpt + cache_write * cache_write_factor + fixed;
            notes.push("请求：stateful_reference，未计入 cache_read 重传".into());
            (b, EstimationSource::TokenProfile)
        }
        ContextTransportMode::Mixed => {
            let b = uncached * bpt
                + cache_read * cache_read_factor * 0.5
                + cache_write * cache_write_factor
                + fixed;
            notes.push("请求：mixed 传输，cache_read 按 50% 折算".into());
            (b, EstimationSource::TokenProfile)
        }
        ContextTransportMode::Unknown => {
            let b = uncached * bpt
                + cache_read * cache_read_factor
                + cache_write * cache_write_factor
                + fixed;
            notes.push("请求：传输模式未知，区间将放宽".into());
            (b, EstimationSource::TokenProfile)
        }
    };

    match input.cache_transport_behavior {
        CacheTransportBehavior::ReferenceOnly => {
            notes.push("请求：cache 行为 reference_only".into());
        }
        CacheTransportBehavior::Unknown => {
            notes.push("请求：cache 行为未知，区间将放宽".into());
        }
        _ => {}
    }

    (Some(payload.round() as i64), source, profile_match)
}

fn estimate_response<'a>(
    input: &EstimateInput<'a>,
    candidates: &[metria_core::model::TrafficProfile],
    notes: &mut Vec<String>,
) -> (Option<i64>, EstimationSource, Option<ProfileMatch>) {
    // 1. 可见内容优先
    if let Some(text) = input.response_text {
        let bytes = text.len() as i64;
        match input.response_reconstruction_quality {
            ReconstructionQuality::Complete => {
                let mut payload = bytes as f64;
                if input.streaming {
                    payload *= 1.0 + STREAM_EVENT_OVERHEAD_RATIO;
                    notes.push("响应：完整重建 + 流式事件开销".into());
                }
                return (
                    Some(payload.round() as i64),
                    EstimationSource::ReconstructedPayload,
                    None,
                );
            }
            ReconstructionQuality::Partial => {
                let payload = (bytes as f64 * 1.1).round() as i64;
                notes.push("响应：部分重建".into());
                return (Some(payload), EstimationSource::PartialReconstruction, None);
            }
            ReconstructionQuality::None => {}
        }
    }

    // 2. Token Profile
    let output_tokens = match input.output_tokens {
        Some(t) if t >= 0 => t,
        _ => {
            notes.push("响应：无 token 与正文，无法估算".into());
            return (None, EstimationSource::Unavailable, None);
        }
    };

    // reasoning 处理：传输语义未知时保守处理，全量输出计入，但放宽区间
    if let Some(r) = input.reasoning_tokens {
        if r > 0 {
            notes.push(format!(
                "响应：reasoning={r}，传输语义未知，按全量输出估算并放宽区间"
            ));
        }
    }

    let profile_match = profile::best_profile(
        candidates,
        &MatchRequest {
            client: input.client.to_string(),
            client_version: None,
            provider: input.provider.map(|s| s.to_string()),
            model: input.model.map(|s| s.to_string()),
            content_profile: ContentProfile::Unknown,
            direction: metria_core::model::TrafficDirection::Response,
            streaming: Some(input.streaming),
            at: Utc::now(),
        },
    );

    let p = profile_match.as_ref();
    let bpt = p
        .map(|m| m.profile.output_bytes_per_token_p50)
        .unwrap_or(4.0) as f64;
    let fixed = p.map(|m| m.profile.fixed_response_bytes).unwrap_or(128) as f64;
    let mut payload = output_tokens as f64 * bpt + fixed;
    if input.streaming {
        payload *= 1.0 + STREAM_EVENT_OVERHEAD_RATIO;
        notes.push("响应：流式事件开销计入".into());
    }
    (
        Some(payload.round() as i64),
        EstimationSource::TokenProfile,
        profile_match,
    )
}

/// 综合来源（取更高优先级；None 一方忽略）。
fn pick_source(a: EstimationSource, b: EstimationSource) -> EstimationSource {
    if a == EstimationSource::Unavailable {
        b
    } else if b == EstimationSource::Unavailable {
        a
    } else {
        a.max(b)
    }
}

fn compute_confidence(
    source: EstimationSource,
    profile_match: Option<&ProfileMatch>,
    input: &EstimateInput<'_>,
) -> Option<f32> {
    let base = match source {
        EstimationSource::ObservedPayloadSize => 0.90,
        EstimationSource::ReconstructedPayload => 0.75,
        EstimationSource::PartialReconstruction => 0.60,
        EstimationSource::ContentBytes => 0.50,
        EstimationSource::TokenProfile => 0.55,
        EstimationSource::UserProfile => 0.65,
        EstimationSource::Unavailable => return None,
    };
    let mut c = base;
    if input.context_transport_mode == ContextTransportMode::Unknown {
        c -= 0.15;
    }
    if input.cache_read_tokens.unwrap_or(0) > 0
        && input.cache_transport_behavior == CacheTransportBehavior::Unknown
    {
        c -= 0.10;
    }
    if input.reasoning_tokens.unwrap_or(0) > 0 {
        c -= 0.05;
    }
    if let Some(m) = profile_match {
        c = (c + m.profile.confidence) / 2.0;
    }
    Some(c.clamp(0.05, 0.95))
}

fn bounds(
    mid: Option<i64>,
    confidence: Option<f32>,
    profile_match: Option<&ProfileMatch>,
    notes: &mut Vec<String>,
) -> (Option<i64>, Option<i64>) {
    let mid = match mid {
        Some(m) if m > 0 => m,
        _ => return (None, None),
    };
    let conf = confidence.unwrap_or(0.3) as f64;
    // 区间宽度：置信度越低越宽
    let spread = (1.2 - conf).clamp(0.15, 0.9);
    // 若使用 profile 分位数，用 p50->p90 的相对差加宽
    let profile_spread = profile_match
        .map(|m| {
            (f64::from(m.profile.input_bytes_per_token_p90)
                / f64::from(m.profile.input_bytes_per_token_p50).max(0.1)
                - 1.0)
                .max(0.0)
        })
        .unwrap_or(0.0);
    let low_factor = spread + profile_spread;
    let high_factor = spread + profile_spread;

    let mut lower = (mid as f64 * (1.0 - low_factor)).round() as i64;
    let mut upper = (mid as f64 * (1.0 + high_factor)).round() as i64;
    // 强制 lower < mid < upper（禁止伪装精确）
    if lower >= mid {
        lower = (mid as f64 * 0.9).floor() as i64;
    }
    if upper <= mid {
        upper = (mid as f64 * 1.1).ceil() as i64;
    }
    if lower < 0 {
        lower = 0;
    }
    notes.push(format!(
        "区间：{lower} ~ {upper}（基于估算 {mid}，可信度 {:.0}%）",
        conf * 100.0
    ));
    (Some(lower), Some(upper))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base<'a>() -> EstimateInput<'a> {
        EstimateInput {
            client: "claude-code",
            provider: Some("anthropic"),
            model: Some("claude-sonnet-4.5"),
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_tokens: Some(300),
            cache_write_tokens: Some(50),
            reasoning_tokens: None,
            streaming: true,
            request_text: None,
            response_text: None,
            request_reconstruction_quality: ReconstructionQuality::None,
            response_reconstruction_quality: ReconstructionQuality::None,
            context_transport_mode: ContextTransportMode::FullContext,
            cache_transport_behavior: CacheTransportBehavior::FullContentSent,
        }
    }

    #[test]
    fn token_profile_estimate_has_range() {
        let out = estimate(&base()).unwrap();
        assert!(out.estimated_total_wire_bytes.is_some());
        let (lo, hi, mid) = (
            out.lower_bound_bytes.unwrap(),
            out.upper_bound_bytes.unwrap(),
            out.estimated_total_wire_bytes.unwrap(),
        );
        assert!(lo < mid && mid < hi, "禁止下界=中值=上界");
        assert!(out.confidence.is_some());
        assert_eq!(out.estimation_source, EstimationSource::TokenProfile);
    }

    #[test]
    fn reconstruction_preferred_over_token_profile() {
        let mut i = base();
        i.request_text = Some(r#"{"model":"x","messages":[{"role":"user","content":"hi"}]}"#);
        i.request_reconstruction_quality = ReconstructionQuality::Complete;
        let out = estimate(&i).unwrap();
        assert_eq!(
            out.estimation_source,
            EstimationSource::ReconstructedPayload
        );
        assert!(out.estimated_request_wire_bytes.is_some());
    }

    #[test]
    fn unavailable_when_nothing() {
        let mut i = base();
        i.input_tokens = None;
        i.output_tokens = None;
        i.request_text = None;
        i.response_text = None;
        let out = estimate(&i).unwrap();
        assert_eq!(out.estimation_source, EstimationSource::Unavailable);
        assert!(out.estimated_total_wire_bytes.is_none());
        assert!(out.lower_bound_bytes.is_none());
    }

    #[test]
    fn stateful_reference_reduces_request() {
        let a = estimate(&base()).unwrap();
        let mut i = base();
        i.context_transport_mode = ContextTransportMode::StatefulReference;
        let b = estimate(&i).unwrap();
        assert!(
            b.estimated_request_wire_bytes.unwrap() <= a.estimated_request_wire_bytes.unwrap(),
            "stateful 不应大于 full_context"
        );
    }

    #[test]
    fn reasoning_present_still_estimates() {
        let mut i = base();
        i.reasoning_tokens = Some(200);
        let out = estimate(&i).unwrap();
        assert!(out.estimated_total_wire_bytes.is_some());
        assert!(out.notes.iter().any(|n| n.contains("reasoning")));
    }
}
