//! 价格体系模型：PricingCatalog、PricingSnapshot、PricingRule、PricingMatch。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{PricingChannel, PricingSource};
use super::ids::Id;

/// 价格目录（数据来源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingCatalog {
    pub id: Id,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub base_url: Option<String>,
    pub authentication_type: Option<String>,
    pub refresh_interval_seconds: Option<i64>,
    pub priority: i64,
    pub last_refresh_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 价格快照（不可变，保存来源与原始数据哈希）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingSnapshot {
    pub id: Id,
    pub catalog_id: Id,
    pub catalog_version: Option<String>,
    pub etag: Option<String>,
    pub fetched_at: DateTime<Utc>,
    pub effective_at: DateTime<Utc>,
    pub content_hash: String,
    pub record_count: i64,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// 价格规则。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingRule {
    pub id: Id,
    pub snapshot_id: Option<Id>,
    pub source: PricingSource,
    pub channel: PricingChannel,
    pub provider_pattern: String,
    pub model_pattern: String,
    pub client_pattern: String,
    pub region_pattern: Option<String>,
    pub service_tier: Option<String>,
    pub currency: String,
    /// 价格单位：per_million_tokens / per_request / per_token
    pub unit: String,
    /// 每百万 token 的价格（微美元）
    pub input_price: Option<i64>,
    pub output_price: Option<i64>,
    pub cache_read_price: Option<i64>,
    pub cache_write_price: Option<i64>,
    pub reasoning_price: Option<i64>,
    /// 每次请求固定费用（微美元）
    pub request_price: Option<i64>,
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_to: Option<DateTime<Utc>>,
    pub priority: i64,
    pub enabled: bool,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PricingRule {
    pub fn is_wildcard(&self) -> bool {
        self.model_pattern.contains('*') || self.model_pattern.contains('?')
    }

    /// 是否在 `at` 时间生效。
    pub fn effective_at(&self, at: DateTime<Utc>) -> bool {
        self.enabled
            && self.effective_from.is_none_or(|f| at >= f)
            && self.effective_to.is_none_or(|t| at <= t)
    }
}

/// 价格匹配结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingMatch {
    pub id: Id,
    pub usage_event_id: String,
    pub pricing_rule_id: Id,
    pub pricing_snapshot_id: Option<Id>,
    /// exact / wildcard / reported / none
    pub match_type: String,
    pub calculated_at: DateTime<Utc>,
    pub input_cost: Option<i64>,
    pub output_cost: Option<i64>,
    pub cache_read_cost: Option<i64>,
    pub cache_write_cost: Option<i64>,
    pub reasoning_cost: Option<i64>,
    pub request_cost: Option<i64>,
    pub total_cost: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn wildcard_detection() {
        let mut r = PricingRule {
            id: Id::new(),
            snapshot_id: None,
            source: PricingSource::UserOverride,
            channel: PricingChannel::VendorDirect,
            provider_pattern: "anthropic".into(),
            model_pattern: "claude-*".into(),
            client_pattern: "*".into(),
            region_pattern: None,
            service_tier: None,
            currency: "usd".into(),
            unit: "per_million_tokens".into(),
            input_price: Some(3_000_000),
            output_price: Some(15_000_000),
            cache_read_price: None,
            cache_write_price: None,
            reasoning_price: None,
            request_price: None,
            effective_from: None,
            effective_to: None,
            priority: 10,
            enabled: true,
            metadata: serde_json::json!({}),
            created_at: t(),
            updated_at: t(),
        };
        assert!(r.is_wildcard());
        assert!(r.effective_at(t()));
        r.enabled = false;
        assert!(!r.effective_at(t()));
    }
}
