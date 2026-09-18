//! 原生运行时观测器的最小阻塞 HTTP 转发层。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::Utc;

use crate::observer_metrics::{ObservedCall, Tracker, MAX_LINE_BYTES};

const MAX_REQUEST_BODY: usize = 8 * 1024 * 1024;

pub(crate) fn proxy_loop(
    listener: TcpListener,
    upstream: String,
    client_id: String,
    stop: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<ObservedCall>>>,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let upstream = upstream.clone();
                let client_id = client_id.clone();
                let calls = calls.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_proxy_connection(stream, &upstream, &client_id, &calls) {
                        tracing::debug!(%e, "观测请求处理失败");
                    }
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                tracing::debug!(%e, "观测端口 accept 失败");
                break;
            }
        }
    }
}

fn handle_proxy_connection(
    mut stream: TcpStream,
    upstream: &str,
    client_id: &str,
    calls: &Arc<Mutex<Vec<ObservedCall>>>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    let started_at = Utc::now();
    let mut reader = BufReader::new(stream.try_clone()?);
    let (method, path, headers, body, request_header_bytes) = read_request(&mut reader)?;
    let model = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        });
    let endpoint = format!("{upstream}{path}");
    let mut tracker = Tracker::new(started_at, endpoint.clone(), model);
    tracker.request_payload_bytes = Some(body.len() as i64);
    tracker.request_wire_bytes = Some((request_header_bytes + body.len()) as i64);

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout(Duration::from_secs(300))
        .build();
    let mut request = agent.request(&method, &endpoint);
    for (key, value) in headers {
        if matches!(
            key.as_str(),
            "host" | "content-length" | "connection" | "transfer-encoding"
        ) {
            continue;
        }
        request = request.set(&key, &value);
    }
    let response = match request.send_bytes(&body) {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(e) => {
            tracker.call.error_kind = Some("upstream_request_failed".into());
            tracker.call.completed_at = Some(Utc::now());
            push_call(calls, tracker.finish());
            write_json_response(&mut stream, 502, br#"{"error":"upstream_request_failed"}"#)?;
            return Err(std::io::Error::other(e.to_string()));
        }
    };
    tracker.call.status_code = Some(response.status() as i64);
    tracker.call.rate_limited = Some(response.status() == 429);
    tracker.call.streaming = response
        .header("Content-Type")
        .map(|v| v.contains("text/event-stream"))
        .unwrap_or(false);
    if response.status() >= 400 {
        tracker.call.error_kind = Some(format!("http_{}", response.status()));
    }

    let mut response_header = format!(
        "HTTP/1.1 {} {}\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n",
        response.status(),
        reason(response.status())
    );
    for name in ["Content-Type", "Cache-Control", "X-Request-ID"] {
        if let Some(value) = response.header(name) {
            response_header.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    response_header.push_str("\r\n");
    let response_header_bytes = response_header.len();
    stream.write_all(response_header.as_bytes())?;
    let mut response_reader = response.into_reader();
    let mut buffer = [0u8; 16 * 1024];
    let mut response_bytes = 0usize;
    loop {
        let n = response_reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        if tracker.call.first_byte_at.is_none() {
            tracker.call.first_byte_at = Some(Utc::now());
        }
        response_bytes += n;
        tracker.feed(&buffer[..n]);
        write_chunk(&mut stream, &buffer[..n])?;
    }
    stream.write_all(b"0\r\n\r\n")?;
    tracker.response_payload_bytes = Some(response_bytes as i64);
    tracker.response_wire_bytes = Some((response_header_bytes + response_bytes) as i64);
    tracker.call.completed_at = Some(Utc::now());
    tracker.call.stream_completed = true;
    let _ = client_id;
    push_call(calls, tracker.finish());
    Ok(())
}

type HttpRequest = (String, String, Vec<(String, String)>, Vec<u8>, usize);

fn read_request(reader: &mut BufReader<TcpStream>) -> std::io::Result<HttpRequest> {
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("POST").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    let mut header_bytes = first.len();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        header_bytes += n;
        if line.len() > MAX_LINE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "header too large",
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if key == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((key, value));
        }
    }
    if content_length > MAX_REQUEST_BODY {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request too large",
        ));
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    Ok((method, path, headers, body, header_bytes))
}

fn write_chunk(stream: &mut TcpStream, body: &[u8]) -> std::io::Result<()> {
    write!(stream, "{:X}\r\n", body.len())?;
    stream.write_all(body)?;
    stream.write_all(b"\r\n")
}

fn write_json_response(stream: &mut TcpStream, status: u16, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        502 => "Bad Gateway",
        _ => "Upstream",
    }
}

fn push_call(calls: &Arc<Mutex<Vec<ObservedCall>>>, call: ObservedCall) {
    if let Ok(mut calls) = calls.lock() {
        calls.push(call);
    }
}
