//! 时间工具：UTC 存储、IANA 分桶、范围校验。

use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

use crate::error::TimeError;

/// 当前 UTC 时间。
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// 校验时间范围：from 必须不晚于 to。
pub fn validate_range(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<(), TimeError> {
    if from > to {
        return Err(TimeError::InvalidRange);
    }
    Ok(())
}

/// 按 IANA 时区切小时桶（返回桶起点，UTC 时间）。
pub fn bucket_hour(t: DateTime<Utc>, tz: Tz) -> DateTime<Utc> {
    let local = t.with_timezone(&tz);
    tz.with_ymd_and_hms(local.year(), local.month(), local.day(), local.hour(), 0, 0)
        .single()
        .unwrap_or(local)
        .with_timezone(&Utc)
}

/// 按 IANA 时区切天桶（返回桶起点，UTC 时间）。
pub fn bucket_day(t: DateTime<Utc>, tz: Tz) -> DateTime<Utc> {
    let local = t.with_timezone(&tz);
    tz.with_ymd_and_hms(local.year(), local.month(), local.day(), 0, 0, 0)
        .single()
        .unwrap_or(local)
        .with_timezone(&Utc)
}

/// 按 IANA 时区切周桶（周一零点）。
pub fn bucket_week(t: DateTime<Utc>, tz: Tz) -> DateTime<Utc> {
    let local = t.with_timezone(&tz);
    let day = local.weekday().num_days_from_monday();
    let start = tz
        .with_ymd_and_hms(local.year(), local.month(), local.day(), 0, 0, 0)
        .single()
        .unwrap_or(local)
        - chrono::Duration::days(i64::from(day));
    start.with_timezone(&Utc)
}

/// 从 range 起点到终点生成小时桶边界列表。
pub fn hourly_buckets(from: DateTime<Utc>, to: DateTime<Utc>, tz: Tz) -> Vec<DateTime<Utc>> {
    let mut buckets = Vec::new();
    let start = bucket_hour(from, tz);
    let mut cur = start;
    while cur <= to {
        buckets.push(cur);
        cur += chrono::Duration::hours(1);
    }
    buckets
}

/// 从 range 起点到终点生成天桶边界列表。
pub fn daily_buckets(from: DateTime<Utc>, to: DateTime<Utc>, tz: Tz) -> Vec<DateTime<Utc>> {
    let mut buckets = Vec::new();
    let start = bucket_day(from, tz);
    let mut cur = start;
    while cur <= to {
        buckets.push(cur);
        cur += chrono::Duration::days(1);
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn range_validation() {
        let a = DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let b = a + Duration::hours(1);
        assert!(validate_range(a, b).is_ok());
        assert!(validate_range(b, a).is_err());
    }

    #[test]
    fn hour_bucket_in_shanghai() {
        // UTC 2026-08-05T16:30:00Z = 上海 2026-08-06 00:30
        let t = DateTime::parse_from_rfc3339("2026-08-05T16:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let bucket = bucket_hour(t, Tz::Asia__Shanghai);
        assert_eq!(
            bucket,
            DateTime::parse_from_rfc3339("2026-08-05T16:00:00Z").unwrap()
        );
    }

    #[test]
    fn day_bucket_utc() {
        let t = DateTime::parse_from_rfc3339("2026-08-05T16:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let bucket = bucket_day(t, Tz::UTC);
        assert_eq!(
            bucket,
            DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z").unwrap()
        );
    }

    #[test]
    fn bucket_sequences() {
        let a = DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let b = a + Duration::hours(25);
        let hours = hourly_buckets(a, b, Tz::UTC);
        assert_eq!(hours.len(), 26);
        let days = daily_buckets(a, b, Tz::UTC);
        assert_eq!(days.len(), 2);
    }
}
