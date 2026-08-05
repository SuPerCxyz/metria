//! Rollup 引擎：事件写入后增量更新 hourly/daily 汇总。

use chrono::{DateTime, Utc};
use metria_storage::rusqlite::params;
use metria_storage::StorageError;
use serde_json::Value;

use crate::db::HubDb;

/// 汇总增量类型。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RollupKind {
    Session,
    Call,
    Usage,
    Traffic,
}

impl HubDb {
    /// 对一条事件做增量 rollup（hourly + daily）。
    pub fn rollup_event(&self, kind: &str, v: &Value) -> Result<(), StorageError> {
        let rk = match kind {
            "session" => RollupKind::Session,
            "call" => RollupKind::Call,
            "usage" => RollupKind::Usage,
            "traffic" => RollupKind::Traffic,
            _ => return Ok(()),
        };
        let ts = event_time(v);
        self.rollup_insert(&ts, rk, v, "hourly_rollups")?;
        self.rollup_insert(&ts, rk, v, "daily_rollups")
    }

    fn rollup_insert(
        &self,
        ts: &DateTime<Utc>,
        kind: RollupKind,
        v: &Value,
        table: &str,
    ) -> Result<(), StorageError> {
        let bucket = match table {
            "hourly_rollups" => {
                metria_core::time::bucket_hour(*ts, chrono_tz::Tz::UTC).to_rfc3339()
            }
            _ => metria_core::time::bucket_day(*ts, chrono_tz::Tz::UTC).to_rfc3339(),
        };
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
        let node = g("node_id");
        let client = g("client_id");
        let source = g("source_id");
        let provider = g("provider_normalized").if_empty(|| g("provider"));
        let model = g("model_normalized").if_empty(|| g("model"));

        let (usage_source, granularity, pricing_source) = if kind == RollupKind::Usage {
            let q = v.get("quality").unwrap_or(&Value::Null);
            let cost = v.get("cost").unwrap_or(&Value::Null);
            let ps = if cost
                .get("reported_micro_usd")
                .and_then(|x| x.as_i64())
                .is_some()
            {
                "reported"
            } else if cost
                .get("calculated_micro_usd")
                .and_then(|x| x.as_i64())
                .is_some()
            {
                "calculated"
            } else if cost
                .get("estimated_micro_usd")
                .and_then(|x| x.as_i64())
                .is_some()
            {
                "estimated"
            } else {
                ""
            };
            (
                q.get("usage_source").and_then(|x| x.as_str()).unwrap_or(""),
                q.get("granularity").and_then(|x| x.as_str()).unwrap_or(""),
                ps,
            )
        } else {
            ("", "", "")
        };

        let (traffic_source, conf_level) = if kind == RollupKind::Traffic {
            (
                g("estimation_source"),
                confidence_level(v.get("confidence").and_then(|x| x.as_f64())),
            )
        } else {
            ("", "")
        };

        let getn = |k: &str| v.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        // model_call_count 仅由 call 事件累计，避免与 session 携带值重复计数
        let (session_count, message_count, tool_count, subagent_count, call_count) = match kind {
            RollupKind::Session => (
                1,
                getn("message_count"),
                getn("tool_call_count"),
                getn("subagent_count"),
                0,
            ),
            RollupKind::Call => (0, 0, 0, 0, 1),
            _ => (0, 0, 0, 0, 0),
        };

        let (input, output, cr, cw, rea) = if kind == RollupKind::Usage {
            let u = v.get("usage").unwrap_or(&Value::Null);
            let un = |k: &str| u.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
            (
                un("input"),
                un("output"),
                un("cache_read"),
                un("cache_write"),
                un("reasoning"),
            )
        } else {
            (0, 0, 0, 0, 0)
        };

        let (rep, calc, est) = if kind == RollupKind::Usage {
            let c = v.get("cost").unwrap_or(&Value::Null);
            let cn = |k: &str| c.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
            (
                cn("reported_micro_usd"),
                cn("calculated_micro_usd"),
                cn("estimated_micro_usd"),
            )
        } else {
            (0, 0, 0)
        };

        let (req_bytes, resp_bytes, total_bytes, lo, hi) = if kind == RollupKind::Traffic {
            (
                getn("estimated_request_wire_bytes"),
                getn("estimated_response_wire_bytes"),
                getn("estimated_total_wire_bytes"),
                getn("lower_bound_bytes"),
                getn("upper_bound_bytes"),
            )
        } else {
            (0, 0, 0, 0, 0)
        };

        let sql = format!(
            "INSERT INTO {table} (
                bucket, node_id, collector_id, client_id, source_id, project_id, provider, model,
                usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                reported_cost, calculated_cost, estimated_cost,
                estimated_request_bytes, estimated_response_bytes, estimated_total_bytes,
                estimated_lower_bound_bytes, estimated_upper_bound_bytes,
                session_count, model_call_count, turn_count, message_count, tool_call_count, subagent_count
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32)
            ON CONFLICT(bucket, node_id, collector_id, client_id, source_id, project_id, provider, model,
                usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level)
            DO UPDATE SET
                input_tokens = input_tokens + excluded.input_tokens,
                output_tokens = output_tokens + excluded.output_tokens,
                cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
                cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
                reasoning_tokens = reasoning_tokens + excluded.reasoning_tokens,
                reported_cost = reported_cost + excluded.reported_cost,
                calculated_cost = calculated_cost + excluded.calculated_cost,
                estimated_cost = estimated_cost + excluded.estimated_cost,
                estimated_request_bytes = estimated_request_bytes + excluded.estimated_request_bytes,
                estimated_response_bytes = estimated_response_bytes + excluded.estimated_response_bytes,
                estimated_total_bytes = estimated_total_bytes + excluded.estimated_total_bytes,
                estimated_lower_bound_bytes = estimated_lower_bound_bytes + excluded.estimated_lower_bound_bytes,
                estimated_upper_bound_bytes = estimated_upper_bound_bytes + excluded.estimated_upper_bound_bytes,
                session_count = session_count + excluded.session_count,
                model_call_count = model_call_count + excluded.model_call_count,
                turn_count = turn_count + excluded.turn_count,
                message_count = message_count + excluded.message_count,
                tool_call_count = tool_call_count + excluded.tool_call_count,
                subagent_count = subagent_count + excluded.subagent_count
        "
        );

        let c = self.conn();
        c.execute(
            &sql,
            params![
                bucket,
                node,
                g("collector_id"),
                client,
                source,
                g("project_id"),
                provider,
                model,
                usage_source,
                granularity,
                pricing_source,
                traffic_source,
                conf_level,
                input,
                output,
                cr,
                cw,
                rea,
                rep,
                calc,
                est,
                req_bytes,
                resp_bytes,
                total_bytes,
                lo,
                hi,
                session_count,
                call_count,
                0,
                message_count,
                tool_count,
                subagent_count,
            ],
        )
        .map_err(StorageError::from)?;
        Ok(())
    }

    /// 重算指定时间范围（先删后插——M1 简化：全量重建通过从事件表聚合）。
    #[allow(dead_code)]
    pub fn rebuild_range(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        // 占位：完整重建实现见后续里程碑
        Ok(())
    }
}

fn event_time(v: &Value) -> DateTime<Utc> {
    let ts = v
        .get("timestamp")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("started_at").and_then(|x| x.as_str()))
        .or_else(|| v.get("calculated_at").and_then(|x| x.as_str()))
        .unwrap_or("");
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn confidence_level(conf: Option<f64>) -> &'static str {
    match conf {
        Some(c) if c >= 0.7 => "high",
        Some(c) if c >= 0.4 => "medium",
        Some(_) => "low",
        None => "",
    }
}

trait IfEmpty {
    fn if_empty<'a>(&'a self, alt: impl FnOnce() -> &'a str) -> &'a str;
}
impl IfEmpty for str {
    fn if_empty<'a>(&'a self, alt: impl FnOnce() -> &'a str) -> &'a str {
        if self.is_empty() {
            alt()
        } else {
            self
        }
    }
}
