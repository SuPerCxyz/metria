//! 用量报告：配置持久化、周期计算、聚合、渲染、渠道投递与调度。
//!
//! 报告只包含统计指标，不含会话正文/提示词/代码；估算项一律标注「估算」。

pub mod aggregate;
pub mod channels;
pub mod charts;
pub mod pdf;
pub mod render;
pub mod scheduler;

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::config::HubConfig;
use crate::db::HubDb;

/// settings 中报告配置的键。
const KEY_REPORT_CONFIG: &str = "report_config";
/// settings 中全局时区的键（数据库值覆盖环境变量）。
const KEY_TIMEZONE: &str = "timezone";

fn default_smtp_port() -> u16 {
    587
}

fn default_tls() -> String {
    "starttls".to_string()
}

fn default_true() -> bool {
    true
}

fn default_time() -> String {
    "12:00".to_string()
}

/// SMTP 渠道配置。`password` 只在写入时提供，读取时不回传。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default)]
    pub from: String,
    /// none | starttls | tls
    #[serde(default = "default_tls")]
    pub tls: String,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: default_smtp_port(),
            username: String::new(),
            password: None,
            from: String::new(),
            tls: default_tls(),
        }
    }
}

/// Webhook 自定义 Header。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderKv {
    pub name: String,
    pub value: String,
}

/// 通用 JSON Webhook 配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebhookConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: Vec<HeaderKv>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

/// 单个调度配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_time")]
    pub time: String,
    /// 每周调度：0=周一 … 6=周日。
    #[serde(default)]
    pub weekday: Option<u8>,
    /// 每月调度：1–28。
    #[serde(default)]
    pub day: Option<u8>,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            enabled: false,
            time: default_time(),
            weekday: None,
            day: None,
        }
    }
}

/// 三个独立调度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedules {
    #[serde(default)]
    pub daily: Schedule,
    #[serde(default)]
    pub weekly: Schedule,
    #[serde(default)]
    pub monthly: Schedule,
}

impl Default for Schedules {
    fn default() -> Self {
        Self {
            daily: Schedule::default(),
            weekly: Schedule {
                weekday: Some(0),
                ..Schedule::default()
            },
            monthly: Schedule {
                day: Some(1),
                ..Schedule::default()
            },
        }
    }
}

/// 报告配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportConfig {
    #[serde(default)]
    pub email_enabled: bool,
    #[serde(default)]
    pub webhook_enabled: bool,
    #[serde(default)]
    pub recipients: String,
    #[serde(default)]
    pub smtp: SmtpConfig,
    #[serde(default)]
    pub webhook: WebhookConfig,
    #[serde(default)]
    pub schedules: Schedules,
    #[serde(default = "default_true")]
    pub attachments_enabled: bool,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            email_enabled: false,
            webhook_enabled: false,
            recipients: String::new(),
            smtp: SmtpConfig::default(),
            webhook: WebhookConfig::default(),
            schedules: Schedules::default(),
            attachments_enabled: true,
        }
    }
}

/// 调度类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Daily,
    Weekly,
    Monthly,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Daily => "daily",
            Kind::Weekly => "weekly",
            Kind::Monthly => "monthly",
        }
    }
}

/// 单渠道投递结果。
#[derive(Debug, Clone, Serialize)]
pub struct ChannelOutcome {
    pub channel: String,
    pub ok: bool,
    pub detail: Option<String>,
}

/// 读取报告配置（缺省时返回默认值；解析失败时返回默认值并记录 warn）。
pub fn load_config(db: &HubDb) -> ReportConfig {
    match db.setting_get(KEY_REPORT_CONFIG) {
        Ok(Some(v)) => serde_json::from_str(&v).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "报告配置解析失败，使用默认值");
            ReportConfig::default()
        }),
        _ => ReportConfig::default(),
    }
}

/// 保存报告配置。
pub fn save_config(db: &HubDb, cfg: &ReportConfig) -> Result<(), String> {
    let json = serde_json::to_string(cfg).map_err(|e| e.to_string())?;
    db.setting_set(KEY_REPORT_CONFIG, &json)
        .map_err(|e| e.to_string())
}

/// 有效全局时区：数据库设置优先，回退环境变量默认。
pub fn effective_timezone(db: &HubDb, cfg: &HubConfig) -> Tz {
    match db.setting_get(KEY_TIMEZONE) {
        Ok(Some(v)) => metria_core::config::parse_timezone(&v).unwrap_or_else(|_| {
            tracing::warn!(value = %v, "数据库时区无效，回退环境默认");
            cfg.timezone
        }),
        _ => cfg.timezone,
    }
}

/// 校验并写入全局时区。
pub fn set_timezone(db: &HubDb, tz: &str) -> Result<(), String> {
    metria_core::config::parse_timezone(tz).map_err(|_| format!("无效的 IANA 时区: {tz}"))?;
    db.setting_set(KEY_TIMEZONE, tz).map_err(|e| e.to_string())
}

/// 当前有效时区的名称。
pub fn effective_timezone_name(db: &HubDb, cfg: &HubConfig) -> String {
    effective_timezone(db, cfg).name().to_string()
}

/// 解析收件人：显式优先 → OIDC 白名单邮箱 → 已存在用户名（邮箱形态）。
pub fn resolve_recipients(db: &HubDb, cfg: &HubConfig, explicit: &str) -> Vec<String> {
    let list: Vec<String> = explicit
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if !list.is_empty() {
        return list;
    }
    if let Some(o) = &cfg.oidc {
        if let Some(email) = &o.allowed_email {
            if looks_like_email(email) {
                return vec![email.clone()];
            }
        }
    }
    if let Ok(users) = db.usernames() {
        if let Some(u) = users.iter().find(|u| looks_like_email(u)) {
            return vec![u.clone()];
        }
    }
    Vec::new()
}

/// 宽松邮箱形态判断：含单个 `@` 且两侧非空。
pub fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    match s.split_once('@') {
        Some((a, b)) => !a.is_empty() && !b.is_empty() && !b.contains('@'),
        None => false,
    }
}

/// 解析 HH:MM。
pub fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    NaiveTime::from_hms_opt(h, m, 0)
}

/// 组合本地日期时间；夏令时歧义取较早值，不存在时返回 None。
fn local_at(tz: Tz, date: NaiveDate, time: NaiveTime) -> Option<DateTime<Tz>> {
    match tz.from_local_datetime(&date.and_time(time)) {
        chrono::LocalResult::Single(dt) => Some(dt),
        chrono::LocalResult::Ambiguous(a, _) => Some(a),
        chrono::LocalResult::None => None,
    }
}

/// 计算某调度的「上一完整周期标识」与「本次应发时刻（本地）」。
///
/// - daily：周期=昨天；应发=今天 HH:MM。
/// - weekly：周期=上一 ISO 周；应发=本周配置周几 HH:MM。
/// - monthly：周期=上一自然月；应发=本月配置日期 HH:MM。
pub fn period_and_due(
    kind: Kind,
    sched: &Schedule,
    now_local: DateTime<Tz>,
) -> Option<(String, DateTime<Tz>)> {
    let tz = now_local.timezone();
    let time = parse_hhmm(&sched.time)?;
    let today = now_local.date_naive();
    match kind {
        Kind::Daily => {
            let due = local_at(tz, today, time)?;
            let period = (today - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            Some((period, due))
        }
        Kind::Weekly => {
            let offset = sched.weekday.unwrap_or(0).min(6) as i64;
            let monday =
                today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
            let due_date = monday + chrono::Duration::days(offset);
            let due = local_at(tz, due_date, time)?;
            let prev_week_day = monday - chrono::Duration::days(1);
            let iso = prev_week_day.iso_week();
            let period = format!("{}-W{:02}", iso.year(), iso.week());
            Some((period, due))
        }
        Kind::Monthly => {
            let day = sched.day.unwrap_or(1).clamp(1, 28) as u32;
            let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
            let due_date = first + chrono::Duration::days(day as i64 - 1);
            let due = local_at(tz, due_date, time)?;
            let prev = first - chrono::Duration::days(1);
            let period = prev.format("%Y-%m").to_string();
            Some((period, due))
        }
    }
}

/// UTC 时间范围 `[from, to)`：把本地周期边界转换为 UTC。
pub fn period_range(
    kind: Kind,
    sched: &Schedule,
    now_local: DateTime<Tz>,
) -> Option<(String, DateTime<Utc>, DateTime<Utc>)> {
    let tz = now_local.timezone();
    let (period, _due) = period_and_due(kind, sched, now_local)?;
    let today = now_local.date_naive();
    let (start_local, end_local) = match kind {
        Kind::Daily => {
            let d = today - chrono::Duration::days(1);
            (d.and_hms_opt(0, 0, 0)?, today.and_hms_opt(0, 0, 0)?)
        }
        Kind::Weekly => {
            let monday =
                today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
            let prev_monday = monday - chrono::Duration::days(7);
            (
                prev_monday.and_hms_opt(0, 0, 0)?,
                monday.and_hms_opt(0, 0, 0)?,
            )
        }
        Kind::Monthly => {
            let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
            let prev_first = (first - chrono::Duration::days(1)).with_day(1)?;
            (
                prev_first.and_hms_opt(0, 0, 0)?,
                first.and_hms_opt(0, 0, 0)?,
            )
        }
    };
    let from = tz
        .from_local_datetime(&start_local)
        .earliest()?
        .with_timezone(&Utc);
    let to = tz
        .from_local_datetime(&end_local)
        .earliest()?
        .with_timezone(&Utc);
    Some((period, from, to))
}

/// 周期的人类可读标题。
pub fn period_label(kind: Kind, period: &str) -> String {
    match kind {
        Kind::Daily => format!("{period}（上一自然日）"),
        Kind::Weekly => format!("{period}（上一自然周）"),
        Kind::Monthly => format!("{period}（上一自然月）"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(tz: Tz, s: &str) -> DateTime<Tz> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&tz)
    }

    #[test]
    fn email_shape() {
        assert!(looks_like_email("a@b.com"));
        assert!(!looks_like_email("admin"));
        assert!(!looks_like_email("a@@b"));
    }

    #[test]
    fn daily_period_and_due() {
        let tz = chrono_tz::Tz::Asia__Shanghai;
        let now = local(tz, "2026-09-10T13:00:00+08:00");
        let sched = Schedule {
            enabled: true,
            time: "12:00".into(),
            weekday: None,
            day: None,
        };
        let (period, due) = period_and_due(Kind::Daily, &sched, now).unwrap();
        assert_eq!(period, "2026-09-09");
        assert_eq!(due.to_rfc3339(), "2026-09-10T12:00:00+08:00");
    }

    #[test]
    fn weekly_period_is_previous_iso_week() {
        let tz = chrono_tz::Tz::Asia__Shanghai;
        // 2026-09-07 是周一
        let now = local(tz, "2026-09-07T13:00:00+08:00");
        let sched = Schedule {
            enabled: true,
            time: "12:00".into(),
            weekday: Some(0),
            day: None,
        };
        let (period, due) = period_and_due(Kind::Weekly, &sched, now).unwrap();
        assert_eq!(period, "2026-W36");
        assert_eq!(due.to_rfc3339(), "2026-09-07T12:00:00+08:00");
    }

    #[test]
    fn monthly_period_previous_month() {
        let tz = chrono_tz::Tz::Asia__Shanghai;
        let now = local(tz, "2026-09-01T13:00:00+08:00");
        let sched = Schedule {
            enabled: true,
            time: "12:00".into(),
            weekday: None,
            day: Some(1),
        };
        let (period, due) = period_and_due(Kind::Monthly, &sched, now).unwrap();
        assert_eq!(period, "2026-08");
        assert_eq!(due.to_rfc3339(), "2026-09-01T12:00:00+08:00");
    }

    #[test]
    fn period_range_daily_is_utc_aligned() {
        let tz = chrono_tz::Tz::Asia__Shanghai;
        let now = local(tz, "2026-09-10T13:00:00+08:00");
        let sched = Schedule {
            enabled: true,
            time: "12:00".into(),
            weekday: None,
            day: None,
        };
        let (_p, from, to) = period_range(Kind::Daily, &sched, now).unwrap();
        assert_eq!(from.to_rfc3339(), "2026-09-08T16:00:00+00:00");
        assert_eq!(to.to_rfc3339(), "2026-09-09T16:00:00+00:00");
    }
}
