//! 报告渠道：SMTP 邮件与通用 JSON Webhook。

use std::time::Duration;

use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{Message, SmtpTransport, Transport};

use super::{SmtpConfig, WebhookConfig};

/// 发送邮件（HTML + 纯文本，图表以内嵌 SVG 放在 HTML 正文中，不附带 PDF）。
/// 同步实现，调用方应在 `spawn_blocking` 中运行。
pub fn send_email(
    smtp: &SmtpConfig,
    recipients: &[String],
    subject: &str,
    html: &str,
    text: &str,
) -> Result<(), String> {
    if smtp.host.trim().is_empty() {
        return Err("未配置 SMTP 服务器地址".into());
    }
    if recipients.is_empty() {
        return Err("无有效收件人".into());
    }
    let from_raw = if smtp.from.trim().is_empty() {
        smtp.username.trim()
    } else {
        smtp.from.trim()
    };
    if from_raw.is_empty() {
        return Err("未配置发件人（from）".into());
    }
    let from: Mailbox = from_raw
        .parse()
        .map_err(|_| format!("发件人地址无效: {from_raw}"))?;

    let mut builder = Message::builder().from(from).subject(subject);
    for r in recipients {
        let mb: Mailbox = r.parse().map_err(|_| format!("收件人地址无效: {r}"))?;
        builder = builder.to(mb);
    }
    let alternative = MultiPart::alternative_plain_html(text.to_string(), html.to_string());
    let email = builder
        .multipart(alternative)
        .map_err(|e| format!("构造邮件失败: {e}"))?;

    let mut tbuilder = SmtpTransport::builder_dangerous(smtp.host.trim()).port(smtp.port);
    match smtp.tls.as_str() {
        "none" => {}
        "tls" => {
            let params = TlsParameters::new(smtp.host.trim().to_string())
                .map_err(|e| format!("TLS 参数构建失败: {e}"))?;
            tbuilder = tbuilder.tls(Tls::Wrapper(params));
        }
        // 默认 starttls
        _ => {
            let params = TlsParameters::new(smtp.host.trim().to_string())
                .map_err(|e| format!("TLS 参数构建失败: {e}"))?;
            tbuilder = tbuilder.tls(Tls::Required(params));
        }
    }
    if !smtp.username.trim().is_empty() {
        let pass = smtp.password.clone().unwrap_or_default();
        tbuilder = tbuilder.credentials(Credentials::new(smtp.username.clone(), pass));
    }
    let transport = tbuilder.timeout(Some(Duration::from_secs(20))).build();

    transport
        .send(&email)
        .map(|_| ())
        .map_err(|e| format!("SMTP 投递失败: {e}"))
}

/// 发送通用 JSON Webhook。同步实现，调用方应在 `spawn_blocking` 中运行。
pub fn send_webhook(wh: &WebhookConfig, payload: &serde_json::Value) -> Result<(), String> {
    if wh.url.trim().is_empty() {
        return Err("未配置 Webhook URL".into());
    }
    let mut req = ureq::post(wh.url.trim()).timeout(Duration::from_secs(15));
    for h in &wh.headers {
        if h.name.trim().is_empty() {
            continue;
        }
        req = req.set(h.name.trim(), h.value.as_str());
    }
    if let Some(secret) = wh.secret.as_ref().filter(|s| !s.trim().is_empty()) {
        req = req.set("X-Metria-Secret", secret.as_str());
    }
    req.send_json(payload)
        .map(|_| ())
        .map_err(|e| format!("Webhook 投递失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_requires_host_and_recipient() {
        let smtp = SmtpConfig::default();
        assert!(send_email(&smtp, &["a@b.com".into()], "s", "<p></p>", " ").is_err());
    }

    #[test]
    fn webhook_requires_url() {
        let wh = WebhookConfig::default();
        assert!(send_webhook(&wh, &serde_json::json!({})).is_err());
    }
}
