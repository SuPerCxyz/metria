//! UsageEvent：不可变的用量事件。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::UsageGranularity;
use super::ids::EventId;

/// Token 使用量。缺失值必须为 `null`，禁止默认填 0。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    pub reasoning: Option<i64>,
}

impl Usage {
    /// 是否完全没有 token 数据。
    pub fn is_empty(&self) -> bool {
        self.input.is_none()
            && self.output.is_none()
            && self.cache_read.is_none()
            && self.cache_write.is_none()
            && self.reasoning.is_none()
    }
}

/// 费用三口径。各自可追溯，互不冒充。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// 客户端日志明确提供的费用
    pub reported_micro_usd: Option<i64>,
    /// 按可靠 Token 与确定价格规则计算
    pub calculated_micro_usd: Option<i64>,
    /// Token 或价格至少一项为估算
    pub estimated_micro_usd: Option<i64>,
    pub pricing_rule_id: Option<String>,
    pub pricing_snapshot_id: Option<String>,
}

/// 数据质量：来源、粒度、可信度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quality {
    pub usage_source: String,
    pub granularity: UsageGranularity,
    pub confidence: Option<f32>,
}

/// 用量事件（不可变，作为去重与追溯的最小单元）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageEvent {
    pub schema_version: u32,
    pub event_id: EventId,
    pub node_id: String,
    pub collector_id: String,
    pub source_id: String,
    pub client_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub model_call_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub provider_raw: Option<String>,
    pub provider_normalized: Option<String>,
    pub model_raw: Option<String>,
    pub model_normalized: Option<String>,
    pub usage: Usage,
    pub cost: Cost,
    pub quality: Quality,
}

impl UsageEvent {
    /// 构建事件 ID（内容哈希），并校验 token 非负。
    pub fn finalize(mut self) -> Result<Self, super::super::error::ModelError> {
        validate_tokens(self.usage.input)?;
        validate_tokens(self.usage.output)?;
        validate_tokens(self.usage.cache_read)?;
        validate_tokens(self.usage.cache_write)?;
        validate_tokens(self.usage.reasoning)?;
        let value = serde_json::to_value(&self)
            .map_err(|e| super::super::error::ModelError::InvalidId(e.to_string()))?;
        self.event_id = EventId::from_json(&value);
        Ok(self)
    }
}

fn validate_tokens(v: Option<i64>) -> Result<(), super::super::error::ModelError> {
    if let Some(n) = v {
        if n < 0 {
            return Err(super::super::error::ModelError::InvalidNumber {
                field: "token",
                message: format!("负数 {n}"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> UsageEvent {
        UsageEvent {
            schema_version: 1,
            event_id: EventId::from_content("placeholder"),
            node_id: "node-01".into(),
            collector_id: "collector-01".into(),
            source_id: "source-01".into(),
            client_id: "claude-code".into(),
            adapter_id: "claude-code".into(),
            adapter_version: "0.1.0".into(),
            session_id: Some("session-01".into()),
            turn_id: None,
            model_call_id: Some("call-01".into()),
            timestamp: DateTime::parse_from_rfc3339("2026-08-05T14:02:11Z")
                .unwrap()
                .with_timezone(&Utc),
            provider_raw: Some("anthropic".into()),
            provider_normalized: Some("anthropic".into()),
            model_raw: Some("claude-opus-4-6".into()),
            model_normalized: Some("claude-opus-4.6".into()),
            usage: Usage {
                input: Some(32100),
                output: Some(1250),
                cache_read: Some(28100),
                cache_write: Some(4000),
                reasoning: None,
            },
            cost: Cost {
                reported_micro_usd: None,
                calculated_micro_usd: Some(83200),
                estimated_micro_usd: None,
                pricing_rule_id: None,
                pricing_snapshot_id: None,
            },
            quality: Quality {
                usage_source: "reported".into(),
                granularity: UsageGranularity::Turn,
                confidence: Some(1.0),
            },
        }
    }

    #[test]
    fn finalize_sets_stable_event_id() {
        let a = base().finalize().unwrap();
        let b = base().finalize().unwrap();
        assert_eq!(a.event_id, b.event_id);
        assert!(a.event_id.as_str().starts_with("blake3:"));
    }

    #[test]
    fn negative_tokens_rejected() {
        let mut e = base();
        e.usage.input = Some(-1);
        assert!(e.finalize().is_err());
    }

    #[test]
    fn json_shape() {
        let e = base().finalize().unwrap();
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["quality"]["granularity"], "turn");
        assert!(json["cost"]["reported_micro_usd"].is_null());
    }
}
