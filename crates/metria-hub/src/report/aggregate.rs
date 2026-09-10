//! 报告聚合：从 `hourly_rollups` 汇总指定 UTC 时间范围的指标与按日序列。

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

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
        self.input_tokens
            + self.output_tokens
            + self.cache_read_tokens
            + self.cache_write_tokens
            + self.reasoning_tokens
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

/// 按日图表数据：日期标签（MM-DD）+ 多条序列。
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
         FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2",
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
                COALESCE(SUM(input_tokens+output_tokens+cache_read_tokens+cache_write_tokens+reasoning_tokens),0) AS tokens
         FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 AND {column} <> ''
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

/// 完整时间轴标签（缺失桶补 0 用）：单日=24 个整点，周/月=范围内每一天。
fn full_axis(
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
    by_hour: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if by_hour {
        let mut cur = from;
        while cur < to && out.len() < 400 {
            out.push(cur.with_timezone(&tz).format("%H:00").to_string());
            cur += chrono::Duration::hours(1);
        }
    } else {
        let start = from.with_timezone(&tz).date_naive();
        let end = (to - chrono::Duration::seconds(1))
            .with_timezone(&tz)
            .date_naive();
        let mut d = start;
        while d <= end && out.len() < 400 {
            out.push(d.format("%m-%d").to_string());
            match d.succ_opt() {
                Some(n) => d = n,
                None => break,
            }
        }
    }
    out
}

fn local_label(bucket: &str, tz: chrono_tz::Tz, by_hour: bool) -> Option<String> {
    DateTime::parse_from_rfc3339(bucket).ok().map(|d| {
        let l = d.with_timezone(&tz);
        if by_hour {
            l.format("%H:00").to_string()
        } else {
            l.format("%m-%d").to_string()
        }
    })
}

/// 按日 Token 构成趋势（输入/输出/缓存读/缓存写/推理）。
pub fn daily_token_chart(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
    by_hour: bool,
) -> ChartSeries {
    let c = db.conn();
    let mut map: BTreeMap<String, [i64; 5]> = BTreeMap::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT bucket,
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(reasoning_tokens),0)
         FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 GROUP BY bucket",
    ) {
        if let Ok(it) = stmt.query_map([from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                [
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ],
            ))
        }) {
            for (bucket, vals) in it.filter_map(Result::ok) {
                if let Some(day) = local_label(&bucket, tz, by_hour) {
                    let e = map.entry(day).or_insert([0; 5]);
                    for i in 0..5 {
                        e[i] += vals[i];
                    }
                }
            }
        }
    }
    let days = full_axis(from, to, tz, by_hour);
    // 与页面 Token 视图一致：输入 / 输出 / 缓存读取
    let names = ["输入", "输出", "缓存读取"];
    let series = (0..3)
        .map(|i| NamedSeries {
            name: names[i].to_string(),
            values: days
                .iter()
                .map(|d| map.get(d).map(|v| v[i]).unwrap_or(0))
                .collect(),
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
    by_hour: bool,
) -> ChartSeries {
    let c = db.conn();
    let mut map: BTreeMap<String, i64> = BTreeMap::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT bucket, COALESCE(SUM(model_call_count),0) FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 GROUP BY bucket",
    ) {
        if let Ok(it) = stmt.query_map([from.to_rfc3339(), to.to_rfc3339()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        }) {
            for (bucket, v) in it.filter_map(Result::ok) {
                if let Some(day) = local_label(&bucket, tz, by_hour) {
                    *map.entry(day).or_insert(0) += v;
                }
            }
        }
    }
    let days = full_axis(from, to, tz, by_hour);
    let series = vec![NamedSeries {
        name: "请求数".to_string(),
        values: days
            .iter()
            .map(|d| map.get(d).copied().unwrap_or(0))
            .collect(),
    }];
    ChartSeries { days, series }
}

/// 按日维度趋势（模型/客户端 Top N，按 token 或调用排序）。
#[allow(clippy::too_many_arguments)]
pub fn dim_daily_chart(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
    column: &str,
    top_n: i64,
    by_tokens: bool,
    by_hour: bool,
) -> ChartSeries {
    let c = db.conn();
    // 与页面 Token 视图口径一致：输入 + 输出 + 缓存读取
    let metric = if by_tokens {
        "SUM(input_tokens+output_tokens+cache_read_tokens)"
    } else {
        "SUM(model_call_count)"
    };
    let from_s = from.to_rfc3339();
    let to_s = to.to_rfc3339();
    let mut names: Vec<String> = Vec::new();
    let top_sql = if top_n > 0 {
        format!(
            "SELECT {column} FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 AND {column} <> '' GROUP BY {column} ORDER BY {metric} DESC LIMIT ?3"
        )
    } else {
        format!(
            "SELECT {column} FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 AND {column} <> '' GROUP BY {column} ORDER BY {metric} DESC"
        )
    };
    if let Ok(mut stmt) = c.prepare(&top_sql) {
        let params: Vec<metria_storage::rusqlite::types::Value> = if top_n > 0 {
            vec![from_s.clone().into(), to_s.clone().into(), top_n.into()]
        } else {
            vec![from_s.clone().into(), to_s.clone().into()]
        };
        if let Ok(it) = stmt.query_map(
            metria_storage::rusqlite::params_from_iter(params.iter()),
            |r| r.get::<_, String>(0),
        ) {
            names = it.filter_map(Result::ok).collect();
        }
    }
    if names.is_empty() {
        return ChartSeries {
            days: Vec::new(),
            series: Vec::new(),
        };
    }
    let placeholders = (0..names.len())
        .map(|i| format!("?{}", i + 3))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT bucket, {column}, {metric} FROM hourly_rollups WHERE bucket >= ?1 AND bucket < ?2 AND {column} IN ({placeholders}) GROUP BY bucket, {column}"
    );
    let mut params: Vec<metria_storage::rusqlite::types::Value> = vec![from_s.into(), to_s.into()];
    for n in &names {
        params.push(n.clone().into());
    }
    let mut map: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    if let Ok(mut stmt) = c.prepare(&sql) {
        if let Ok(it) = stmt.query_map(
            metria_storage::rusqlite::params_from_iter(params.iter()),
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        ) {
            for (bucket, name, val) in it.filter_map(Result::ok) {
                if let Some(day) = local_label(&bucket, tz, by_hour) {
                    *map.entry(day).or_default().entry(name).or_insert(0) += val;
                }
            }
        }
    }
    let days = full_axis(from, to, tz, by_hour);
    let series = names
        .iter()
        .map(|n| NamedSeries {
            name: n.clone(),
            values: days
                .iter()
                .map(|d| map.get(d).and_then(|m| m.get(n)).copied().unwrap_or(0))
                .collect(),
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
        assert_eq!(m.total_tokens(), 15);
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
}
