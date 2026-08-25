//! Pull 服务模式：本地极简 HTTP API，供 Hub 主动拉取采集数据。
//!
//! 零新增依赖：std TcpListener + 手写 HTTP/1.1 解析（仅面向 Hub 单一可信客户端，
//! 请求格式固定，解析失败即断连）。端点：
//! - `GET /collect`：返回待传批次（UploadBatch JSON + zstd，`X-Batch-Id` 头）；空批次 204
//! - `POST /ack`：`{"batch_id": "..."}`，幂等确认并删除 spool 事件
//! - `GET /status`：版本 / 来源健康 / spool 统计
//!
//! 全部端点要求 `Authorization: Bearer <token>`，否则 401。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use metria_protocol::UploadBatch;

use crate::config::AgentConfig;
use crate::spool::Spool;

/// 服务状态（/status 响应体）。
#[derive(Debug, serde::Serialize)]
struct StatusBody {
    ok: bool,
    version: String,
    mode: &'static str,
    pending_events: i64,
    spool_bytes: i64,
    sources_total: i64,
    sources_healthy: i64,
}

/// 启动 Pull 服务（阻塞直至 stop 置位或监听失败）。
pub fn serve(cfg: AgentConfig, token: String, stop: Arc<AtomicBool>) -> crate::error::Result<()> {
    let addr = format!("0.0.0.0:{}", cfg.listen_port);
    let listener = TcpListener::bind(&addr)?;
    listener.set_nonblocking(true)?;
    tracing::info!(%addr, "Pull 服务已启动，等待 Hub 拉取");

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((stream, _peer)) => {
                let cfg = cfg.clone();
                let token = token.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let _ = handle_conn(cfg, token, stream, stop);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                tracing::warn!("Pull 服务 accept 失败: {e}");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    tracing::info!("Pull 服务已停止");
    Ok(())
}

fn handle_conn(
    cfg: AgentConfig,
    token: String,
    mut stream: TcpStream,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let path = parts
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();

    let mut content_length = 0usize;
    let mut authorized = false;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            break;
        }
        let t = h.trim();
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            } else if key == "authorization" {
                authorized = constant_time_eq(val, &format!("Bearer {token}"));
            }
        }
    }

    // 上限保护：ack 请求体极小，超限直接断连
    if content_length > 64 * 1024 {
        return write_response(&mut stream, 413, "payload too large", b"");
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    if !authorized {
        return write_response(
            &mut stream,
            401,
            "unauthorized",
            br#"{"error":"unauthorized"}"#,
        );
    }
    if stop.load(Ordering::Relaxed) {
        return write_response(&mut stream, 503, "shutting down", b"");
    }

    match (method.as_str(), path.as_str()) {
        ("GET", "/collect") => handle_collect(&cfg, &mut stream),
        ("POST", "/ack") => handle_ack(&cfg, &body, &mut stream),
        ("GET", "/status") => handle_status(&cfg, &mut stream),
        _ => write_response(&mut stream, 404, "not found", br#"{"error":"not_found"}"#),
    }
}

fn handle_collect(cfg: &AgentConfig, stream: &mut TcpStream) -> std::io::Result<()> {
    let mut spool = match Spool::open(
        &cfg.data_dir.join("spool.db"),
        cfg.max_pending_events,
        cfg.max_spool_bytes,
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Pull /collect 打开 spool 失败: {e}");
            return write_response(stream, 500, "spool error", br#"{"error":"spool_error"}"#);
        }
    };
    let batch = match spool.collect_batch(cfg.batch_max_events, cfg.batch_max_bytes) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Pull /collect 组批失败: {e}");
            return write_response(stream, 500, "spool error", br#"{"error":"spool_error"}"#);
        }
    };
    let Some((batch_id, events)) = batch else {
        return write_response(stream, 204, "no content", b"");
    };
    // pull 模式下 agent 不感知 node_id/collector_id，由 Hub 侧按 token 归属重写
    let batch = UploadBatch {
        schema_version: metria_protocol::limits::SCHEMA_VERSION,
        batch_id,
        node_id: "pull".into(),
        collector_id: "pull".into(),
        agent_version: metria_core::VERSION.to_string(),
        events: events
            .into_iter()
            .map(|e| metria_protocol::BatchEvent {
                kind: e.kind,
                event_id: e.event_id,
                payload: e.payload,
            })
            .collect(),
    };
    let json = match serde_json::to_vec(&batch) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("Pull /collect 序列化失败: {e}");
            return write_response(stream, 500, "serialize error", br#"{"error":"serialize"}"#);
        }
    };
    let compressed = zstd::stream::encode_all(&json[..], 3).unwrap_or_else(|_| json.clone());
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: zstd\r\nX-Batch-Id: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        batch.batch_id,
        compressed.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&compressed)?;
    stream.flush()
}

fn handle_ack(cfg: &AgentConfig, body: &[u8], stream: &mut TcpStream) -> std::io::Result<()> {
    let req: metria_protocol::AckRequest =
        match serde_json::from_slice::<metria_protocol::AckRequest>(body) {
            Ok(r) if !r.batch_id.trim().is_empty() => r,
            _ => return write_response(stream, 400, "bad request", br#"{"error":"invalid_ack"}"#),
        };
    let mut spool = match Spool::open(
        &cfg.data_dir.join("spool.db"),
        cfg.max_pending_events,
        cfg.max_spool_bytes,
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Pull /ack 打开 spool 失败: {e}");
            return write_response(stream, 500, "spool error", br#"{"error":"spool_error"}"#);
        }
    };
    match spool.ack_batch(&req.batch_id) {
        Ok(crate::spool::AckResult::Ok) => {
            tracing::info!(batch = %req.batch_id, "Pull 批次已确认");
            write_json(stream, 200, br#"{"ok":true,"already":false}"#)
        }
        Ok(crate::spool::AckResult::AlreadyAcked) => {
            write_json(stream, 200, br#"{"ok":true,"already":true}"#)
        }
        Ok(crate::spool::AckResult::NotFound) => {
            write_json(stream, 404, br#"{"error":"batch_not_found"}"#)
        }
        Err(e) => {
            tracing::warn!("Pull /ack 处理失败: {e}");
            write_response(stream, 500, "spool error", br#"{"error":"spool_error"}"#)
        }
    }
}

fn handle_status(cfg: &AgentConfig, stream: &mut TcpStream) -> std::io::Result<()> {
    let spool = Spool::open(
        &cfg.data_dir.join("spool.db"),
        cfg.max_pending_events,
        cfg.max_spool_bytes,
    );
    let (pending, bytes, sources_total, sources_healthy) = match &spool {
        Ok(s) => (
            s.pending_count(),
            s.spool_bytes(),
            s.source_stats().0,
            s.source_stats().1,
        ),
        Err(_) => (0, 0, 0, 0),
    };
    let body = StatusBody {
        ok: true,
        version: metria_core::VERSION.to_string(),
        mode: "pull",
        pending_events: pending,
        spool_bytes: bytes,
        sources_total,
        sources_healthy,
    };
    let json = serde_json::to_vec(&body).unwrap_or_else(|_| br#"{"ok":false}"#.to_vec());
    write_json(stream, 200, &json)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn write_json(stream: &mut TcpStream, status: u16, body: &[u8]) -> std::io::Result<()> {
    let reason = if status == 200 { "OK" } else { "Error" };
    write_response(stream, status, reason, body)
}
