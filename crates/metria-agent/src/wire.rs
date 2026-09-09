//! Hub HTTP 客户端（阻塞栈，ureq + rustls）。

use std::time::Duration;

use metria_protocol::{
    CursorEntry, CursorSyncRequest, CursorSyncResponse, HeartbeatRequest, HeartbeatResponse,
    RegisterRequest, RegisterResponse, UploadBatch, UploadResponse,
};

use crate::error::{AgentError, Result};

/// Hub 客户端。
#[derive(Debug, Clone)]
pub struct HubClient {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl HubClient {
    pub fn new(base: &str, token: Option<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("metria-agent/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            agent,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base, path)
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        let mut req = match method {
            "POST" => self.agent.post(&self.url(path)),
            "GET" => self.agent.get(&self.url(path)),
            _ => self.agent.get(&self.url(path)),
        };
        if let Some(t) = &self.token {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        req.set("Content-Type", "application/json")
    }

    /// 注册 Node + Collector。
    pub fn register(&self, req: &RegisterRequest) -> Result<RegisterResponse> {
        let resp = self
            .request("POST", "/collectors/register")
            .send_json(serde_json::to_value(req).map_err(|e| AgentError::Serde(e.to_string()))?)
            .map_err(|e| AgentError::Http(format!("register 失败: {e}")))?;
        resp.into_json::<RegisterResponse>()
            .map_err(|e| AgentError::Http(format!("register 响应解析失败: {e}")))
    }

    /// 心跳。
    pub fn heartbeat(&self, req: &HeartbeatRequest) -> Result<HeartbeatResponse> {
        let resp = self
            .request("POST", "/collectors/heartbeat")
            .send_json(serde_json::to_value(req).map_err(|e| AgentError::Serde(e.to_string()))?)
            .map_err(|e| AgentError::Http(format!("heartbeat 失败: {e}")))?;
        resp.into_json::<HeartbeatResponse>()
            .map_err(|e| AgentError::Http(format!("heartbeat 响应解析失败: {e}")))
    }

    /// 上传压缩批次（zstd）。
    pub fn upload(&self, batch: &UploadBatch) -> Result<UploadResponse> {
        let json = serde_json::to_vec(batch).map_err(|e| AgentError::Serde(e.to_string()))?;
        let compressed = zstd::encode_all(&json[..], 3)
            .map_err(|e| AgentError::Http(format!("zstd 压缩失败: {e}")))?;
        let resp = self
            .agent
            .post(&self.url("/events/batch"))
            .set("Content-Type", "application/json")
            .set("Content-Encoding", "zstd")
            .apply_auth(self.token.as_deref())
            .send_bytes(&compressed);
        // 413：批次过大（压缩后超限），调用方可据此拆批重试
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(413, _)) => return Err(AgentError::BatchTooLarge),
            Err(e) => return Err(AgentError::Http(format!("upload 失败: {e}"))),
        };
        let status = resp.status();
        if status == 413 {
            return Err(AgentError::BatchTooLarge);
        }
        resp.into_json::<UploadResponse>()
            .map_err(|e| AgentError::Http(format!("upload 响应解析失败 (status {status}): {e}")))
    }

    /// 拉取当前 collector 全部 Source 游标；Hub 不支持（404）时返回 None。
    pub fn fetch_cursors(&self) -> Result<Option<Vec<CursorEntry>>> {
        match self.request("GET", "/collectors/cursors").call() {
            Ok(resp) => {
                let parsed: CursorSyncResponse = resp
                    .into_json()
                    .map_err(|e| AgentError::Http(format!("游标响应解析失败: {e}")))?;
                Ok(Some(parsed.cursors))
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(AgentError::Http(format!("拉取游标失败: {e}"))),
        }
    }

    /// 推进游标（事件确认上传后调用；幂等 upsert）。
    pub fn push_cursors(&self, cursors: &[CursorEntry]) -> Result<()> {
        if cursors.is_empty() {
            return Ok(());
        }
        let body = CursorSyncRequest {
            schema_version: metria_protocol::limits::SCHEMA_VERSION,
            cursors: cursors.to_vec(),
        };
        self.request("POST", "/collectors/cursors")
            .send_json(serde_json::to_value(&body).map_err(|e| AgentError::Serde(e.to_string()))?)
            .map_err(|e| AgentError::Http(format!("推进游标失败: {e}")))?;
        Ok(())
    }

    /// 健康检查。
    pub fn healthz(&self) -> Result<()> {
        self.agent
            .get(&format!("{}/healthz", self.base))
            .call()
            .map_err(|e| AgentError::Http(format!("healthz 失败: {e}")))?;
        Ok(())
    }
}

trait ApplyAuth {
    fn apply_auth(self, token: Option<&str>) -> Self;
}

impl ApplyAuth for ureq::Request {
    fn apply_auth(self, token: Option<&str>) -> Self {
        match token {
            Some(t) => self.set("Authorization", &format!("Bearer {t}")),
            None => self,
        }
    }
}
