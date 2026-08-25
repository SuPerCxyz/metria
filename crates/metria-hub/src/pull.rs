//! Hub Pull 调度器：主动出站拉取配置了 Agent 地址的节点。
//!
//! 每轮遍历 `agent_url` 非空节点：`GET /collect`（Bearer 节点 token）→ 复用现有
//! ingest 校验/幂等/rollup → `POST /ack`。单节点失败指数退避（内存态），不影响其他节点。
//! push 模式节点（未配置 agent_url）不参与调度。

use std::collections::HashMap;
use std::io::Read;
use std::time::{Duration, Instant};

use metria_protocol::{AckRequest, UploadBatch};

use crate::api::process_batch;
use crate::api::AppState;

const COLLECT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// 启动 Pull 调度后台任务。
pub fn spawn_pull_scheduler(st: AppState) {
    let interval_secs: u64 = std::env::var("METRIA_PULL_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v >= 5)
        .unwrap_or(60);
    tokio::spawn(async move {
        // 延迟启动，避免阻塞启动路径
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut backoffs: HashMap<String, (u32, Instant)> = HashMap::new();
        loop {
            let nodes = st.db.list_pull_nodes();
            for (node_id, agent_url, token_enc) in nodes {
                // 节点级退避：未到期跳过
                if let Some((fails, until)) = backoffs.get(&node_id) {
                    if *fails > 0 && Instant::now() < *until {
                        continue;
                    }
                }
                let result = pull_node(&st, &node_id, &agent_url, token_enc.as_deref());
                match result {
                    Ok(()) => {
                        backoffs.remove(&node_id);
                    }
                    Err(e) => {
                        let entry = backoffs
                            .entry(node_id.clone())
                            .or_insert((0, Instant::now()));
                        entry.0 = entry.0.saturating_add(1).min(20);
                        let delay = (Duration::from_secs(60) * 2u32.saturating_pow(entry.0.min(8)))
                            .min(MAX_BACKOFF);
                        entry.1 = Instant::now() + delay;
                        tracing::warn!(
                            node = %node_id,
                            error = %e,
                            fails = entry.0,
                            "Pull 拉取失败（退避 {}s）",
                            delay.as_secs()
                        );
                        let _ = st
                            .db
                            .record_pull_result(&node_id, Some(&e), chrono::Utc::now());
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        }
    });
}

/// 拉取单个节点：collect → ingest → ack。成功返回 Ok。
fn pull_node(
    st: &AppState,
    node_id: &str,
    agent_url: &str,
    token_enc: Option<&str>,
) -> Result<(), String> {
    let token_enc = token_enc.ok_or("节点缺少 pull token（请在 Web 重新生成安装命令）")?;
    let token = crate::crypto::decrypt_node_token(token_enc)
        .ok_or("节点 token 解密失败（METRIA_SESSION_SECRET 是否变更？请重新生成安装命令）")?;
    let collector_id = st
        .db
        .get_node_collector_id(node_id)
        .ok_or("节点缺少 collector 记录")?;
    let base = agent_url.trim_end_matches('/');

    // ---- /collect ----
    let resp = ureq::get(&format!("{base}/collect"))
        .timeout(COLLECT_TIMEOUT)
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("collect 请求失败: {e}"))?;
    let status = resp.status();
    if status == 204 {
        st.db
            .record_pull_result(node_id, None, chrono::Utc::now())
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    if status != 200 {
        return Err(format!("collect 返回意外状态 {status}"));
    }
    let mut compressed = Vec::new();
    resp.into_reader()
        .read_to_end(&mut compressed)
        .map_err(|e| format!("collect 读取失败: {e}"))?;
    if compressed.len() > metria_protocol::limits::MAX_COMPRESSED_BODY {
        return Err("collect 批次超过压缩上限".into());
    }
    let json = zstd::stream::decode_all(compressed.as_slice())
        .map_err(|e| format!("collect 解压失败: {e}"))?;
    let mut batch: UploadBatch =
        serde_json::from_slice(&json).map_err(|e| format!("collect 批次解析失败: {e}"))?;
    if let Err(e) = metria_protocol::validate_batch(&batch) {
        return Err(format!("collect 批次校验失败: {e}"));
    }

    // 身份归属：pull 模式 agent 不感知 node_id/collector_id，按 token 对应节点重写
    batch.node_id = node_id.to_string();
    batch.collector_id = collector_id.clone();
    for ev in &mut batch.events {
        if let Some(obj) = ev.payload.as_object_mut() {
            obj.insert("node_id".into(), serde_json::Value::String(node_id.into()));
            obj.insert(
                "collector_id".into(),
                serde_json::Value::String(collector_id.clone()),
            );
        }
    }

    // ---- ingest（复用现有校验/幂等/rollup）----
    let resp = process_batch(st, &batch, json.len() as i64);
    if !resp.ok {
        let detail: Vec<String> = resp
            .failed
            .iter()
            .map(|f| format!("{}: {}", f.event_id, f.reason))
            .collect();
        return Err(format!("批次部分失败（保留待重拉）: {}", detail.join("; ")));
    }

    // ---- /ack ----
    let ack = ureq::post(&format!("{base}/ack"))
        .timeout(COLLECT_TIMEOUT)
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(AckRequest {
            batch_id: batch.batch_id.clone(),
        })
        .map_err(|e| format!("ack 请求失败: {e}"))?;
    if ack.status() != 200 {
        return Err(format!("ack 返回意外状态 {}", ack.status()));
    }

    st.db
        .record_pull_result(node_id, None, chrono::Utc::now())
        .map_err(|e| e.to_string())?;
    Ok(())
}
