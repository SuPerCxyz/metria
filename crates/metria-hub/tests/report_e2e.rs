//! 报告端到端：进程内最小 SMTP 与 HTTP 接收端，验证投递链路与渠道隔离。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;
use metria_hub::report;

/// 最小 SMTP 接收端：接受一次会话，记录是否收到 DATA，并捕获 DATA 载荷原文。
fn spawn_smtp_sink() -> (u16, Arc<AtomicBool>, Arc<Mutex<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let got = Arc::new(AtomicBool::new(false));
    let flag = got.clone();
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let cap = captured.clone();
    thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            let _ = writer.write_all(b"220 metria-test ESMTP\r\n");
            let mut in_data = false;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let cmd = line.trim_end().to_string();
                if in_data {
                    if cmd == "." {
                        in_data = false;
                        let _ = writer.write_all(b"250 OK\r\n");
                    } else {
                        flag.store(true, Ordering::SeqCst);
                        cap.lock().unwrap().extend_from_slice(line.as_bytes());
                    }
                    continue;
                }
                let upper = cmd.to_uppercase();
                if upper.starts_with("EHLO") {
                    let _ = writer.write_all(b"250-metria-test\r\n250-8BITMIME\r\n250 OK\r\n");
                } else if upper.starts_with("DATA") {
                    in_data = true;
                    let _ = writer.write_all(b"354 End data\r\n");
                } else if upper.starts_with("QUIT") {
                    let _ = writer.write_all(b"221 Bye\r\n");
                    break;
                } else {
                    let _ = writer.write_all(b"250 OK\r\n");
                }
            }
        }
    });
    (port, got, captured)
}

/// 最小 HTTP 接收端：收到带 body 的 POST 即置位。
fn spawn_http_sink() -> (u16, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let got = Arc::new(AtomicBool::new(false));
    let flag = got.clone();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut content_len = 0usize;
            let mut first = true;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line == "\r\n" || line == "\n" {
                    break;
                }
                if first {
                    first = false;
                }
                let lower = line.to_lowercase();
                if let Some(rest) = lower.strip_prefix("content-length:") {
                    content_len = rest.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_len];
            if content_len > 0 && reader.read_exact(&mut body).is_ok() {
                flag.store(true, Ordering::SeqCst);
            }
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK");
        }
    });
    (port, got)
}

fn temp_db() -> (HubDb, HubConfig, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = HubConfig {
        data_dir: dir.path().to_path_buf(),
        database_url: format!("sqlite://{}/test.db", dir.path().display()),
        ..Default::default()
    };
    let db = HubDb::open(&cfg).unwrap();
    db.apply_migrations().unwrap();
    (db, cfg, dir)
}

fn config_email(port: u16) -> report::ReportConfig {
    report::ReportConfig {
        email_enabled: true,
        recipients: "user@test.local".into(),
        smtp: report::SmtpConfig {
            host: "127.0.0.1".into(),
            port,
            username: String::new(),
            password: None,
            from: "metria@test.local".into(),
            tls: "none".into(),
        },
        ..Default::default()
    }
}

fn config_webhook(port: u16) -> report::ReportConfig {
    report::ReportConfig {
        webhook_enabled: true,
        webhook: report::WebhookConfig {
            url: format!("http://127.0.0.1:{port}/hook"),
            headers: vec![report::HeaderKv {
                name: "X-Test".into(),
                value: "1".into(),
            }],
            secret: Some("s3cret".into()),
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn email_and_webhook_delivered() {
    let (db, cfg, _dir) = temp_db();
    let (smtp_port, email_got, captured) = spawn_smtp_sink();
    let (http_port, hook_got) = spawn_http_sink();
    let mut rc = config_email(smtp_port);
    rc.webhook_enabled = true;
    rc.webhook = config_webhook(http_port).webhook;
    report::save_config(&db, &rc).unwrap();

    let outcomes = report::scheduler::dispatch(
        &db,
        &cfg,
        "test",
        "test-1",
        "测试（上一自然日）",
        chrono::Utc::now() - chrono::Duration::days(1),
        chrono::Utc::now(),
    )
    .await;

    assert!(
        outcomes.iter().any(|o| o.channel == "email" && o.ok),
        "{outcomes:?}"
    );
    assert!(
        outcomes.iter().any(|o| o.channel == "webhook" && o.ok),
        "{outcomes:?}"
    );
    thread::sleep(Duration::from_millis(200));
    assert!(email_got.load(Ordering::SeqCst), "SMTP 未收到 DATA");
    assert!(hook_got.load(Ordering::SeqCst), "Webhook 未收到 body");
    let raw = {
        let guard = captured.lock().unwrap();
        String::from_utf8_lossy(&guard[..]).to_string()
    };
    assert!(
        raw.contains("application/pdf"),
        "邮件应包含 PDF 附件: {raw}"
    );
    assert!(
        raw.to_lowercase()
            .contains("content-disposition: attachment"),
        "应为附件而非内联: {raw}"
    );
    assert!(raw.contains(".pdf"), "附件应有 .pdf 文件名: {raw}");
}

#[tokio::test]
async fn attachments_disabled_omits_pdf() {
    let (db, cfg, _dir) = temp_db();
    let (smtp_port, email_got, captured) = spawn_smtp_sink();
    let mut rc = config_email(smtp_port);
    rc.attachments_enabled = false;
    report::save_config(&db, &rc).unwrap();

    let outcomes = report::scheduler::dispatch(
        &db,
        &cfg,
        "test",
        "test-noatt",
        "测试（上一自然日）",
        chrono::Utc::now() - chrono::Duration::days(1),
        chrono::Utc::now(),
    )
    .await;

    assert!(outcomes.iter().any(|o| o.channel == "email" && o.ok));
    thread::sleep(Duration::from_millis(200));
    assert!(email_got.load(Ordering::SeqCst));
    let raw = {
        let guard = captured.lock().unwrap();
        String::from_utf8_lossy(&guard[..]).to_string()
    };
    assert!(
        !raw.contains("application/pdf"),
        "关闭附件后不应有 PDF: {raw}"
    );
}

#[tokio::test]
async fn email_failure_isolated_from_webhook() {
    let (db, cfg, _dir) = temp_db();
    // 取一个已释放端口，连接应失败
    let closed_port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let (http_port, hook_got) = spawn_http_sink();
    let mut rc = config_email(closed_port);
    rc.smtp.tls = "none".into();
    rc.webhook_enabled = true;
    rc.webhook = config_webhook(http_port).webhook;
    report::save_config(&db, &rc).unwrap();

    let outcomes = report::scheduler::dispatch(
        &db,
        &cfg,
        "test",
        "test-2",
        "测试（上一自然日）",
        chrono::Utc::now() - chrono::Duration::days(1),
        chrono::Utc::now(),
    )
    .await;

    assert!(
        outcomes.iter().any(|o| o.channel == "email" && !o.ok),
        "邮件渠道应失败: {outcomes:?}"
    );
    assert!(
        outcomes.iter().any(|o| o.channel == "webhook" && o.ok),
        "Webhook 应仍成功: {outcomes:?}"
    );
    thread::sleep(Duration::from_millis(200));
    assert!(hook_got.load(Ordering::SeqCst));
}

#[tokio::test]
async fn success_records_history_and_marks_period_sent() {
    let (db, cfg, _dir) = temp_db();
    let (smtp_port, _got, _cap) = spawn_smtp_sink();
    report::save_config(&db, &config_email(smtp_port)).unwrap();
    let now = chrono::Utc::now();
    let outcomes = report::scheduler::dispatch(
        &db,
        &cfg,
        "daily",
        "2026-09-09",
        "2026-09-09（上一自然日）",
        now - chrono::Duration::days(1),
        now,
    )
    .await;
    assert!(outcomes.iter().any(|o| o.ok));
    assert!(db.report_period_sent("daily", "2026-09-09").unwrap());
    let hist = db.recent_report_sends(10).unwrap();
    assert!(hist
        .iter()
        .any(|h| h.period == "2026-09-09" && h.status == "success"));
}
