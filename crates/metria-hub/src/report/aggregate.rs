//! 报告聚合：复用 Web 趋势查询生成指定时间范围的报告序列。

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::api::{handlers_query::query_usage_timeseries, RangeParams};
use crate::db::HubDb;

/// 报告聚合结果。费用/流量为微美元与字节；0 表示该口径无数据。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReportMetrics {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub calls: i64,
    pub sessions: i64,
    pub reported_cost_micro_usd: i64,
    pub calculated_cost_micro_usd: i64,
    pub estimated_cost_micro_usd: i64,
    pub estimated_total_bytes: i64,
    pub estimated_lower_bound_bytes: i64,
    pub estimated_upper_bound_bytes: i64,
    pub top_models: Vec<DimCount>,
    pub top_clients: Vec<DimCount>,
    pub has_data: bool,
}

impl ReportMetrics {
    pub fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.reasoning_tokens
    }

    pub(crate) fn token_components(&self) -> i64 {
        self.total_tokens() + self.cache_read_tokens + self.cache_write_tokens
    }
}

/// 维度计数（模型 / 客户端）。
#[derive(Debug, Clone, Serialize)]
pub struct DimCount {
    pub name: String,
    pub calls: i64,
    pub tokens: i64,
}

/// 一条命名序列（图表用）。
#[derive(Debug, Clone, Serialize)]
pub struct NamedSeries {
    pub name: String,
    pub values: Vec<i64>,
}

/// 报告时间轴标签 + 多条序列。
#[derive(Debug, Clone, Serialize)]
pub struct ChartSeries {
    pub days: Vec<String>,
    pub series: Vec<NamedSeries>,
}

impl ChartSeries {
    pub fn max_value(&self) -> i64 {
        self.series
            .iter()
            .flat_map(|s| s.values.iter())
            .copied()
            .max()
            .unwrap_or(0)
    }
}

/// 聚合 `[from, to)` 范围内全部节点的用量。
pub fn aggregate(db: &HubDb, from: DateTime<Utc>, to: DateTime<Utc>, top_n: i64) -> ReportMetrics {
    let c = db.conn();
    let from_s = from.to_rfc3339();
    let to_s = to.to_rfc3339();

    let mut m = ReportMetrics::default();
    let totals = c.query_row(
        "SELECT
            COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
            COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
            COALESCE(SUM(reasoning_tokens),0),
            COALESCE(SUM(reported_cost),0), COALESCE(SUM(calculated_cost),0), COALESCE(SUM(estimated_cost),0),
            COALESCE(SUM(estimated_total_bytes),0), COALESCE(SUM(estimated_lower_bound_bytes),0), COALESCE(SUM(estimated_upper_bound_bytes),0),
            COALESCE(SUM(model_call_count),0), COALESCE(SUM(session_count),0)
         FROM hourly_rollups WHERE julianday(bucket) >= julianday(?1) AND julianday(bucket) < julianday(?2)",
        [&from_s, &to_s],
        |r| {
            Ok::<_, metria_storage::rusqlite::Error>((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, i64>(9)?,
                r.get::<_, i64>(10)?,
                r.get::<_, i64>(11)?,
                r.get::<_, i64>(12)?,
            ))
        },
    );
    if let Ok(t) = totals {
        m.input_tokens = t.0;
        m.output_tokens = t.1;
        m.cache_read_tokens = t.2;
        m.cache_write_tokens = t.3;
        m.reasoning_tokens = t.4;
        m.reported_cost_micro_usd = t.5;
        m.calculated_cost_micro_usd = t.6;
        m.estimated_cost_micro_usd = t.7;
        m.estimated_total_bytes = t.8;
        m.estimated_lower_bound_bytes = t.9;
        m.estimated_upper_bound_bytes = t.10;
        m.calls = t.11;
        m.sessions = t.12;
    }

    m.top_models = dim_breakdown(&c, &from_s, &to_s, "model", top_n);
    m.top_clients = dim_breakdown(&c, &from_s, &to_s, "client_id", top_n);
    m.has_data = m.calls > 0;
    m
}

fn dim_breakdown(
    c: &metria_storage::rusqlite::Connection,
    from: &str,
    to: &str,
    column: &str,
    top_n: i64,
) -> Vec<DimCount> {
    // column 来自内部常量，不接受外部输入，无注入风险。
    let sql = format!(
        "SELECT {column}, COALESCE(SUM(model_call_count),0) AS calls,
                COALESCE(SUM(input_tokens+output_tokens+reasoning_tokens),0) AS tokens
         FROM hourly_rollups WHERE julianday(bucket) >= julianday(?1) AND julianday(bucket) < julianday(?2) AND {column} <> ''
         GROUP BY {column} ORDER BY calls DESC, tokens DESC LIMIT ?3"
    );
    let Ok(mut stmt) = c.prepare(&sql) else {
        return Vec::new();
    };
    let rows = stmt.query_map(metria_storage::rusqlite::params![from, to, top_n], |r| {
        Ok(DimCount {
            name: r.get(0)?,
            calls: r.get(1)?,
            tokens: r.get(2)?,
        })
    });
    match rows {
        Ok(it) => it.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

fn query_params(
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
    dim: Option<&str>,
) -> RangeParams {
    RangeParams {
        from: Some(from.to_rfc3339()),
        to: Some(to.to_rfc3339()),
        timezone: Some(tz.to_string()),
        dim: dim.map(str::to_string),
        ..RangeParams::default()
    }
}

fn query_points(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
    dim: Option<&str>,
) -> Vec<Value> {
    query_usage_timeseries(db, &query_params(from, to, tz, dim)).unwrap_or_default()
}

fn bucket(point: &Value) -> Option<&str> {
    point.get("bucket").and_then(Value::as_str)
}

fn unique_buckets(points: &[Value]) -> Vec<String> {
    let mut buckets = Vec::new();
    for point in points {
        let Some(current) = bucket(point) else {
            continue;
        };
        if buckets.last().map(String::as_str) != Some(current) {
            buckets.push(current.to_string());
        }
    }
    buckets
}

fn label_bucket(bucket: &str, tz: chrono_tz::Tz, include_date: bool) -> String {
    if let Ok(value) = DateTime::parse_from_rfc3339(bucket) {
        let format = if include_date { "%m-%d %H:%M" } else { "%H:%M" };
        return value.with_timezone(&tz).format(format).to_string();
    }
    if let Ok(value) = NaiveDate::parse_from_str(bucket, "%Y-%m-%d") {
        return if include_date {
            value.format("%m-%d 00:00").to_string()
        } else {
            "00:00".to_string()
        };
    }
    bucket.to_string()
}

fn axis(
    points: &[Value],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
) -> (Vec<String>, Vec<String>) {
    let buckets = unique_buckets(points);
    let include_date = to - from > chrono::Duration::days(1);
    let labels = buckets
        .iter()
        .map(|b| label_bucket(b, tz, include_date))
        .collect();
    (buckets, labels)
}

fn value(point: &Value, key: &str) -> i64 {
    point.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn overall_values(points: &[Value], buckets: &[String], key: &str) -> Vec<i64> {
    let by_bucket: HashMap<&str, i64> = points
        .iter()
        .filter_map(|point| bucket(point).map(|b| (b, value(point, key))))
        .collect();
    buckets
        .iter()
        .map(|bucket| by_bucket.get(bucket.as_str()).copied().unwrap_or(0))
        .collect()
}

/// 与 Web `/usage/timeseries` 使用相同分桶、补零和时间轴。
pub fn daily_token_chart(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
) -> ChartSeries {
    let points = query_points(db, from, to, tz, None);
    let (buckets, days) = axis(&points, from, to, tz);
    let names = ["输入", "输出", "缓存读取"];
    let series = (0..3)
        .map(|i| NamedSeries {
            name: names[i].to_string(),
            values: overall_values(
                &points,
                &buckets,
                ["input_tokens", "output_tokens", "cache_read_tokens"][i],
            ),
        })
        .collect();
    ChartSeries { days, series }
}

/// 按日请求数趋势。
pub fn daily_calls_chart(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
) -> ChartSeries {
    let points = query_points(db, from, to, tz, None);
    let (buckets, days) = axis(&points, from, to, tz);
    let series = vec![NamedSeries {
        name: "请求数".to_string(),
        values: overall_values(&points, &buckets, "model_calls"),
    }];
    ChartSeries { days, series }
}

/// 按 Web 趋势查询的维度序列（模型/客户端）。
pub fn dim_daily_chart(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
    column: &str,
    by_tokens: bool,
) -> ChartSeries {
    let dimension = match column {
        "model" => "model",
        "client" | "client_id" => "client",
        _ => {
            return ChartSeries {
                days: Vec::new(),
                series: Vec::new(),
            }
        }
    };
    let points = query_points(db, from, to, tz, Some(dimension));
    let (buckets, days) = axis(&points, from, to, tz);
    let mut totals: HashMap<String, i64> = HashMap::new();
    let mut values: HashMap<(String, String), i64> = HashMap::new();
    for point in &points {
        let (Some(bucket), Some(dimension)) = (
            bucket(point),
            point.get("dimension").and_then(Value::as_str),
        ) else {
            continue;
        };
        if dimension.is_empty() {
            continue;
        }
        let current = if by_tokens {
            value(point, "input_tokens")
                + value(point, "output_tokens")
                + value(point, "reasoning_tokens")
        } else {
            value(point, "model_calls")
        };
        *totals.entry(dimension.to_string()).or_default() += current;
        values.insert((bucket.to_string(), dimension.to_string()), current);
    }
    let mut names: Vec<(String, i64)> = totals.into_iter().collect();
    names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let series = names
        .into_iter()
        .map(|(name, _)| NamedSeries {
            values: buckets
                .iter()
                .map(|b| values.get(&(b.clone(), name.clone())).copied().unwrap_or(0))
                .collect(),
            name,
        })
        .collect();
    ChartSeries { days, series }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_add_up() {
        let m = ReportMetrics {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_write_tokens: 4,
            reasoning_tokens: 5,
            ..Default::default()
        };
        assert_eq!(m.total_tokens(), 8);
        assert_eq!(m.token_components(), 15);
    }

    #[test]
    fn chart_max_value() {
        let cs = ChartSeries {
            days: vec!["09-01".into(), "09-02".into()],
            series: vec![
                NamedSeries {
                    name: "a".into(),
                    values: vec![1, 9],
                },
                NamedSeries {
                    name: "b".into(),
                    values: vec![3, 4],
                },
            ],
        };
        assert_eq!(cs.max_value(), 9);
    }

    #[test]
    fn report_labels_preserve_hour_and_day_buckets() {
        assert_eq!(
            label_bucket("2026-09-12T15:30:00+00:00", chrono_tz::UTC, true),
            "09-12 15:30"
        );
        assert_eq!(
            label_bucket("2026-09-12T15:30:00+00:00", chrono_tz::UTC, false),
            "15:30"
        );
        assert_eq!(
            label_bucket("2026-09-12", chrono_tz::UTC, true),
            "09-12 00:00"
        );
    }
}
