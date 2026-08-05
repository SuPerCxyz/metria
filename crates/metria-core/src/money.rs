//! 金额：整数微美元（i64）。
//!
//! 禁止使用浮点数累计金额；运算使用 i128 中间量防溢出。

use serde::{Deserialize, Serialize};

use crate::error::MoneyError;

/// 微美元金额。
///
/// 1 USD = 1_000_000 微美元。价格为每 1M token 的美元数时，
/// 乘以 token 数 / 1_000_000 得到微美元（见 `from_price_per_m`）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct MicroUsd(i64);

impl MicroUsd {
    pub const ZERO: MicroUsd = MicroUsd(0);
    pub const MAX: MicroUsd = MicroUsd(i64::MAX);

    pub const fn new(v: i64) -> Result<Self, MoneyError> {
        if v < 0 {
            return Err(MoneyError::Negative(v));
        }
        Ok(Self(v))
    }

    pub const fn new_unchecked(v: i64) -> Self {
        Self(v)
    }

    pub fn value(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        let s = i128::from(self.0) + i128::from(other.0);
        if s > i128::from(i64::MAX) {
            return Err(MoneyError::Overflow("金额加法溢出".to_string()));
        }
        Ok(Self(s as i64))
    }

    /// 乘法：金额 × 无符号整数（如 token 数）。
    pub fn checked_mul(self, n: u64) -> Result<Self, MoneyError> {
        let s = i128::from(self.0) * i128::from(n);
        if s > i128::from(i64::MAX) {
            return Err(MoneyError::Overflow("金额乘法溢出".to_string()));
        }
        Ok(Self(s as i64))
    }

    /// 累计一批金额（跳过 None），溢出时返回错误。
    pub fn sum(values: impl IntoIterator<Item = Option<MicroUsd>>) -> Result<Self, MoneyError> {
        let mut total = 0i128;
        for x in values.into_iter().flatten() {
            total += i128::from(x.0);
        }
        if total > i128::from(i64::MAX) {
            return Err(MoneyError::Overflow("金额累计溢出".to_string()));
        }
        Ok(Self(total as i64))
    }

    /// 由「每百万 token 价格（美元）」与 token 数计算费用（微美元）。
    ///
    /// 价格单位：USD / 1M tokens。公式：price * tokens（价格以微美元/百万token
    /// 表示时，直接 price_micro * tokens / 1_000_000）。
    pub fn from_price_per_m(price_per_m_micro_usd: i64, tokens: u64) -> Result<Self, MoneyError> {
        if price_per_m_micro_usd < 0 {
            return Err(MoneyError::Negative(price_per_m_micro_usd));
        }
        // 用 i128 避免中间溢出；整除截断（整数微美元精度足够）。
        let num = i128::from(price_per_m_micro_usd) * i128::from(tokens);
        let cost = num / 1_000_000;
        if cost > i128::from(i64::MAX) {
            return Err(MoneyError::Overflow("价格计算溢出".to_string()));
        }
        Ok(Self(cost as i64))
    }

    /// 人类可读美元表示（仅用于展示，不做累计）。
    pub fn usd_f64(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

impl std::fmt::Display for MicroUsd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} 微美元", self.0)
    }
}

impl TryFrom<i64> for MicroUsd {
    type Error = MoneyError;
    fn try_from(v: i64) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}

impl From<MicroUsd> for i64 {
    fn from(v: MicroUsd) -> i64 {
        v.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_calculation() {
        // $5 / 1M tokens，1000 tokens → 5000 微美元 = $0.005
        let cost = MicroUsd::from_price_per_m(5_000_000, 1_000).unwrap();
        assert_eq!(cost.value(), 5_000);
        assert!((cost.usd_f64() - 0.005).abs() < 1e-9);
    }

    #[test]
    fn arithmetic_overflow_safe() {
        let big = MicroUsd::new(i64::MAX).unwrap();
        assert!(big.checked_add(MicroUsd::new(1).unwrap()).is_err());
        // i64::MAX 微美元 * i64::MAX token / 1e6 超出 i64 范围 → 必须报溢出
        let m = MicroUsd::from_price_per_m(i64::MAX, i64::MAX as u64).unwrap_err();
        assert!(m.to_string().contains("溢出"));
    }

    #[test]
    fn negative_rejected() {
        assert!(MicroUsd::new(-1).is_err());
        assert!(MicroUsd::from_price_per_m(-1, 10).is_err());
    }

    #[test]
    fn sum_skips_none() {
        let s = MicroUsd::sum([
            Some(MicroUsd::new(100).unwrap()),
            None,
            Some(MicroUsd::new(200).unwrap()),
        ])
        .unwrap();
        assert_eq!(s.value(), 300);
    }
}
