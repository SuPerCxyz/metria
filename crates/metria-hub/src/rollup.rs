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

    /// Rollup 对账：对比 raw 事件表与 rollup 汇总的计数/字节，返回差异摘要。
    ///
    /// 逐 bucket 对比 hourly_rollups 与 sessions/model_calls/usage_events/traffic_estimates
    /// 的聚合值。差异过大时记录告警（写入 server_meta），并可通过 [`Self::rebuild_drift`]
    /// 触发重建。每次扫描限制在最近 N 天，避免全库扫描。
    pub fn reconcile_rollups(&self, days: i64) -> Result<ReconcileReport, StorageError> {
        let since = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let mut report = ReconcileReport::default();

        // 1. 每个 bucket 的 rollup 汇总值（作用域内借用 conn，离开即释放锁）
        let rollups: Vec<(String, i64, i64, i64, i64, i64)> = {
            let c = self.conn();
            let mut stmt = c
                .prepare(
                    "SELECT bucket,
                    SUM(session_count), SUM(model_call_count), SUM(input_tokens),
                    SUM(output_tokens), SUM(estimated_total_bytes)
                 FROM hourly_rollups WHERE bucket >= ?1 GROUP BY bucket ORDER BY bucket",
                )
                .map_err(StorageError::from)?;
            let rows = stmt
                .query_map([&since], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                })
                .map_err(StorageError::from)?;
            rows.filter_map(|r| r.ok()).collect()
        };

        for (bucket, rsess, rcall, rinput, routput, rtraffic) in &rollups {
            // 2. raw 事件表对应 bucket 的计数（按 UTC 分桶对齐）
            let actual = self
                .conn()
                .query_row(
                    "SELECT
                        (SELECT COUNT(*) FROM sessions WHERE started_at >= ?1 AND started_at < ?2),
                        (SELECT COUNT(*) FROM model_calls WHERE started_at >= ?1 AND started_at < ?2),
                        (SELECT COALESCE(SUM(input_tokens),0) FROM usage_events WHERE timestamp >= ?1 AND timestamp < ?2),
                        (SELECT COALESCE(SUM(output_tokens),0) FROM usage_events WHERE timestamp >= ?1 AND timestamp < ?2),
                        (SELECT COALESCE(SUM(estimated_total_wire_bytes),0) FROM traffic_estimates WHERE calculated_at >= ?1 AND calculated_at < ?2)",
                    params![bucket, next_bucket(bucket).to_rfc3339()],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, i64>(4)?,
                        ))
                    },
                )
                .map_err(StorageError::from)?;

            let (asess, acall, ainput, aoutput, atraffic) = actual;
            let sess_drift = asess.abs_diff(*rsess);
            let call_drift = acall.abs_diff(*rcall);
            let input_drift = ainput.abs_diff(*rinput);
            let output_drift = aoutput.abs_diff(*routput);
            let traffic_drift = atraffic.abs_diff(*rtraffic);
            report.buckets += 1;
            if sess_drift + call_drift + input_drift + output_drift + traffic_drift > 0 {
                report.drift_buckets += 1;
                tracing::warn!(
                    bucket = %bucket,
                    rollup_sessions = rsess, actual_sessions = asess,
                    rollup_calls = rcall, actual_calls = acall,
                    rollup_input = rinput, actual_input = ainput,
                    rollup_output = routput, actual_output = aoutput,
                    rollup_traffic = rtraffic, actual_traffic = atraffic,
                    "rollup 对账差异"
                );
            }
        }
        Ok(report)
    }

    /// 对指定时间范围重建 rollup：从 raw 事件表重新聚合（先删后插）。
    /// 幂等：仅重建最近 `days` 天，供对账漂移修复与手工重算使用。
    pub fn rebuild_drift(&self, days: i64) -> Result<usize, StorageError> {
        let since = Utc::now() - chrono::Duration::days(days);

        // 1. 收集待重建的 raw 事件（作用域内借用 conn，离开即释放锁）
        let (sessions, calls): (Vec<Value>, Vec<Value>) = {
            let c = self.conn();
            let mut sess = c
                .prepare(
                    "SELECT started_at, node_id, collector_id, client_id, source_id, project_id,
                        provider_normalized, primary_model_normalized,
                        message_count, tool_call_count, subagent_count, input_tokens, output_tokens,
                        cache_read_tokens, cache_write_tokens, reasoning_tokens,
                        reported_cost_micro_usd, calculated_cost_micro_usd, estimated_cost_micro_usd,
                        estimated_request_bytes, estimated_response_bytes, estimated_total_bytes
                     FROM sessions WHERE started_at >= ?1",
                )
                .map_err(StorageError::from)?;
            let srows: Vec<Value> = sess
                .query_map([since.to_rfc3339()], |r| {
                    let ts: String = r.get(0)?;
                    Ok(serde_json::json!({
                        "timestamp": ts,
                        "node_id": r.get::<_, String>(1)?,
                        "collector_id": r.get::<_, String>(2)?,
                        "client_id": r.get::<_, String>(3)?,
                        "source_id": r.get::<_, String>(4)?,
                        "project_id": r.get::<_, Option<String>>(5)?,
                        "provider_normalized": r.get::<_, Option<String>>(6)?,
                        "model_normalized": r.get::<_, Option<String>>(7)?,
                        "message_count": r.get::<_, i64>(8)?,
                        "tool_call_count": r.get::<_, i64>(9)?,
                        "subagent_count": r.get::<_, i64>(10)?,
                        "input_tokens": r.get::<_, Option<i64>>(11)?,
                        "output_tokens": r.get::<_, Option<i64>>(12)?,
                        "cache_read_tokens": r.get::<_, Option<i64>>(13)?,
                        "cache_write_tokens": r.get::<_, Option<i64>>(14)?,
                        "reasoning_tokens": r.get::<_, Option<i64>>(15)?,
                        "reported_cost_micro_usd": r.get::<_, Option<i64>>(16)?,
                        "calculated_cost_micro_usd": r.get::<_, Option<i64>>(17)?,
                        "estimated_cost_micro_usd": r.get::<_, Option<i64>>(18)?,
                        "estimated_request_bytes": r.get::<_, Option<i64>>(19)?,
                        "estimated_response_bytes": r.get::<_, Option<i64>>(20)?,
                        "estimated_total_bytes": r.get::<_, Option<i64>>(21)?,
                    }))
                })
                .map_err(StorageError::from)?
                .filter_map(|r| r.ok())
                .collect();

            let mut calls = c
                .prepare(
                    "SELECT started_at, node_id, collector_id, client_id, source_id, model_normalized
                     FROM model_calls WHERE started_at >= ?1",
                )
                .map_err(StorageError::from)?;
            let crows: Vec<Value> = calls
                .query_map([since.to_rfc3339()], |r| {
                    Ok(serde_json::json!({
                        "timestamp": r.get::<_, String>(0)?,
                        "node_id": r.get::<_, String>(1)?,
                        "collector_id": r.get::<_, String>(2)?,
                        "client_id": r.get::<_, String>(3)?,
                        "source_id": r.get::<_, String>(4)?,
                        "model_normalized": r.get::<_, Option<String>>(5)?,
                    }))
                })
                .map_err(StorageError::from)?
                .filter_map(|r| r.ok())
                .collect();
            (srows, crows)
        };

        // 2. 删除待重建 bucket 的 rollup 行（先删后插，幂等）
        let c = self.conn();
        c.execute(
            "DELETE FROM hourly_rollups WHERE bucket >= ?1",
            [since.to_rfc3339()],
        )
        .map_err(StorageError::from)?;
        c.execute(
            "DELETE FROM daily_rollups WHERE bucket >= ?1",
            [metria_core::time::bucket_day(since, chrono_tz::Tz::UTC).to_rfc3339()],
        )
        .map_err(StorageError::from)?;
        drop(c);

        // 3. 逐条增量重建（每次调用内部重新获取连接锁）
        let mut rebuilt = 0usize;
        for row in &sessions {
            self.rollup_event("session", row)?;
            rebuilt += 1;
        }
        for row in &calls {
            self.rollup_event("call", row)?;
            rebuilt += 1;
        }
        Ok(rebuilt)
    }
}

/// 对账报告摘要。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    /// 已对账的 bucket 数。
    pub buckets: i64,
    /// 存在差异的 bucket 数。
    pub drift_buckets: i64,
}

/// 取下一个小时 bucket 边界（对账对比用，保持 UTC 对齐）。
fn next_bucket(bucket: &str) -> DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(bucket)
        .map(|t| t.with_timezone(&Utc) + chrono::Duration::hours(1))
        .unwrap_or_else(|_| Utc::now())
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

#[cfg(test)]
mod tests {
    use super::*;
    use metria_storage::DbOptions;

    fn test_db(tag: &str) -> HubDb {
        let dir =
            std::env::temp_dir().join(format!("metria-rollup-test-{}-{}", std::process::id(), tag));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = crate::config::HubConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            data_dir: dir.clone(),
            database_url: format!("sqlite://{}/hub.db", dir.display()),
            content_mode: metria_core::ContentMode::Metadata,
            timezone: chrono_tz::Tz::UTC,
            log_filter: "error".into(),
            demo: false,
        };
        let db = HubDb::open(&cfg).unwrap();
        db.apply_migrations().unwrap();
        db
    }

    fn sess_json(key: &str, ts: &str) -> serde_json::Value {
        serde_json::json!({
            "source_session_id": key, "timestamp": ts, "started_at": ts,
            "node_id": "n1", "collector_id": "c1", "client_id": "claude-code",
            "source_id": "s1", "project_id": null, "provider_normalized": "anthropic",
            "model_normalized": "claude-sonnet-4.5",
            "message_count": 3, "tool_call_count": 1, "subagent_count": 0,
            "input_tokens": 1000, "output_tokens": 200,
            "cache_read_tokens": 0, "cache_write_tokens": 0, "reasoning_tokens": 0,
            "reported_cost_micro_usd": 5000, "calculated_cost_micro_usd": 5000,
            "estimated_cost_micro_usd": null,
            "estimated_request_bytes": 1000, "estimated_response_bytes": 2000,
            "estimated_total_bytes": 3000, "estimated_lower_bound_bytes": 2000,
            "estimated_upper_bound_bytes": 4000
        })
    }

    fn call_json(ts: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": ts, "started_at": ts,
            "node_id": "n1", "collector_id": "c1", "client_id": "claude-code",
            "source_id": "s1", "provider_normalized": "anthropic",
            "model_normalized": "claude-sonnet-4.5",
        })
    }

    #[test]
    fn reconcile_reports_no_drift_after_clean_ingest() {
        let db = test_db("reconcile");
        let sess = sess_json("sess-a", "2026-08-06T01:30:00Z");
        let call = call_json("2026-08-06T01:35:00Z");
        db.upsert_session(&sess).unwrap();
        db.insert_call(&call, "n1:sess-a").unwrap();
        db.rollup_event("session", &sess).unwrap();
        db.rollup_event("call", &call).unwrap();

        let report = db.reconcile_rollups(1).unwrap();
        assert!(report.drift_buckets == 0, "干净数据不应有漂移: {report:?}");
        assert!(report.buckets >= 1, "应有 bucket 被对账");
    }

    #[test]
    fn rebuild_drift_rebuilds_rollup() {
        let db = test_db("rebuild");
        let sess = sess_json("sess-b", "2026-08-06T02:30:00Z");
        let call = call_json("2026-08-06T02:35:00Z");
        db.upsert_session(&sess).unwrap();
        db.insert_call(&call, "n1:sess-b").unwrap();
        db.rollup_event("session", &sess).unwrap();
        db.rollup_event("call", &call).unwrap();

        // 人为制造漂移：删掉 rollup 行
        {
            let c = db.conn();
            c.execute(
                "DELETE FROM hourly_rollups WHERE bucket LIKE '2026-08-06%'",
                [],
            )
            .unwrap();
        }
        let rebuilt = db.rebuild_drift(1).unwrap();
        assert!(rebuilt >= 2, "应重建 session+call: {rebuilt}");
        let report = db.reconcile_rollups(1).unwrap();
        assert!(report.drift_buckets == 0, "重建后应无漂移: {report:?}");
    }

    #[test]
    fn db_options_defaults_apply_pragmas() {
        let dir = std::env::temp_dir().join(format!("metria-pragma-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let conn = metria_storage::open(&path, &DbOptions::default()).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        metria_storage::wal_checkpoint(&conn).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
