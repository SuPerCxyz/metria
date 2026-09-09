//! 无状态轮询采集：游标外置 Hub，事件确认后推进游标，遗留 spool 一次性迁移。
//!
//! 顺序约束（硬性）：某 Source 的全部事件被 Hub 确认（accepted/duplicate）后
//! 才回推该 Source 的新游标；上传失败不推游标，下轮从旧游标重扫，Hub 按
//! event_id 幂等去重。Hub 离线时暂停扫描等待恢复，恢复后按最新游标补齐。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use metria_adapter_api::ScanIdentity;
use metria_core::model::SourceCursor;
use metria_protocol::{BatchEvent, CursorEntry, UploadBatch};

use crate::config::AgentConfig;
use crate::error::{AgentError, Result};
use crate::scanner::{PendingEvent, Scanner};
use crate::spool::Spool;
use crate::wire::HubClient;

/// 清积压的总时长上限（迁移用；超时保留 spool 下次重试）。
const MIGRATION_DRAIN_DEADLINE: Duration = Duration::from_secs(600);

/// 一轮采集统计。
#[derive(Debug, Default)]
pub struct CycleStats {
    pub sources: usize,
    pub events: usize,
    pub errors: usize,
}

/// 可中断睡眠：返回 true 表示收到退出信号。
pub fn sleep_interruptible(stop: &AtomicBool, dur: Duration) -> bool {
    let deadline = Instant::now() + dur;
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    stop.load(Ordering::Relaxed)
}

fn parse_cursor_map(entries: Vec<CursorEntry>) -> HashMap<String, SourceCursor> {
    entries
        .into_iter()
        .filter_map(|e| {
            serde_json::from_str(&e.cursor_json)
                .ok()
                .map(|c| (e.source_id, c))
        })
        .collect()
}

fn build_batch(identity: &ScanIdentity, batch_id: &str, events: &[PendingEvent]) -> UploadBatch {
    UploadBatch {
        schema_version: metria_protocol::limits::SCHEMA_VERSION,
        batch_id: batch_id.to_string(),
        node_id: identity.node_id.clone(),
        collector_id: identity.collector_id.clone(),
        agent_version: metria_core::VERSION.to_string(),
        events: events
            .iter()
            .map(|e| BatchEvent {
                kind: e.kind.clone(),
                event_id: e.event_id.clone(),
                payload: e.payload.clone(),
            })
            .collect(),
    }
}

/// 按事件数 + 未压缩字节预算切块；单个超预算事件独立成块（由 413 拆批兜底）。
fn chunk_events(
    events: &[PendingEvent],
    max_events: usize,
    max_bytes: usize,
) -> Vec<Vec<PendingEvent>> {
    let mut out = Vec::new();
    let mut cur: Vec<PendingEvent> = Vec::new();
    let mut bytes = 0usize;
    for e in events {
        let size = serde_json::to_string(&e.payload)
            .map(|s| s.len())
            .unwrap_or(0)
            + e.kind.len();
        if !cur.is_empty() && (cur.len() >= max_events || bytes + size > max_bytes) {
            out.push(std::mem::take(&mut cur));
            bytes = 0;
        }
        bytes += size;
        cur.push(e.clone());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 上传一批；返回被 Hub 永久拒绝（跳过）的事件数。可重试失败返回 Err。
fn upload_chunk(
    client: &HubClient,
    identity: &ScanIdentity,
    chunk: &[PendingEvent],
) -> Result<usize> {
    let batch_id = format!("batch-{}", metria_core::model::Id::new());
    let batch = build_batch(identity, &batch_id, chunk);
    let resp = client.upload(&batch)?;
    let mut fatal: Vec<(String, String)> = Vec::new();
    for f in &resp.failed {
        if f.retryable {
            return Err(AgentError::Http(format!(
                "Hub 返回可重试失败（event {}），本轮中止",
                f.event_id
            )));
        }
        fatal.push((f.event_id.clone(), f.reason.clone()));
    }
    if !fatal.is_empty() {
        // 永久拒绝等价旧 dead_letter 语义：记录 event_id 与原因后跳过，避免坏事件活锁
        tracing::error!(
            "Hub 永久拒绝 {} 条事件（跳过，游标将推进）样例: {}{}",
            fatal.len(),
            fatal
                .iter()
                .take(5)
                .map(|(id, r)| format!("{id}({r})"))
                .collect::<Vec<_>>()
                .join(", "),
            if fatal.len() > 5 { ", …" } else { "" }
        );
    }
    Ok(fatal.len())
}

/// 上传事件列表（自动切块；413 时二分拆批重试）。返回已处理事件数。
fn upload_events(
    cfg: &AgentConfig,
    client: &HubClient,
    identity: &ScanIdentity,
    events: &[PendingEvent],
) -> Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }
    let chunks = chunk_events(events, cfg.batch_max_events, cfg.batch_max_bytes);
    let mut uploaded = 0usize;
    for chunk in &chunks {
        match upload_chunk(client, identity, chunk) {
            Ok(_) => uploaded += chunk.len(),
            Err(AgentError::BatchTooLarge) if chunk.len() > 1 => {
                let mid = chunk.len() / 2;
                uploaded += upload_events(cfg, client, identity, &chunk[..mid])?;
                uploaded += upload_events(cfg, client, identity, &chunk[mid..])?;
            }
            Err(AgentError::BatchTooLarge) => {
                tracing::error!(
                    "单条事件超限被永久拒绝（跳过，游标将推进）：{}",
                    chunk[0].event_id
                );
                uploaded += 1;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(uploaded)
}

/// 单 Source 流水线：上传 → 全部确认后推进游标（顺序硬约束）。
fn upload_and_advance(
    cfg: &AgentConfig,
    client: &HubClient,
    identity: &ScanIdentity,
    source_id: &str,
    events: &[PendingEvent],
    next_cursor: Option<&SourceCursor>,
) -> Result<usize> {
    let uploaded = upload_events(cfg, client, identity, events)?;
    if let Some(c) = next_cursor {
        let entry = CursorEntry {
            source_id: source_id.to_string(),
            cursor_json: serde_json::to_string(c).map_err(|e| AgentError::Serde(e.to_string()))?,
            updated_at: None,
        };
        client.push_cursors(std::slice::from_ref(&entry))?;
    }
    Ok(uploaded)
}

/// 一轮采集：拉游标 → 逐 Source「扫描→上传→推游标」。
pub fn run_cycle(
    scanner: &Scanner,
    cfg: &AgentConfig,
    client: &HubClient,
    identity: &ScanIdentity,
    cursors: &HashMap<String, SourceCursor>,
) -> Result<CycleStats> {
    let mut totals = CycleStats::default();
    for (client_name, adapter) in scanner.iter_adapters() {
        let Some(sources) = scanner.discover_sources(client_name, adapter) else {
            continue;
        };
        for source in sources {
            let source_id = source.path_hash.as_str().to_string();
            let (events, next_cursor) =
                match scanner.scan_source_events(adapter, &source, cursors.get(&source_id)) {
                    Ok(v) => v,
                    Err(e) => {
                        totals.errors += 1;
                        tracing::warn!("扫描 {} 失败: {e}", source.canonical_path.display());
                        continue;
                    }
                };
            totals.sources += 1;
            if events.is_empty() && next_cursor.is_none() {
                continue;
            }
            match upload_and_advance(
                cfg,
                client,
                identity,
                &source_id,
                &events,
                next_cursor.as_ref(),
            ) {
                Ok(n) => totals.events += n,
                Err(e) => {
                    // 上传失败：该 Source 游标不推进，中止本轮（下轮从旧游标重扫）
                    totals.errors += 1;
                    tracing::warn!("Source {source_id} 本轮未完成（游标不推进，下轮重试）: {e}");
                    return Ok(totals);
                }
            }
        }
    }
    Ok(totals)
}

/// 无状态轮询主循环（阻塞直至退出信号或致命错误）。
pub fn polling_loop(
    cfg: AgentConfig,
    client: HubClient,
    identity: ScanIdentity,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let scanner = Scanner::new(cfg.clone(), identity.clone());
    let poll = Duration::from_secs(cfg.poll_interval_seconds);
    let max_backoff = Duration::from_secs(600);
    let mut backoff = poll;
    tracing::info!(interval_s = cfg.poll_interval_seconds, "无状态轮询采集启动");
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match client.fetch_cursors() {
            Ok(Some(entries)) => {
                let cursors = parse_cursor_map(entries);
                match run_cycle(&scanner, &cfg, &client, &identity, &cursors) {
                    Ok(stats) => {
                        backoff = poll;
                        if stats.sources > 0 || stats.errors > 0 {
                            tracing::debug!(
                                "轮询完成: sources={} events={} errors={}",
                                stats.sources,
                                stats.events,
                                stats.errors
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("本轮采集未完成（游标不推进）: {e}");
                    }
                }
                if sleep_interruptible(&stop, poll) {
                    break;
                }
            }
            Ok(None) => {
                tracing::error!("Hub 不支持游标同步（404）：无状态模式需先升级 Hub");
                return Err(AgentError::Internal(
                    "Hub 不支持游标同步，请先升级 Hub".into(),
                ));
            }
            Err(e) => {
                // Hub 离线：暂停扫描，游标保持不动；恢复后从最新游标补齐
                tracing::warn!("Hub 不可达，暂停扫描等待恢复: {e}");
                backoff = (backoff * 2).min(max_backoff);
                if sleep_interruptible(&stop, backoff) {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// 遗留 spool 一次性迁移：先清积压 → 再推游标 → 后删文件。
/// 任一步失败保留现场并返回 Err（下次启动重试）。
pub fn migrate_legacy_spool(
    cfg: &AgentConfig,
    client: &HubClient,
    identity: &ScanIdentity,
) -> Result<()> {
    let path = cfg.data_dir.join("spool.db");
    if !path.exists() {
        return Ok(());
    }
    tracing::info!("检测到遗留本地 spool，开始一次性迁移（先清积压，再推游标，最后删除）");
    let mut spool = Spool::open(&path, cfg.max_pending_events, cfg.max_spool_bytes)?;

    // 1) 尽力清空积压（bounded 时长 + 无进展护栏）
    let deadline = Instant::now() + MIGRATION_DRAIN_DEADLINE;
    let mut no_progress = 0u32;
    loop {
        if Instant::now() > deadline {
            return Err(AgentError::Internal(
                "清积压超时（10 分钟），保留 spool 下次启动重试".into(),
            ));
        }
        let (batch_id, events) = spool.next_batch(cfg.batch_max_events, cfg.batch_max_bytes);
        if events.is_empty() {
            break;
        }
        let batch = build_batch(identity, &batch_id, &events);
        match client.upload(&batch) {
            Ok(resp) => {
                let to_ack: Vec<String> = resp
                    .accepted
                    .iter()
                    .chain(resp.duplicate.iter())
                    .cloned()
                    .collect();
                if !to_ack.is_empty() {
                    spool.ack_uploaded(&batch_id, &to_ack)?;
                }
                let retryable: Vec<String> = resp
                    .failed
                    .iter()
                    .filter(|f| f.retryable)
                    .map(|f| f.event_id.clone())
                    .collect();
                let fatal: Vec<(String, String)> = resp
                    .failed
                    .iter()
                    .filter(|f| !f.retryable)
                    .map(|f| (f.event_id.clone(), f.reason.clone()))
                    .collect();
                if !retryable.is_empty() {
                    spool.fail_events(&batch_id, &retryable, true, "hub 重试")?;
                }
                if !fatal.is_empty() {
                    tracing::error!(
                        "迁移清积压：Hub 永久拒绝 {} 条（随 spool 删除）样例: {}",
                        fatal.len(),
                        fatal
                            .iter()
                            .take(3)
                            .map(|(id, r)| format!("{id}({r})"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    let fatal_ids: Vec<String> = fatal.iter().map(|(id, _)| id.clone()).collect();
                    spool.fail_events(&batch_id, &fatal_ids, false, "hub 拒绝")?;
                }
                if to_ack.is_empty() && !resp.failed.is_empty() {
                    no_progress += 1;
                    if no_progress >= 20 {
                        return Err(AgentError::Internal(
                            "积压事件持续被 Hub 拒绝，中止迁移（保留 spool）".into(),
                        ));
                    }
                } else {
                    no_progress = 0;
                }
            }
            Err(e) => {
                return Err(AgentError::Http(format!(
                    "迁移清积压失败（保留 spool）: {e}"
                )));
            }
        }
    }

    // 2) 推游标（积压清空后，游标代表的扫描位之前的数据已全部确认）
    let cursors = spool.all_cursors();
    if !cursors.is_empty() {
        let entries: Vec<CursorEntry> = cursors
            .into_iter()
            .map(|(source_id, cursor_json)| CursorEntry {
                source_id,
                cursor_json,
                updated_at: None,
            })
            .collect();
        let n = entries.len();
        client.push_cursors(&entries)?;
        tracing::info!("已推送 {n} 条本地游标到 Hub");
    }

    // 3) 删除 spool 文件，进入无状态模式
    drop(spool);
    for name in ["spool.db", "spool.db-wal", "spool.db-shm"] {
        let p = cfg.data_dir.join(name);
        if p.exists() {
            std::fs::remove_file(&p)?;
        }
    }
    tracing::info!("遗留 spool 迁移完成，本地数据库已删除，进入无状态轮询");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentConfig;
    use metria_core::model::ContentHash;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Mutex;

    fn test_cfg(data_dir: &std::path::Path) -> AgentConfig {
        AgentConfig {
            node_id: String::new(),
            node_name: "test".into(),
            hub_url: None,
            token: Some("tok".into()),
            listen_port: 0,
            claude_path: None,
            codex_path: None,
            opencode_path: None,
            content_mode: metria_core::ContentMode::Metadata,
            data_dir: data_dir.to_path_buf(),
            max_pending_events: 1000,
            max_spool_bytes: 10_000_000,
            batch_max_events: 256,
            batch_max_bytes: 1024 * 1024,
            scan_interval_seconds: 10,
            reconcile_interval_seconds: 300,
            heartbeat_interval_seconds: 60,
            upload_interval_seconds: 15,
            poll_interval_seconds: 60,
            token_refresh_interval_seconds: 6 * 24 * 3600,
            log_filter: "error".into(),
        }
    }

    fn identity() -> ScanIdentity {
        ScanIdentity {
            node_id: "node-x".into(),
            collector_id: "collector-node-x".into(),
        }
    }

    fn ev(id: &str) -> PendingEvent {
        PendingEvent {
            event_id: id.into(),
            kind: "usage".into(),
            payload: serde_json::json!({"n": 1}),
        }
    }

    fn jsonl_cursor(offset: i64) -> SourceCursor {
        SourceCursor::Jsonl(metria_core::model::JsonlCursor {
            canonical_path_hash: ContentHash::hash_str("path"),
            file_identity: "id".into(),
            inode: 1,
            size: 200,
            mtime: 0,
            byte_offset: offset,
            last_event_hash: None,
            last_scan_at: None,
        })
    }

    /// 脚本化 fake hub：按序响应，记录 (请求行, body)。
    fn spawn_scripted_hub(
        responses: Vec<(u16, String)>,
        requests: Arc<Mutex<Vec<String>>>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for (status, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let Some((request_line, req_body)) = read_http_request(&mut stream) else {
                    break;
                };
                requests.lock().unwrap().push(format!(
                    "{request_line} {}",
                    String::from_utf8_lossy(&req_body)
                ));
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        addr
    }

    fn read_http_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
        let mut buf = Vec::new();
        let mut one = [0u8; 1];
        loop {
            match stream.read(&mut one) {
                Ok(0) => return None,
                Ok(_) => {
                    buf.push(one[0]);
                    if buf.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                Err(_) => return None,
            }
        }
        let head = String::from_utf8_lossy(&buf).to_string();
        let mut lines = head.split("\r\n");
        let request_line = lines.next().unwrap_or("").to_string();
        let mut content_length = 0usize;
        for line in lines {
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            stream.read_exact(&mut body).ok()?;
        }
        Some((request_line, body))
    }

    fn client(addr: &str) -> HubClient {
        HubClient::new(&format!("http://{addr}"), Some("tok".into()))
    }

    fn ack_body(ids: &[&str]) -> String {
        serde_json::json!({
            "batch_id": "b1",
            "ok": true,
            "accepted": ids,
            "duplicate": [],
            "failed": [],
            "message": null,
        })
        .to_string()
    }

    #[test]
    fn fetch_cursors_404_means_unsupported() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let addr = spawn_scripted_hub(vec![(404, "{}".into())], requests.clone());
        let got = client(&addr).fetch_cursors().unwrap();
        assert!(got.is_none());
        assert!(requests.lock().unwrap()[0].starts_with("GET /api/v1/collectors/cursors"));
    }

    #[test]
    fn push_cursors_posts_json() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let addr = spawn_scripted_hub(vec![(200, r#"{"ok":true}"#.into())], requests.clone());
        let entry = CursorEntry {
            source_id: "s1".into(),
            cursor_json: r#"{"Jsonl":{"byte_offset":10}}"#.into(),
            updated_at: None,
        };
        client(&addr).push_cursors(&[entry]).unwrap();
        let recorded = requests.lock().unwrap();
        assert!(recorded[0].starts_with("POST /api/v1/collectors/cursors"));
        assert!(recorded[0].contains("s1"));
    }

    #[test]
    fn advance_only_after_full_ack() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let addr = spawn_scripted_hub(
            vec![(200, ack_body(&["e1"])), (200, r#"{"ok":true}"#.into())],
            requests.clone(),
        );
        let cfg = test_cfg(&std::env::temp_dir());
        let uploaded = upload_and_advance(
            &cfg,
            &client(&addr),
            &identity(),
            "s1",
            &[ev("e1")],
            Some(&jsonl_cursor(10)),
        )
        .unwrap();
        assert_eq!(uploaded, 1);
        let recorded = requests.lock().unwrap();
        assert!(recorded[0].contains("/events/batch"));
        assert!(recorded[1].contains("/collectors/cursors"));
    }

    #[test]
    fn no_advance_when_upload_fails() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let addr = spawn_scripted_hub(vec![(500, "{}".into())], requests.clone());
        let cfg = test_cfg(&std::env::temp_dir());
        let result = upload_and_advance(
            &cfg,
            &client(&addr),
            &identity(),
            "s1",
            &[ev("e1")],
            Some(&jsonl_cursor(10)),
        );
        assert!(result.is_err());
        let recorded = requests.lock().unwrap();
        assert!(!recorded.iter().any(|r| r.contains("/collectors/cursors")));
    }

    #[test]
    fn fatal_rejection_still_advances() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let failed = serde_json::json!({
            "batch_id": "b1",
            "ok": false,
            "accepted": [],
            "duplicate": [],
            "failed": [{"event_id": "e1", "reason": "invalid", "retryable": false}],
            "message": null,
        })
        .to_string();
        let addr = spawn_scripted_hub(
            vec![(200, failed), (200, r#"{"ok":true}"#.into())],
            requests.clone(),
        );
        let cfg = test_cfg(&std::env::temp_dir());
        let uploaded = upload_and_advance(
            &cfg,
            &client(&addr),
            &identity(),
            "s1",
            &[ev("e1")],
            Some(&jsonl_cursor(10)),
        )
        .unwrap();
        assert_eq!(uploaded, 1);
        assert!(requests.lock().unwrap()[1].contains("/collectors/cursors"));
    }

    #[test]
    fn retryable_failure_blocks_advance() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let failed = serde_json::json!({
            "batch_id": "b1",
            "ok": false,
            "accepted": [],
            "duplicate": [],
            "failed": [{"event_id": "e1", "reason": "busy", "retryable": true}],
            "message": null,
        })
        .to_string();
        let addr = spawn_scripted_hub(vec![(200, failed)], requests.clone());
        let cfg = test_cfg(&std::env::temp_dir());
        let result = upload_and_advance(
            &cfg,
            &client(&addr),
            &identity(),
            "s1",
            &[ev("e1")],
            Some(&jsonl_cursor(10)),
        );
        assert!(result.is_err());
        assert!(!requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("/collectors/cursors")));
    }

    #[test]
    fn batch_too_large_splits_recursively() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        // 首批 413 → 二分拆批（2+1）→ 各 200 → 推游标
        let addr = spawn_scripted_hub(
            vec![
                (413, "{}".into()),
                (200, ack_body(&["e1", "e2"])),
                (200, ack_body(&["e3"])),
                (200, r#"{"ok":true}"#.into()),
            ],
            requests.clone(),
        );
        let cfg = test_cfg(&std::env::temp_dir());
        let events = vec![ev("e1"), ev("e2"), ev("e3")];
        let uploaded = upload_and_advance(
            &cfg,
            &client(&addr),
            &identity(),
            "s1",
            &events,
            Some(&jsonl_cursor(10)),
        )
        .unwrap();
        assert_eq!(uploaded, 3);
        let recorded = requests.lock().unwrap();
        assert_eq!(recorded.len(), 4);
        assert!(recorded[3].contains("/collectors/cursors"));
    }

    #[test]
    fn chunk_events_respects_limits() {
        let events: Vec<PendingEvent> = (0..5).map(|i| ev(&format!("e{i}"))).collect();
        let chunks = chunk_events(&events, 2, 1_000_000);
        assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), 5);
        assert!(chunks.iter().all(|c| c.len() <= 2));
        // 超小字节预算：逐事件成块
        let chunks = chunk_events(&events, 256, 10);
        assert_eq!(chunks.len(), 5);
    }

    #[test]
    fn migration_drains_pushes_and_removes_spool() {
        let dir = std::env::temp_dir().join(format!("metria-mig-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let spool_path = dir.join("spool.db");
        {
            let mut spool = Spool::open(&spool_path, 100, 10_000_000).unwrap();
            spool
                .insert_batch(
                    &[ev("m1"), ev("m2")],
                    &[crate::spool::CursorUpdate {
                        source_id: "s1".into(),
                        cursor_json: r#"{"Jsonl":{"byte_offset":50}}"#.into(),
                    }],
                )
                .unwrap();
        }
        let requests = Arc::new(Mutex::new(Vec::new()));
        let addr = spawn_scripted_hub(
            vec![
                (200, ack_body(&["m1", "m2"])),
                (200, r#"{"ok":true}"#.into()),
            ],
            requests.clone(),
        );
        let cfg = test_cfg(&dir);
        migrate_legacy_spool(&cfg, &client(&addr), &identity()).unwrap();
        assert!(!spool_path.exists(), "迁移成功后应删除 spool");
        let recorded = requests.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert!(recorded[0].contains("/events/batch"));
        assert!(recorded[1].contains("/collectors/cursors"));
        assert!(recorded[1].contains("byte_offset"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_keeps_spool_when_hub_down() {
        let dir = std::env::temp_dir().join(format!("metria-mig-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let spool_path = dir.join("spool.db");
        {
            let mut spool = Spool::open(&spool_path, 100, 10_000_000).unwrap();
            spool.insert_batch(&[ev("m1")], &[]).unwrap();
        }
        let requests = Arc::new(Mutex::new(Vec::new()));
        let addr = spawn_scripted_hub(vec![(500, "{}".into())], requests.clone());
        let cfg = test_cfg(&dir);
        assert!(migrate_legacy_spool(&cfg, &client(&addr), &identity()).is_err());
        assert!(spool_path.exists(), "迁移失败应保留 spool");
        assert!(!requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("/collectors/cursors")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
