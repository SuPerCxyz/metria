//! 原生 Agent 的临时 HTTP 流观测器。
//!
//! 观测器只在 `metria observe` 的子进程生命周期内存在：客户端通过进程级
//! base URL 访问本地端口，观测器转发到上游并只保留调用级派生指标。

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use chrono::Utc;
use metria_core::model::{Cost, EventId, Id, Quality, Usage, UsageEvent, UsageGranularity};
use metria_protocol::{BatchEvent, RegisterRequest, UploadBatch};
use serde_json::Value;

use crate::config::AgentConfig;
use crate::error::{AgentError, Result};
use crate::observer_metrics::{CallIdentity, ObservedCall};
use crate::observer_proxy::proxy_loop;
use crate::spool::{PendingEvent, Spool};
use crate::wire::HubClient;

/// 启动一次临时观测并执行客户端命令。
pub fn run(
    cfg: AgentConfig,
    client_id: &str,
    upstream: Option<&str>,
    provider: Option<&str>,
    command: &[String],
) -> Result<()> {
    if command.is_empty() || command[0].trim().is_empty() {
        return Err(AgentError::Internal("observe 需要客户端命令".into()));
    }
    let upstream = resolve_upstream(client_id, upstream)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let temporary_config = if client_id == "opencode" {
        Some(prepare_opencode_config(
            provider,
            &format!("http://127.0.0.1:{port}"),
        )?)
    } else {
        None
    };
    let stop = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let proxy_stop = stop.clone();
    let proxy_calls = calls.clone();
    let proxy_client = client_id.to_string();
    let proxy_upstream = upstream.clone();
    let proxy_thread = thread::spawn(move || {
        proxy_loop(
            listener,
            proxy_upstream,
            proxy_client,
            proxy_stop,
            proxy_calls,
        )
    });
    handle_signal(stop.clone());

    let mut child = Command::new(&command[0]);
    child.args(&command[1..]);
    child.stdin(Stdio::inherit());
    child.stdout(Stdio::inherit());
    child.stderr(Stdio::inherit());
    apply_client_env(&mut child, client_id, port, temporary_config.as_deref());
    configure_child_process(&mut child);
    let mut cleanup = RuntimeCleanup::new(stop.clone(), proxy_thread, temporary_config);
    tracing::info!(client = client_id, port, "临时模型观测已启动");
    let mut child = child.spawn()?;
    let lease_seconds = std::env::var("METRIA_OBSERVE_LEASE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(24 * 60 * 60);
    let started = Instant::now();
    let child_result = loop {
        if stop.load(Ordering::Relaxed) {
            stop_child(&mut child);
        }
        if lease_expired(started, Duration::from_secs(lease_seconds)) {
            stop.store(true, Ordering::Relaxed);
            stop_child(&mut child);
            break Err(AgentError::Internal("observe 租约已到期".into()));
        }
        match child.try_wait()? {
            Some(status) => break Ok(status),
            None => thread::sleep(Duration::from_millis(200)),
        }
    };
    cleanup.shutdown();
    let calls = calls
        .lock()
        .map_err(|_| AgentError::Internal("观测结果锁损坏".into()))?
        .clone();
    if let Err(e) = upload_calls(&cfg, client_id, calls) {
        tracing::warn!(%e, "观测结果上传失败");
    }
    let status = child_result?;
    if !status.success() {
        return Err(AgentError::Internal(format!("客户端退出状态: {status}")));
    }
    Ok(())
}

struct RuntimeCleanup {
    stop: Arc<AtomicBool>,
    proxy_thread: Option<thread::JoinHandle<()>>,
    temporary_config: Option<std::path::PathBuf>,
}

impl RuntimeCleanup {
    fn new(
        stop: Arc<AtomicBool>,
        proxy_thread: thread::JoinHandle<()>,
        temporary_config: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            stop,
            proxy_thread: Some(proxy_thread),
            temporary_config,
        }
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.proxy_thread.take() {
            let _ = thread.join();
        }
        if let Some(path) = self.temporary_config.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for RuntimeCleanup {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lease_expired(started: Instant, lease: Duration) -> bool {
    started.elapsed() >= lease
}

fn configure_child_process(command: &mut Command) {
    #[cfg(unix)]
    {
        command.process_group(0);
        #[cfg(target_os = "linux")]
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    let _ = command;
}

fn stop_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        if pid > 0 {
            // 负 PID 向整个观测子进程组发送信号，避免客户端派生进程继续使用本地端口。
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
        }
    }
    let _ = child.kill();
}

fn resolve_upstream(client_id: &str, upstream: Option<&str>) -> Result<String> {
    if let Some(value) = upstream.filter(|v| !v.trim().is_empty()) {
        return Ok(value.trim_end_matches('/').to_string());
    }
    let env_name = match client_id {
        "claude" | "claude-code" => "ANTHROPIC_BASE_URL",
        "codex" => "OPENAI_BASE_URL",
        "opencode" => "METRIA_OBSERVE_UPSTREAM",
        _ => return Err(AgentError::Internal(format!("不支持的客户端: {client_id}"))),
    };
    if let Ok(value) = std::env::var(env_name) {
        if !value.trim().is_empty() {
            return Ok(value.trim_end_matches('/').to_string());
        }
    }
    match client_id {
        "claude" | "claude-code" => Ok("https://api.anthropic.com".into()),
        "codex" => Ok("https://api.openai.com/v1".into()),
        _ => Err(AgentError::Internal(
            "OpenCode 需要 --upstream 或 METRIA_OBSERVE_UPSTREAM（按当前 provider 配置填写）"
                .into(),
        )),
    }
}

fn apply_client_env(
    command: &mut Command,
    client_id: &str,
    port: u16,
    config: Option<&std::path::Path>,
) {
    let base = format!("http://127.0.0.1:{port}");
    match client_id {
        "claude" | "claude-code" => {
            command.env("ANTHROPIC_BASE_URL", &base);
        }
        "codex" => {
            command.env("OPENAI_BASE_URL", &base);
            command.env("OPENAI_API_BASE", &base);
        }
        "opencode" => {
            if let Some(config) = config {
                command.env("OPENCODE_CONFIG", config);
            }
        }
        _ => {}
    }
    command.env("METRIA_OBSERVE_ACTIVE", "1");
}

fn prepare_opencode_config(provider: Option<&str>, local_base: &str) -> Result<std::path::PathBuf> {
    let source = opencode_config_path().ok_or_else(|| {
        AgentError::Internal("未找到 OpenCode 配置；请设置 OPENCODE_CONFIG 后重试观测".into())
    })?;
    prepare_opencode_config_file(&source, provider, local_base)
}

fn prepare_opencode_config_file(
    source: &std::path::Path,
    provider: Option<&str>,
    local_base: &str,
) -> Result<std::path::PathBuf> {
    let text = std::fs::read_to_string(source)?;
    let cleaned = strip_jsonc(&text);
    let mut value: Value = serde_json::from_str(&cleaned)
        .map_err(|e| AgentError::Internal(format!("OpenCode 配置解析失败: {e}")))?;
    let providers = if value.get("providers").is_some() {
        value.get_mut("providers")
    } else {
        value.get_mut("provider")
    }
    .and_then(Value::as_object_mut)
    .ok_or_else(|| AgentError::Internal("OpenCode 配置缺少 providers/provider".into()))?;
    let selected = match provider {
        Some(name) => name.to_string(),
        None if providers.len() == 1 => providers.keys().next().cloned().unwrap_or_default(),
        None => {
            return Err(AgentError::Internal(
                "OpenCode 配置包含多个 provider，请使用 --provider 指定观测目标".into(),
            ))
        }
    };
    let Some(config) = providers.get_mut(&selected).and_then(Value::as_object_mut) else {
        return Err(AgentError::Internal(format!(
            "OpenCode provider 不存在: {selected}"
        )));
    };
    let section = config
        .entry("settings")
        .or_insert_with(|| serde_json::json!({}));
    let settings = section
        .as_object_mut()
        .ok_or_else(|| AgentError::Internal("OpenCode provider settings 不是对象".into()))?;
    settings.insert("baseURL".into(), Value::String(local_base.into()));
    if let Some(options) = config.get_mut("options").and_then(Value::as_object_mut) {
        options.insert("baseURL".into(), Value::String(local_base.into()));
    }
    let path = std::env::temp_dir().join(format!(
        "metria-opencode-observe-{}-{}.json",
        std::process::id(),
        EventId::from_content(&selected)
            .as_str()
            .get(7..19)
            .unwrap_or("tmp")
    ));
    let serialized =
        serde_json::to_vec_pretty(&value).map_err(|e| AgentError::Serde(e.to_string()))?;
    std::fs::write(&path, serialized)?;
    Ok(path)
}

fn opencode_config_path() -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("OPENCODE_CONFIG") {
        candidates.push(std::path::PathBuf::from(path));
    }
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        candidates.push(std::path::PathBuf::from(&dir).join("opencode/opencode.json"));
        candidates.push(std::path::PathBuf::from(dir).join("opencode/opencode.jsonc"));
    }
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        candidates.push(std::path::PathBuf::from(&home).join(".config/opencode/opencode.json"));
        candidates.push(std::path::PathBuf::from(&home).join(".config/opencode/opencode.jsonc"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(std::path::PathBuf::from(&local).join("opencode/opencode.json"));
        candidates.push(std::path::PathBuf::from(local).join("opencode/opencode.jsonc"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// 足够覆盖 OpenCode 配置常见 JSONC 注释和尾逗号，不解析或输出配置正文到日志。
fn strip_jsonc(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut string = false;
    let mut escape = false;
    while let Some(ch) = chars.next() {
        if string {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                string = false;
            }
            continue;
        }
        if ch == '"' {
            string = true;
            output.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    output.push('\n');
                    break;
                }
            }
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    let mut cleaned = output;
    loop {
        let next = cleaned
            .replace(",\n}", "\n}")
            .replace(",\r\n}", "\r\n}")
            .replace(",\n]", "\n]")
            .replace(",\r\n]", "\r\n]");
        if next == cleaned {
            break;
        }
        cleaned = next;
    }
    cleaned
}

fn upload_calls(cfg: &AgentConfig, client_id: &str, calls: Vec<ObservedCall>) -> Result<()> {
    if calls.is_empty() {
        return Ok(());
    }
    let token = cfg
        .token
        .clone()
        .ok_or_else(|| AgentError::Internal("observe 需要 METRIA_AGENT_TOKEN".into()))?;
    let node_id = if cfg.node_id.trim().is_empty() {
        format!(
            "node-{}",
            &EventId::from_content(&cfg.node_name).as_str()[7..15]
        )
    } else {
        cfg.node_id.clone()
    };
    let (collector_id, hub) = if let Some(hub_url) = cfg.hub_url.as_deref() {
        let hub = HubClient::new(hub_url, Some(token));
        let registered = hub.register(&RegisterRequest {
            schema_version: metria_protocol::limits::SCHEMA_VERSION,
            node_id: node_id.clone(),
            node_name: cfg.node_name.clone(),
            node_platform: Some(std::env::consts::OS.into()),
            node_architecture: Some(std::env::consts::ARCH.into()),
            node_timezone: Some("UTC".into()),
            agent_version: metria_core::VERSION.to_string(),
            protocol_version: metria_protocol::limits::PROTOCOL_VERSION,
            container_image: None,
            collector_id_hint: None,
        })?;
        (registered.collector_id, Some(hub))
    } else {
        // Pull 模式由 Hub 拉取时会按节点 token 重写批次和事件身份。
        (format!("collector-{node_id}"), None)
    };
    let source_id = format!("runtime:{client_id}");
    let source_session_id = format!("runtime:{client_id}:{}", std::process::id());
    let session_id = Id::new().as_str().to_string();
    let started_at = calls
        .iter()
        .map(|c| c.started_at)
        .min()
        .unwrap_or_else(Utc::now);
    let session = serde_json::json!({
        "id": session_id,
        "source_session_id": source_session_id,
        "node_id": node_id,
        "collector_id": collector_id,
        "source_id": source_id,
        "client_id": client_id,
        "started_at": started_at,
        "status": "ended",
        "message_count": 0,
        "tool_call_count": 0,
        "subagent_count": 0,
        "model_call_count": calls.len(),
        "content_available": false,
        "created_at": started_at,
    });
    let source = serde_json::json!({
        "id": source_id.clone(),
        "node_id": node_id.clone(),
        "collector_id": collector_id.clone(),
        "client_id": client_id,
        "adapter_id": "runtime-http",
        "adapter_version": metria_core::VERSION,
        "source_fingerprint": format!("runtime:{client_id}"),
        "source_path_hash": format!("runtime:{client_id}"),
        "capabilities": [
            "runtime_observation",
            "ttft",
            "output_speed",
            "observed_bytes"
        ],
        "status": "active",
    });
    let mut events = vec![
        BatchEvent {
            kind: "source".into(),
            event_id: EventId::from_content(&format!("runtime-source:{source_session_id}"))
                .to_string(),
            payload: source,
        },
        BatchEvent {
            kind: "session".into(),
            event_id: EventId::from_content(&format!("runtime-session:{source_session_id}"))
                .to_string(),
            payload: session,
        },
    ];
    for observed in calls {
        let call_id = Id::new().as_str().to_string();
        let identity = CallIdentity {
            id: &call_id,
            node_id: &node_id,
            collector_id: &collector_id,
            source_id: &source_id,
            source_session_id: &source_session_id,
            session_id: &session_id,
            client_id,
        };
        let call = observed.to_payload(&identity);
        let usage = UsageEvent {
            schema_version: 1,
            event_id: EventId::from_content("runtime-placeholder"),
            node_id: node_id.clone(),
            collector_id: collector_id.clone(),
            source_id: source_id.clone(),
            client_id: client_id.into(),
            adapter_id: client_id.into(),
            adapter_version: metria_core::VERSION.into(),
            session_id: Some(session_id.clone()),
            turn_id: None,
            model_call_id: Some(call_id.clone()),
            timestamp: observed.started_at,
            provider_raw: Some(client_id.into()),
            provider_normalized: Some(client_id.into()),
            model_raw: observed.model.clone(),
            model_normalized: observed.model.clone(),
            usage: Usage {
                input: observed.input_tokens,
                output: observed.output_tokens,
                cache_read: observed.cache_read_tokens,
                cache_write: observed.cache_write_tokens,
                reasoning: observed.reasoning_tokens,
            },
            cost: Cost::default(),
            quality: Quality {
                usage_source: "reported".into(),
                granularity: UsageGranularity::Call,
                confidence: Some(1.0),
            },
        }
        .finalize()
        .map_err(|e| AgentError::Internal(e.to_string()))?;
        events.push(BatchEvent {
            kind: "call".into(),
            event_id: EventId::from_content(&format!("runtime-call:{call_id}")).to_string(),
            payload: call,
        });
        events.push(BatchEvent {
            kind: "usage".into(),
            event_id: usage.event_id.to_string(),
            payload: serde_json::to_value(usage).map_err(|e| AgentError::Serde(e.to_string()))?,
        });
    }
    let batch = UploadBatch {
        schema_version: metria_protocol::limits::SCHEMA_VERSION,
        batch_id: EventId::from_content(&format!("runtime-batch:{source_session_id}")).to_string(),
        node_id,
        collector_id,
        agent_version: metria_core::VERSION.to_string(),
        events,
    };
    let upload_result = match hub {
        Some(hub) => hub.upload(&batch),
        None => Err(AgentError::Internal(
            "observe 使用本地 spool，等待 Pull Hub 拉取".into(),
        )),
    };
    match upload_result {
        Ok(_) => Ok(()),
        Err(upload_error) => {
            let mut spool = Spool::open(
                &cfg.data_dir.join("spool.db"),
                cfg.max_pending_events,
                cfg.max_spool_bytes,
            )
            .map_err(|spool_error| {
                AgentError::Internal(format!(
                    "观测上传失败且无法写入 spool: {upload_error}; {spool_error}"
                ))
            })?;
            let pending = batch
                .events
                .into_iter()
                .map(|event| PendingEvent {
                    event_id: event.event_id,
                    kind: event.kind,
                    payload: event.payload,
                })
                .collect::<Vec<_>>();
            spool.insert_batch(&pending, &[])?;
            tracing::warn!(%upload_error, "观测结果已写入本地 spool，等待 Agent 下次迁移上传");
            Ok(())
        }
    }
}

fn handle_signal(stop: Arc<AtomicBool>) {
    let _ = ctrlc::set_handler(move || stop.store(true, Ordering::Relaxed));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observer_metrics::has_output_delta;

    #[test]
    fn computes_observed_speed_without_ttft() {
        let started = Utc::now();
        let mut call =
            ObservedCall::new(started, "https://example.test/v1".into(), Some("m".into()));
        call.first_token_at = Some(started + chrono::Duration::milliseconds(500));
        call.last_output_at = Some(started + chrono::Duration::milliseconds(2500));
        call.output_tokens = Some(100);
        let identity = CallIdentity {
            id: "call",
            node_id: "node",
            collector_id: "collector",
            source_id: "source",
            source_session_id: "session",
            session_id: "sid",
            client_id: "codex",
        };
        let value = call.to_payload(&identity);
        assert_eq!(value["ttft_ms"], 500);
        assert_eq!(value["generation_duration_ms"], 2000);
        assert_eq!(value["output_tokens_per_second_milli"], 50_000);
    }

    #[test]
    fn output_delta_detection_does_not_store_text() {
        let value = serde_json::json!({"choices":[{"delta":{"content":"secret"}}]});
        assert!(has_output_delta(&value));
    }

    #[test]
    fn normal_stop_stops_proxy_and_removes_temporary_config() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                thread::yield_now();
            }
        });
        let mut cleanup = RuntimeCleanup::new(stop.clone(), worker, Some(path.clone()));
        cleanup.shutdown();
        assert!(stop.load(Ordering::Relaxed));
        assert!(!path.exists());
    }

    #[test]
    fn observer_failure_cleanup_runs_on_drop() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                thread::yield_now();
            }
        });
        {
            let _cleanup = RuntimeCleanup::new(stop.clone(), worker, Some(path.clone()));
        }
        assert!(stop.load(Ordering::Relaxed));
        assert!(!path.exists());
    }

    #[test]
    fn lease_expiry_and_child_exit_paths_are_detectable() {
        assert!(lease_expired(
            Instant::now() - Duration::from_secs(2),
            Duration::from_secs(1)
        ));
        assert!(!lease_expired(Instant::now(), Duration::from_secs(60)));

        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let worker = thread::spawn(|| {});
        let mut cleanup = RuntimeCleanup::new(stop, worker, Some(path.clone()));
        cleanup.shutdown();
        assert!(!path.exists());
    }

    #[test]
    fn opencode_wrapper_does_not_mutate_original_config() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let original = br#"{
          // keep this comment in the source file
          "providers": {"openai": {"settings": {"baseURL": "https://api.example"}}}
        }"#;
        std::fs::write(file.path(), original).unwrap();
        let wrapped =
            prepare_opencode_config_file(file.path(), Some("openai"), "http://127.0.0.1:1234")
                .unwrap();
        assert_eq!(std::fs::read(file.path()).unwrap(), original);
        let wrapped_value: Value =
            serde_json::from_slice(&std::fs::read(&wrapped).unwrap()).unwrap();
        assert_eq!(
            wrapped_value["providers"]["openai"]["settings"]["baseURL"],
            "http://127.0.0.1:1234"
        );
        std::fs::remove_file(wrapped).unwrap();
    }
}
