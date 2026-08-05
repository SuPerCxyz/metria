//! Traffic Profile 注册表与匹配。
//!
//! 来源优先级：user 精确 > user 通配 > adapter > learned 精确 > learned 通配 > builtin > unavailable。
//! M1 实现 builtin + user + adapter 静态匹配；自动学习在 S2/M2 加入。

use chrono::{DateTime, Utc};
use metria_core::model::{
    ContentProfile, Id, TrafficDirection, TrafficProfile, TrafficProfileSource,
};

use crate::error::TrafficError;

/// 内置 bytes-per-token 基准（版本 1）。
///
/// 来源说明：基于常见 tokenizer 的近似实测区间，非「1 token = 4 bytes」固定值。
/// 中文约 2.2、英文约 4.0、代码约 3.8、base64 约 1.35 字节/token。
pub fn builtin_bytes_per_token(p: ContentProfile) -> f32 {
    match p {
        ContentProfile::NaturalLanguageZh => 2.2,
        ContentProfile::NaturalLanguageEn => 4.0,
        ContentProfile::SourceCode => 3.8,
        ContentProfile::Json => 3.6,
        ContentProfile::ToolSchema => 3.6,
        ContentProfile::ToolResult => 4.0,
        ContentProfile::TerminalOutput => 3.5,
        ContentProfile::Log => 3.5,
        ContentProfile::Markdown => 3.5,
        ContentProfile::Xml => 3.5,
        ContentProfile::Base64 => 1.35,
        ContentProfile::Mixed => 3.2,
        ContentProfile::Unknown => 3.6,
    }
}

/// 固定请求开销（字节），覆盖基础 JSON 包装、元数据等。
pub const BUILTIN_FIXED_REQUEST_BYTES: i64 = 1024;
/// 固定响应开销。
pub const BUILTIN_FIXED_RESPONSE_BYTES: i64 = 128;

/// 匹配结果。
#[derive(Debug)]
pub struct ProfileMatch {
    pub profile: TrafficProfile,
    /// 匹配原因说明（供展示与测试）。
    pub reason: String,
    /// 优先级数值（越大越优先）。
    pub priority: i32,
}

/// 匹配请求条件。
#[derive(Debug, Clone)]
pub struct MatchRequest {
    pub client: String,
    pub client_version: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub content_profile: ContentProfile,
    pub direction: TrafficDirection,
    pub streaming: Option<bool>,
    pub at: DateTime<Utc>,
}

/// 从候选列表中选出最佳 Profile。
///
/// 排序规则（权重）：来源优先级（user=4/adapter=3/learned=2/builtin=1）> 模型匹配精确度 > 生效时间。
pub fn best_profile(candidates: &[TrafficProfile], req: &MatchRequest) -> Option<ProfileMatch> {
    candidates
        .iter()
        .filter(|p| p.effective_at(req.at))
        .filter(|p| {
            p.content_profile == req.content_profile || p.content_profile == ContentProfile::Unknown
        })
        .filter(|p| p.direction == req.direction)
        .map(|p| {
            let src_priority = source_priority(p.source);
            let model_exact = if p.model_pattern == "*" {
                0
            } else if metria_core::normalize::pattern_match(
                &p.model_pattern,
                req.model.as_deref().unwrap_or(""),
            ) {
                2
            } else {
                -100
            };
            let client_ok = metria_core::normalize::pattern_match(&p.client_pattern, &req.client);
            let base = if client_ok {
                src_priority + model_exact
            } else {
                -1000
            };
            ProfileMatch {
                profile: p.clone(),
                reason: format!(
                    "source={:?} client={} model={} content={:?}",
                    p.source, p.client_pattern, p.model_pattern, p.content_profile
                ),
                priority: base,
            }
        })
        .filter(|m| m.priority >= 0)
        .max_by_key(|m| m.priority)
}

fn source_priority(s: TrafficProfileSource) -> i32 {
    match s {
        TrafficProfileSource::User => 4,
        TrafficProfileSource::Adapter => 3,
        TrafficProfileSource::Learned => 2,
        TrafficProfileSource::Builtin => 1,
    }
}

/// 为某 client + content_profile + direction 构造内置 Profile。
pub fn builtin_profile(
    client: &str,
    content_profile: ContentProfile,
    direction: TrafficDirection,
) -> TrafficProfile {
    TrafficProfile::builtin(
        Id::new(),
        client,
        content_profile,
        direction,
        builtin_bytes_per_token(content_profile),
        match direction {
            TrafficDirection::Request => BUILTIN_FIXED_REQUEST_BYTES,
            TrafficDirection::Response => BUILTIN_FIXED_RESPONSE_BYTES,
        },
    )
}

/// 默认候选集：内置全内容类型。
pub fn builtin_candidates(client: &str) -> Vec<TrafficProfile> {
    use ContentProfile::*;
    [
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
    ]
    .into_iter()
    .flat_map(|p| {
        [
            builtin_profile(client, p, TrafficDirection::Request),
            builtin_profile(client, p, TrafficDirection::Response),
        ]
    })
    .collect()
}

/// 校验 Profile 数值合法（区间、比率非负、p50<=p75<=p90）。
pub fn validate_profile(p: &TrafficProfile) -> Result<(), TrafficError> {
    if p.input_bytes_per_token_p50 < 0.0
        || p.output_bytes_per_token_p50 < 0.0
        || p.http_overhead_ratio < 0.0
        || p.transport_overhead_ratio < 0.0
        || p.cache_read_transport_factor < 0.0
        || p.cache_write_transport_factor < 0.0
        || p.confidence < 0.0
        || p.confidence > 1.0
    {
        return Err(TrafficError::InvalidProfile(
            "数值非法（负值或置信度越界）".into(),
        ));
    }
    if p.input_bytes_per_token_p50 > p.input_bytes_per_token_p75
        || p.input_bytes_per_token_p75 > p.input_bytes_per_token_p90
    {
        return Err(TrafficError::InvalidProfile("p50<=p75<=p90 违反".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_bpt_sane() {
        assert!(
            builtin_bytes_per_token(ContentProfile::NaturalLanguageZh)
                < builtin_bytes_per_token(ContentProfile::NaturalLanguageEn)
        );
        assert!(builtin_bytes_per_token(ContentProfile::Base64) < 2.0);
    }

    #[test]
    fn match_prefers_user_over_builtin() {
        let builtin = builtin_profile(
            "claude-code",
            ContentProfile::NaturalLanguageEn,
            TrafficDirection::Request,
        );
        let mut user = builtin.clone();
        user.id = Id::new();
        user.source = TrafficProfileSource::User;
        user.model_pattern = "claude-*".into();
        let req = MatchRequest {
            client: "claude-code".into(),
            client_version: None,
            provider: Some("anthropic".into()),
            model: Some("claude-opus-4.6".into()),
            content_profile: ContentProfile::NaturalLanguageEn,
            direction: TrafficDirection::Request,
            streaming: None,
            at: Utc::now(),
        };
        let m = best_profile(&[builtin.clone(), user.clone()], &req).unwrap();
        assert_eq!(m.profile.source, TrafficProfileSource::User);
        assert!(m.profile.model_pattern.contains("claude"));
    }

    #[test]
    fn validate_rejects_bad() {
        let mut p = builtin_profile("c", ContentProfile::Json, TrafficDirection::Request);
        assert!(validate_profile(&p).is_ok());
        p.input_bytes_per_token_p50 = 9.0; // > p75
        assert!(validate_profile(&p).is_err());
    }
}
