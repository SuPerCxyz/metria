//! 报告设置与发送 API（Admin 鉴权）。

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::report::{self, ReportConfig};

use super::{auth_user, json_err, AppState};

/// GET /api/v1/settings/report
pub(crate) async fn report_settings_get(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if auth_user(&st, &headers).is_none() {
        return json_err(StatusCode::UNAUTHORIZED, "unauthorized", "未登录");
    }
    let mut cfg = report::load_config(&st.db);
    let smtp_password_set = cfg
        .smtp
        .password
        .as_ref()
        .map(|p| !p.is_empty())
        .unwrap_or(false);
    cfg.smtp.password = None;
    let timezone = report::effective_timezone_name(&st.db, &st.cfg);
    let resolved_recipients = report::resolve_recipients(&st.db, &st.cfg, &cfg.recipients);
    Json(json!({
        "config": cfg,
        "smtp_password_set": smtp_password_set,
        "timezone": timezone,
        "timezone_default": st.cfg.timezone.name(),
        "resolved_recipients": resolved_recipients,
    }))
    .into_response()
}

/// PUT /api/v1/settings/report
pub(crate) async fn report_settings_put(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if auth_user(&st, &headers).is_none() {
        return json_err(StatusCode::UNAUTHORIZED, "unauthorized", "未登录");
    }
    let mut incoming: ReportConfig = match serde_json::from_value(body.clone()) {
        Ok(v) => v,
        Err(e) => return json_err(StatusCode::BAD_REQUEST, "invalid_config", &e.to_string()),
    };
    if let Err(msg) = validate(&incoming) {
        return json_err(StatusCode::BAD_REQUEST, "invalid_config", &msg);
    }
    if let Some(tz) = body.get("timezone").and_then(|v| v.as_str()) {
        let tz = tz.trim();
        if !tz.is_empty() {
            if let Err(e) = report::set_timezone(&st.db, tz) {
                return json_err(StatusCode::BAD_REQUEST, "invalid_config", &e);
            }
        }
    }
    // 密码：未提供（null/空）时保留旧值，避免页面回填掩码造成清空
    let existing = report::load_config(&st.db);
    let new_pass = incoming
        .smtp
        .password
        .as_ref()
        .filter(|p| !p.is_empty())
        .cloned();
    incoming.smtp.password = new_pass.or(existing.smtp.password);
    if let Err(e) = report::save_config(&st.db, &incoming) {
        return json_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error", &e);
    }
    Json(json!({ "ok": true })).into_response()
}

/// POST /api/v1/settings/report/test
pub(crate) async fn report_settings_test(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if auth_user(&st, &headers).is_none() {
        return json_err(StatusCode::UNAUTHORIZED, "unauthorized", "未登录");
    }
    let outcomes = report::scheduler::send_test(&st.db, &st.cfg).await;
    let any_ok = outcomes.iter().any(|o| o.ok);
    Json(json!({ "ok": any_ok, "results": outcomes })).into_response()
}

/// GET /api/v1/settings/report/history
pub(crate) async fn report_history(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if auth_user(&st, &headers).is_none() {
        return json_err(StatusCode::UNAUTHORIZED, "unauthorized", "未登录");
    }
    match st.db.recent_report_sends(50) {
        Ok(rows) => Json(json!({ "sends": rows })).into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "db_error",
            &e.to_string(),
        ),
    }
}

fn validate(c: &ReportConfig) -> Result<(), String> {
    for (name, s) in [
        ("每日", &c.schedules.daily),
        ("每周", &c.schedules.weekly),
        ("每月", &c.schedules.monthly),
    ] {
        if report::parse_hhmm(&s.time).is_none() {
            return Err(format!("{name}发送时间格式应为 HH:MM"));
        }
    }
    if c.schedules.weekly.weekday.unwrap_or(0) > 6 {
        return Err("每周调度周几应为 0（周一）到 6（周日）".into());
    }
    let day = c.schedules.monthly.day.unwrap_or(1);
    if !(1..=28).contains(&day) {
        return Err("每月调度日期应为 1 到 28".into());
    }
    match c.smtp.tls.as_str() {
        "none" | "starttls" | "tls" => {}
        _ => return Err("TLS 模式应为 none/starttls/tls".into()),
    }
    if c.smtp.port == 0 {
        return Err("SMTP 端口无效".into());
    }
    if c.email_enabled {
        if c.smtp.host.trim().is_empty() {
            return Err("启用邮件渠道需配置 SMTP 服务器地址".into());
        }
        if c.smtp.from.trim().is_empty() && c.smtp.username.trim().is_empty() {
            return Err("启用邮件渠道需配置发件人或用户名".into());
        }
        for r in c.recipients.split(',') {
            let r = r.trim();
            if !r.is_empty() && !report::looks_like_email(r) {
                return Err(format!("收件人不是合法邮箱: {r}"));
            }
        }
    }
    if c.webhook_enabled {
        let u = c.webhook.url.trim();
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return Err("启用 Webhook 需配置 http(s) URL".into());
        }
    }
    Ok(())
}
