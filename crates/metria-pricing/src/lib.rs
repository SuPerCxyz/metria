//! metria-pricing: 模型价格引擎。
//!
//! 优先级：reported_cost > user 精确规则 > user 通配规则 > builtin > unavailable。
//! M1 内置目录覆盖常见模型（来源标注 builtin_catalog，仅近似参考）。

use chrono::Utc;
use metria_core::model::{Id, PricingChannel, PricingMatch, PricingRule, PricingSource, Usage};
use metria_core::normalize::pattern_match;
use metria_core::MicroUsd;

/// 价格引擎错误。
#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    #[error("金额溢出: {0}")]
    Overflow(String),
}

/// 费用计算结果。
#[derive(Debug, Clone, Default)]
pub struct CostResult {
    pub reported_micro_usd: Option<i64>,
    pub calculated_micro_usd: Option<i64>,
    pub estimated_micro_usd: Option<i64>,
    pub rule_id: Option<String>,
    pub snapshot_id: Option<String>,
    /// 价格缺失标记
    pub pricing_available: bool,
}

/// 内置价格定义（每百万 token，微美元）。
struct BuiltinPrice {
    provider: &'static str,
    model_pattern: &'static str,
    input: i64,
    output: i64,
    cache_read: Option<i64>,
    cache_write: Option<i64>,
}

/// 内置目录（版本 1，来源 builtin_catalog；近似公开定价，非厂商直连保证）。
const BUILTIN: &[BuiltinPrice] = &[
    BuiltinPrice {
        provider: "anthropic",
        model_pattern: "claude-opus-4*",
        input: 15_000_000,
        output: 75_000_000,
        cache_read: Some(1_500_000),
        cache_write: Some(18_750_000),
    },
    BuiltinPrice {
        provider: "anthropic",
        model_pattern: "claude-sonnet-4*",
        input: 3_000_000,
        output: 15_000_000,
        cache_read: Some(300_000),
        cache_write: Some(3_750_000),
    },
    BuiltinPrice {
        provider: "anthropic",
        model_pattern: "claude-haiku-4*",
        input: 1_000_000,
        output: 5_000_000,
        cache_read: Some(100_000),
        cache_write: Some(1_250_000),
    },
    BuiltinPrice {
        provider: "openai",
        model_pattern: "gpt-4o",
        input: 2_500_000,
        output: 10_000_000,
        cache_read: Some(1_250_000),
        cache_write: None,
    },
    BuiltinPrice {
        provider: "openai",
        model_pattern: "gpt-4.1*",
        input: 2_000_000,
        output: 8_000_000,
        cache_read: Some(500_000),
        cache_write: None,
    },
    BuiltinPrice {
        provider: "openai",
        model_pattern: "o3-mini",
        input: 1_100_000,
        output: 4_400_000,
        cache_read: Some(550_000),
        cache_write: None,
    },
    BuiltinPrice {
        provider: "openai",
        model_pattern: "o4-mini",
        input: 1_100_000,
        output: 4_400_000,
        cache_read: Some(550_000),
        cache_write: None,
    },
    BuiltinPrice {
        provider: "openai",
        model_pattern: "gpt-5*",
        input: 1_250_000,
        output: 10_000_000,
        cache_read: Some(625_000),
        cache_write: None,
    },
    BuiltinPrice {
        provider: "deepseek",
        model_pattern: "deepseek-chat",
        input: 270_000,
        output: 1_100_000,
        cache_read: Some(70_000),
        cache_write: None,
    },
    BuiltinPrice {
        provider: "deepseek",
        model_pattern: "deepseek-reasoner",
        input: 550_000,
        output: 2_190_000,
        cache_read: Some(140_000),
        cache_write: None,
    },
    BuiltinPrice {
        provider: "codex",
        model_pattern: "gpt-5-codex*",
        input: 1_250_000,
        output: 10_000_000,
        cache_read: Some(625_000),
        cache_write: None,
    },
];

/// 价格引擎。
#[derive(Debug, Default)]
pub struct PricingEngine {
    /// 用户规则（按优先级降序）。
    user_rules: Vec<PricingRule>,
    /// 内置目录是否启用。
    builtin_enabled: bool,
}

impl PricingEngine {
    pub fn new() -> Self {
        Self {
            user_rules: Vec::new(),
            builtin_enabled: true,
        }
    }

    /// 添加用户规则（调用方保证按优先级插入）。
    pub fn add_user_rule(&mut self, rule: PricingRule) {
        self.user_rules.push(rule);
    }

    pub fn set_builtin_enabled(&mut self, enabled: bool) {
        self.builtin_enabled = enabled;
    }

    /// 计算一次调用的费用。
    ///
    /// 优先级：已有 reported > user 精确 > user 通配 > builtin > unavailable。
    pub fn compute(
        &self,
        usage: &Usage,
        model: Option<&str>,
        provider: Option<&str>,
        at: chrono::DateTime<chrono::Utc>,
        reported_micro_usd: Option<i64>,
    ) -> Result<CostResult, PricingError> {
        if reported_micro_usd.is_some() {
            return Ok(CostResult {
                reported_micro_usd,
                pricing_available: true,
                ..Default::default()
            });
        }

        let rule: Option<PricingRule> = match self
            .user_rules
            .iter()
            .filter(|r| r.effective_at(at))
            .filter(|r| model_is_match(r, model))
            .filter(|r| provider_is_match(r, provider))
            .filter(|r| has_price(r))
            .min_by_key(|r| -r.priority)
        {
            Some(r) => Some(r.clone()),
            None if self.builtin_enabled => self.builtin_rule(model, provider, at),
            None => None,
        };

        let Some(rule) = rule else {
            // 无价格 → 不硬造
            return Ok(CostResult::default());
        };

        let mut calculated = 0i64;
        let mut overflow = false;
        for (tokens, price) in [
            (usage.input.unwrap_or(0) as u64, rule.input_price),
            (usage.output.unwrap_or(0) as u64, rule.output_price),
            (usage.cache_read.unwrap_or(0) as u64, rule.cache_read_price),
            (
                usage.cache_write.unwrap_or(0) as u64,
                rule.cache_write_price,
            ),
            (usage.reasoning.unwrap_or(0) as u64, rule.reasoning_price),
        ] {
            if let Some(p) = price {
                match MicroUsd::from_price_per_m(p, tokens) {
                    Ok(c) => calculated += c.value(),
                    Err(_) => overflow = true,
                }
            }
        }
        if overflow {
            return Err(PricingError::Overflow("价格计算溢出".into()));
        }

        // 有 request_price 固定费
        if let Some(rp) = rule.request_price {
            calculated += rp;
        }

        // 是否估算口径：token 或价格任一不可靠
        let estimated = !usage_complete(usage);

        Ok(CostResult {
            reported_micro_usd: None,
            calculated_micro_usd: if estimated { None } else { Some(calculated) },
            estimated_micro_usd: if estimated { Some(calculated) } else { None },
            rule_id: Some(rule.id.as_str().to_string()),
            snapshot_id: None,
            pricing_available: true,
        })
    }

    fn builtin_rule(
        &self,
        model: Option<&str>,
        provider: Option<&str>,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Option<PricingRule> {
        let model = model?;
        for b in BUILTIN {
            if !pattern_match(b.model_pattern, model) {
                continue;
            }
            if let Some(p) = provider {
                if p != b.provider && !pattern_match(b.provider, p) {
                    continue;
                }
            }
            return Some(PricingRule {
                id: Id::new(),
                snapshot_id: None,
                source: PricingSource::BuiltinCatalog,
                channel: PricingChannel::VendorDirect,
                provider_pattern: b.provider.to_string(),
                model_pattern: b.model_pattern.to_string(),
                client_pattern: "*".to_string(),
                region_pattern: None,
                service_tier: None,
                currency: "usd".to_string(),
                unit: "per_million_tokens".to_string(),
                input_price: Some(b.input),
                output_price: Some(b.output),
                cache_read_price: b.cache_read,
                cache_write_price: b.cache_write,
                reasoning_price: None,
                request_price: None,
                effective_from: None,
                effective_to: None,
                priority: 0,
                enabled: true,
                metadata: serde_json::json!({"note": "内置近似价格，非厂商直连保证"}),
                created_at: at,
                updated_at: at,
            });
        }
        None
    }

    /// 从用法构造一个 PricingMatch 记录。
    pub fn to_match(&self, usage_event_id: &str, result: &CostResult) -> Option<PricingMatch> {
        if !result.pricing_available {
            return None;
        }
        Some(PricingMatch {
            id: Id::new(),
            usage_event_id: usage_event_id.to_string(),
            pricing_rule_id: result
                .rule_id
                .as_deref()
                .map(|s| Id::parse(s).unwrap_or_default())
                .unwrap_or_default(),
            pricing_snapshot_id: None,
            match_type: if result.reported_micro_usd.is_some() {
                "reported"
            } else if result.rule_id.is_some() {
                "rule"
            } else {
                "none"
            }
            .to_string(),
            calculated_at: Utc::now(),
            input_cost: result.calculated_micro_usd.or(result.estimated_micro_usd),
            output_cost: None,
            cache_read_cost: None,
            cache_write_cost: None,
            reasoning_cost: None,
            request_cost: None,
            total_cost: result.calculated_micro_usd.or(result.estimated_micro_usd),
        })
    }
}

fn model_is_match(rule: &PricingRule, model: Option<&str>) -> bool {
    match model {
        Some(m) => pattern_match(&rule.model_pattern, m),
        None => rule.model_pattern == "*",
    }
}

fn provider_is_match(rule: &PricingRule, provider: Option<&str>) -> bool {
    match provider {
        Some(p) => pattern_match(&rule.provider_pattern, p),
        None => rule.provider_pattern == "*",
    }
}

fn has_price(rule: &PricingRule) -> bool {
    rule.input_price.is_some()
        || rule.output_price.is_some()
        || rule.cache_read_price.is_some()
        || rule.cache_write_price.is_some()
        || rule.reasoning_price.is_some()
        || rule.request_price.is_some()
}

/// 是否所有关键 token 都有值（缺失则费用只能按估算口径）。
fn usage_complete(u: &Usage) -> bool {
    u.input.is_some() && u.output.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use metria_core::model::Usage;

    fn usage() -> Usage {
        Usage {
            input: Some(1000),
            output: Some(2000),
            cache_read: Some(100),
            cache_write: Some(50),
            reasoning: Some(30),
        }
    }

    #[test]
    fn reported_priority() {
        let e = PricingEngine::new();
        let r = e
            .compute(
                &usage(),
                Some("claude-sonnet-4.5"),
                Some("anthropic"),
                Utc::now(),
                Some(12345),
            )
            .unwrap();
        assert_eq!(r.reported_micro_usd, Some(12345));
        assert!(r.calculated_micro_usd.is_none());
    }

    #[test]
    fn builtin_match_calculates() {
        let e = PricingEngine::new();
        let r = e
            .compute(
                &usage(),
                Some("claude-sonnet-4.5"),
                Some("anthropic"),
                Utc::now(),
                None,
            )
            .unwrap();
        assert!(r.pricing_available);
        let calc = r.calculated_micro_usd.expect("应计算费用");
        // 1000*3 + 2000*15 + 100*0.3 + 50*3.75 = 3000+30000+30+187.5 ≈ 33218
        assert!(calc > 30_000 && calc < 35_000, "calc={calc}");
    }

    #[test]
    fn unknown_model_no_price() {
        let e = PricingEngine::new();
        let r = e
            .compute(
                &usage(),
                Some("totally-unknown-model"),
                Some("x"),
                Utc::now(),
                None,
            )
            .unwrap();
        assert!(!r.pricing_available);
        assert!(r.calculated_micro_usd.is_none());
    }

    #[test]
    fn missing_tokens_estimated() {
        let e = PricingEngine::new();
        let mut u = usage();
        u.input = None;
        let r = e
            .compute(
                &u,
                Some("claude-sonnet-4.5"),
                Some("anthropic"),
                Utc::now(),
                None,
            )
            .unwrap();
        assert!(r.calculated_micro_usd.is_none());
        assert!(r.estimated_micro_usd.is_some(), "缺 token 应标记估算");
    }

    #[test]
    fn user_rule_beats_builtin() {
        let e = PricingEngine::new();
        let mut rule = PricingRule {
            id: Id::new(),
            snapshot_id: None,
            source: PricingSource::UserOverride,
            channel: PricingChannel::VendorDirect,
            provider_pattern: "*".into(),
            model_pattern: "claude-*".into(),
            client_pattern: "*".into(),
            region_pattern: None,
            service_tier: None,
            currency: "usd".into(),
            unit: "per_million_tokens".into(),
            input_price: Some(1_000_000),
            output_price: Some(2_000_000),
            cache_read_price: None,
            cache_write_price: None,
            reasoning_price: None,
            request_price: None,
            effective_from: None,
            effective_to: None,
            priority: 10,
            enabled: true,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        rule.id = Id::new();
        let mut e = e;
        e.add_user_rule(rule);
        let r = e
            .compute(
                &usage(),
                Some("claude-sonnet-4.5"),
                Some("anthropic"),
                Utc::now(),
                None,
            )
            .unwrap();
        let calc = r.calculated_micro_usd.unwrap();
        // 1000*1 + 2000*2 = 5000
        assert_eq!(calc, 5_000);
    }
}
