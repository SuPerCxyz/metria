//! metria import：将客户端数据源目录导入为归一化 NDJSON 事件。

use std::io::Write;
use std::path::PathBuf;

use metria_adapter_api::{DiscoveryContext, ScanIdentity};

/// import 子命令入口。
pub fn run_import(
    source: &str,
    path: &str,
    dry_run: bool,
    out: Option<&str>,
) -> Result<(), String> {
    let adapter = crate::registry::adapter(source)
        .ok_or_else(|| format!("未知数据源 `{source}`，可用：claude | codex | opencode"))?;

    let ctx = DiscoveryContext {
        node_id: "import".into(),
        collector_id: "import".into(),
        root_paths: vec![PathBuf::from(path)],
    };
    let sources = adapter
        .discover(&ctx)
        .map_err(|e| format!("发现失败: {e}"))?;
    if sources.is_empty() {
        eprintln!("在 `{path}` 未发现任何 {source} 数据源");
        return Ok(());
    }

    let identity = ScanIdentity {
        node_id: "import".into(),
        collector_id: "import".into(),
    };

    let mut writer: Box<dyn Write> = match out {
        Some(p) => Box::new(std::fs::File::create(p).map_err(|e| e.to_string())?),
        None => Box::new(std::io::stdout()),
    };

    let mut total = ImportTotals::default();
    for src in &sources {
        let batch = adapter
            .scan(src, None, &identity)
            .map_err(|e| format!("扫描 {} 失败: {e}", src.canonical_path.display()))?;
        total.sessions += batch.sessions.len() as u64;
        total.calls += batch.model_calls.len() as u64;
        total.usage += batch.usage_events.len() as u64;
        total.traffic += batch.traffic_estimates.len() as u64;

        eprintln!(
            "{}: sessions={} calls={} usage={} traffic={} warnings={}",
            src.canonical_path.display(),
            batch.sessions.len(),
            batch.model_calls.len(),
            batch.usage_events.len(),
            batch.traffic_estimates.len(),
            batch.warnings.len()
        );
        if dry_run {
            continue;
        }
        for s in &batch.sessions {
            write_event(&mut writer, "session", s)?;
        }
        for c in &batch.model_calls {
            write_event(&mut writer, "call", c)?;
        }
        for u in &batch.usage_events {
            write_event(&mut writer, "usage", u)?;
        }
        for t in &batch.traffic_estimates {
            write_event(&mut writer, "traffic", t)?;
        }
    }
    eprintln!(
        "完成：sessions={} calls={} usage={} traffic={}",
        total.sessions, total.calls, total.usage, total.traffic
    );
    Ok(())
}

#[derive(Debug, Default)]
struct ImportTotals {
    sessions: u64,
    calls: u64,
    usage: u64,
    traffic: u64,
}

fn write_event<W: Write, T: serde::Serialize>(
    w: &mut W,
    kind: &str,
    value: &T,
) -> Result<(), String> {
    let line = serde_json::json!({ "kind": kind, "data": value });
    serde_json::to_writer(&mut *w, &line).map_err(|e| e.to_string())?;
    w.write_all(b"\n").map_err(|e| e.to_string())?;
    Ok(())
}
