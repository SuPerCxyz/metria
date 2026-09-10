//! 报告调度：三个相互独立的周期任务与统一投递分发。

use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::interval;

use crate::config::HubConfig;
use crate::db::HubDb;
use plotters::style::RGBColor;

use super::aggregate::{self, aggregate, ChartSeries, ReportMetrics};
use super::channels::{send_email, send_webhook};
use super::charts;
use super::render::{render_html, render_text, webhook_payload, ReportChart, ReportMeta};
use super::{
    effective_timezone, effective_timezone_name, load_config, period_and_due, period_label,
    period_range, resolve_recipients, ChannelOutcome, Kind,
};

/// 统一投递：聚合 → 渲染 → 各启用渠道发送 → 记录结果。
///
/// 渠道失败相互隔离；只要有一个渠道成功，调用方即可视为本周期已发送。
pub async fn dispatch(
    db: &HubDb,
    hub_cfg: &HubConfig,
    kind: &str,
    period: &str,
    period_human: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<ChannelOutcome> {
    let cfg = load_config(db);
    let tz_name = effective_timezone_name(db, hub_cfg);
    let meta = ReportMeta {
        title: "Metria 用量报告".to_string(),
        period: period_human.to_string(),
        timezone: tz_name,
        kind: kind.to_string(),
        generated_at: Utc::now().to_rfc3339(),
    };
    let metrics = aggregate(db, from, to, 8);
    let tz = effective_timezone(db, hub_cfg);
    let charts = build_charts(db, from, to, tz, &metrics);
    let html = render_html(&metrics, &meta, &charts);
    let text = render_text(&metrics, &meta);
    let payload = webhook_payload(&metrics, &meta);

    let mut outcomes: Vec<ChannelOutcome> = Vec::new();

    if cfg.email_enabled {
        let recipients = resolve_recipients(db, hub_cfg, &cfg.recipients);
        if recipients.is_empty() {
            outcomes.push(ChannelOutcome {
                channel: "email".into(),
                ok: false,
                detail: Some("无有效收件人（未配置且无法从 OIDC/用户解析）".into()),
            });
        } else {
            let subject = format!("Metria 用量报告 · {period_human}");
            let smtp = cfg.smtp.clone();
            let (html_c, text_c) = (html.clone(), text.clone());
            let charts_c = charts.clone();
            let rc = recipients.clone();
            let res = tokio::task::spawn_blocking(move || {
                send_email(&smtp, &rc, &subject, &html_c, &text_c, &charts_c)
            })
            .await;
            outcomes.push(outcome_from("email", res));
        }
    } else {
        tracing::debug!("邮件渠道未启用，跳过");
    }

    if cfg.webhook_enabled {
        let wh = cfg.webhook.clone();
        let payload_c = payload.clone();
        let res = tokio::task::spawn_blocking(move || send_webhook(&wh, &payload_c)).await;
        outcomes.push(outcome_from("webhook", res));
    }

    for o in &outcomes {
        let status = if o.ok { "success" } else { "failed" };
        let _ = db.record_report_send(kind, period, &o.channel, status, o.detail.as_deref());
    }
    if outcomes.is_empty() {
        tracing::warn!(kind, period, "无任何启用渠道，报告未发送");
    }
    outcomes
}

fn hex_to_rgb(hex: &str) -> RGBColor {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(h.get(0..2).unwrap_or("63"), 16).unwrap_or(99);
    let g = u8::from_str_radix(h.get(2..4).unwrap_or("66"), 16).unwrap_or(102);
    let b = u8::from_str_radix(h.get(4..6).unwrap_or("f1"), 16).unwrap_or(241);
    RGBColor(r, g, b)
}

fn has_values(cs: &ChartSeries) -> bool {
    cs.series.iter().any(|s| s.values.iter().any(|v| *v > 0))
}

const TOKEN_COLORS: [&str; 3] = ["#6366f1", "#10b981", "#f59e0b"];
const DIM_COLORS: [&str; 8] = [
    "#6366f1", "#10b981", "#f59e0b", "#ef4444", "#06b6d4", "#8b5cf6", "#ec4899", "#84cc16",
];

fn build_charts(
    db: &HubDb,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: chrono_tz::Tz,
    m: &ReportMetrics,
) -> Vec<ReportChart> {
    let mut out = Vec::new();
    if !m.has_data {
        return out;
    }
    // 单日报告按小时分桶，周/月按天分桶
    let by_hour = (to - from).num_hours() <= 36;
    let mk = |cs: &ChartSeries,
              colors: &[&str],
              cid: &str,
              title: &str,
              fill: bool|
     -> Option<ReportChart> {
        if !has_values(cs) {
            return None;
        }
        let data: Vec<(RGBColor, Vec<i64>)> = cs
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| (hex_to_rgb(colors[i % colors.len()]), s.values.clone()))
            .collect();
        let png = charts::line_chart_png(&cs.days, &data, 1200, 300, fill).ok()?;
        let series = cs
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), colors[i % colors.len()].to_string()))
            .collect();
        Some(ReportChart {
            cid: cid.to_string(),
            title: title.to_string(),
            png,
            series,
            days: cs.days.clone(),
        })
    };

    let token = aggregate::daily_token_chart(db, from, to, tz, by_hour);
    if let Some(c) = mk(&token, &TOKEN_COLORS, "chart-token", "Token 趋势", true) {
        out.push(c);
    }
    let calls = aggregate::daily_calls_chart(db, from, to, tz, by_hour);
    if let Some(c) = mk(&calls, &["#6366f1"], "chart-calls", "请求数量趋势", true) {
        out.push(c);
    }
    let models = aggregate::dim_daily_chart(db, from, to, tz, "model", 0, true, by_hour);
    if let Some(c) = mk(
        &models,
        &DIM_COLORS,
        "chart-models",
        "模型趋势（按 Token）",
        true,
    ) {
        out.push(c);
    }
    let clients = aggregate::dim_daily_chart(db, from, to, tz, "client_id", 0, true, by_hour);
    if let Some(c) = mk(
        &clients,
        &DIM_COLORS,
        "chart-clients",
        "Agent 趋势（按 Token）",
        true,
    ) {
        out.push(c);
    }
    out
}

fn outcome_from(
    channel: &str,
    res: Result<Result<(), String>, tokio::task::JoinError>,
) -> ChannelOutcome {
    match res {
        Ok(Ok(())) => ChannelOutcome {
            channel: channel.into(),
            ok: true,
            detail: None,
        },
        Ok(Err(e)) => ChannelOutcome {
            channel: channel.into(),
            ok: false,
            detail: Some(e),
        },
        Err(e) => ChannelOutcome {
            channel: channel.into(),
            ok: false,
            detail: Some(format!("任务执行失败: {e}")),
        },
    }
}

/// 立即发送测试报告（按当前配置的启用渠道）。
pub async fn send_test(db: &HubDb, hub_cfg: &HubConfig) -> Vec<ChannelOutcome> {
    let cfg = load_config(db);
    let tz = effective_timezone(db, hub_cfg);
    let now = Utc::now().with_timezone(&tz);
    let (period, from, to) =
        period_range(Kind::Daily, &cfg.schedules.daily, now).unwrap_or_else(|| {
            (
                "test".to_string(),
                Utc::now() - chrono::Duration::days(1),
                Utc::now(),
            )
        });
    let human = format!("{period}（测试）");
    dispatch(
        db,
        hub_cfg,
        "test",
        &format!("test-{}", Utc::now().timestamp()),
        &human,
        from,
        to,
    )
    .await
}

/// 启动报告调度后台任务（每 60 秒检查三个独立调度）。
pub fn spawn_report_scheduler(db: HubDb, hub_cfg: HubConfig) {
    tokio::spawn(async move {
        // 首次延迟，避免与启动初始化争用
        tokio::time::sleep(Duration::from_secs(20)).await;
        let mut tick = interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            let cfg = load_config(&db);
            let tz = effective_timezone(&db, &hub_cfg);
            let now = Utc::now().with_timezone(&tz);
            let schedules = [
                (Kind::Daily, cfg.schedules.daily.clone()),
                (Kind::Weekly, cfg.schedules.weekly.clone()),
                (Kind::Monthly, cfg.schedules.monthly.clone()),
            ];
            for (kind, sched) in schedules {
                if !sched.enabled {
                    continue;
                }
                let Some((period, due)) = period_and_due(kind, &sched, now) else {
                    continue;
                };
                if now < due {
                    continue;
                }
                if db
                    .report_period_sent(kind.as_str(), &period)
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some((_, from, to)) = period_range(kind, &sched, now) else {
                    continue;
                };
                let human = period_label(kind, &period);
                tracing::info!(kind = kind.as_str(), period = %period, "开始发送周期报告");
                let outcomes =
                    dispatch(&db, &hub_cfg, kind.as_str(), &period, &human, from, to).await;
                for o in &outcomes {
                    if o.ok {
                        tracing::info!(channel = %o.channel, kind = kind.as_str(), period = %period, "报告发送成功");
                    } else {
                        tracing::warn!(channel = %o.channel, kind = kind.as_str(), period = %period, detail = ?o.detail, "报告发送失败");
                    }
                }
            }
        }
    });
}
