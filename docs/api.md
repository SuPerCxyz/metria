# Hub API 参考

所有 API 前缀 `/api/v1`。除 `/healthz`、`/api/v1/auth/login`、`/api/v1/share/{slug}` 和 Agent 二进制下载接口外均需认证。

## 认证

- **Admin**：`POST /auth/login` `{username, password}` → `{token}`；之后 `Authorization: Bearer <token>`。
  `logout` / `me` / `profile` / `change-password`。首次初始化凭据由 `METRIA_ADMIN_USER` / `METRIA_ADMIN_PASSWORD` 注入，修改密码后以 SQLite users 记录为准。
- **Collector**：`Authorization: Bearer <collector-token>`。token 仅存哈希，默认有效期 7 天
  （`collector_tokens.expires_at`，过期需重新注册）。也可通过 `METRIA_COLLECTOR_TOKEN` 配置共享 bootstrap token。
- **SSE**：`/stream` 因 EventSource 无法带 Header，允许 `?token=` 传会话 token。

## Collector 协议

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/collectors/register` | 注册 node+collector，返回 node_id/collector_id；校验协议版本（不兼容 → 400） |
| POST | `/collectors/heartbeat` | 心跳 + spool 状态 + agent_clock（Hub 计算 clock_skew） |
| GET | `/collectors/status` | collector 状态 |
| GET | `/collectors/config` | 下发配置（当前固定 metadata） |
| POST | `/events/batch` | zstd/raw 批上传；校验后幂等落库 + 增量 rollup |

上传校验：schema 版本、事件数 ≤256、单事件 ≤2MiB、JSON 深度 ≤32、解压后 ≤8MiB（zstd 限长防 zip bomb）。
响应含 `accepted / duplicate / failed`（部分成功语义，failed 可标记 retryable）。

## 查询

通用参数：`from/to/timezone/granularity/allocation_mode` + 维度过滤 + 分页（`limit`）。
分析图表支持用逗号分隔的 `exclude_client_ids` / `exclude_models` 排除多个 Agent 或模型；
适用于 `/overview`、`/usage/timeseries`、`/usage/breakdown` 和 `/usage/latency*`，用于图例隐藏后的联动统计。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/overview` | 汇总统计卡片（读 rollup） |
| GET | `/usage/timeseries` | Token/Cost/Traffic 时间序列 |
| GET | `/usage/breakdown` | 按 Node 汇总 |
| GET | `/nodes` `/nodes/{id}` | Node 列表 / 详情 |
| GET | `/nodes/{id}/install` | Admin 生成节点专属 Token、动态 Hub 地址和平台安装命令；可传 `hub_url` 查询参数覆盖当前地址 |
| GET | `/nodes/{id}/agent/download` | 公开下载该节点平台/架构对应的 Agent 二进制，不接收 Token；支持 Linux amd64/arm64 与 Windows amd64 |
| GET | `/agent/download` | 公开下载当前 Hub 架构的 Agent 二进制（兼容入口） |
| GET | `/nodes/{id}/clients` `/sessions` `/calls` | Node 下的来源/会话/调用 |
| GET | `/clients` `/clients/{id}` `/clients/{id}/models` | Client 列表/详情/模型 |
| GET | `/models` `/models/{id}` | 模型列表/详情 |
| GET | `/calls` `/calls/{id}` | 调用列表/详情 |
| GET | `/sessions` `/sessions/{id}` | 会话列表/详情 |
| GET | `/sessions/{id}/calls` `/tools` `/timeline` `/subagents` | 会话明细 |
| GET | `/traffic/summary` `/traffic/by-node|client|model|provider` | 流量汇总与分维 |
| GET | `/data-quality` | 数据来源分布与解析告警 |
| GET | `/system/info` | 当前内容保存模式、时区和数据保留状态（只读） |
| GET | `/export` | 导出（JSON/NDJSON/CSV） |

账户资料：`GET/PUT /auth/profile` 读取或更新显示名称、头像文字和头像颜色；`POST /auth/change-password` 校验旧密码并更新密码哈希。密码修改成功后现有会话失效。

## 用量报告设置

以下接口均要求 Admin 会话认证，配置保存在 Hub 的 `settings` 表中：

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/settings/report` | 读取报告配置、有效时区、解析后的收件人；SMTP 密码只返回 `smtp_password_set`，不返回密码内容 |
| PUT | `/settings/report` | 保存全局时区、收件人、SMTP、Webhook、日/周/月调度和邮件图表开关 |
| POST | `/settings/report/test` | 按当前配置立即测试所有启用渠道；返回 `ok` 和每个渠道的结果/错误详情 |
| GET | `/settings/report/history` | 返回最近 30 条发送记录，包含时间、类型、周期、渠道、状态和详情 |

报告配置示例：

```json
{
  "timezone": "Asia/Shanghai",
  "email_enabled": true,
  "webhook_enabled": false,
  "recipients": "owner@example.com",
  "smtp": {
    "host": "smtp.example.com",
    "port": 587,
    "username": "owner@example.com",
    "password": "只在保存时提供，读取时不回传",
    "from": "Metria <owner@example.com>",
    "tls": "starttls"
  },
  "schedules": {
    "daily": {"enabled": true, "time": "12:00"},
    "weekly": {"enabled": false, "time": "12:00", "weekday": 0},
    "monthly": {"enabled": false, "time": "12:00", "day": 1}
  },
  "attachments_enabled": true
}
```

`attachments_enabled` 的含义是“邮件附带图表”，只控制 HTML 正文中的内嵌 SVG，不会发送 PDF 附件。自动调度对同一 `类型 + 周期` 只尝试一次，失败后不会在当前周期持续重试；测试发送使用独立的测试周期。

## Traffic Profiles / Pricing / Share

| 方法 | 路径 | 说明 |
|---|---|---|
| GET/POST | `/traffic/profiles` | 列表 / 新建用户 profile |
| DELETE | `/traffic/profiles/{id}` | 删除用户 profile |
| POST | `/traffic/profiles/learn` | 从样本聚合 learned profile（P50/P75/P90） |
| POST | `/traffic/profiles/test` | 匹配测试 |
| POST | `/traffic/reestimate` | 历史重新估算（保留新版本） |
| GET | `/pricing/catalogs` `/snapshots` `/rules` | 目录/快照/规则列表 |
| POST | `/pricing/rules` | 新建用户规则 |
| POST | `/pricing/catalogs/{id}/refresh` | 手动同步外部目录（OpenRouter/LiteLLM/Custom） |
| POST | `/pricing/test` | 规则匹配测试 |
| POST | `/pricing/reprice` | 历史重新计价（保留历史快照） |
| POST | `/shares` | 创建分享（session/node，返回公开只读链接） |
| GET | `/shares` | 分享列表 |
| GET | `/share/{slug}` | 公开只读视图（脱敏 DTO，无需认证） |

## SSE

`GET /stream`：推送 `usage.created / call.updated / session.updated / traffic.estimated / rollup.updated`；
30 秒心跳 ping。前端据此 invalidate 对应查询（增量刷新，不整站刷新）。

## 错误格式

统一 `{"error": "<code>", "message": "<中文说明>"}`。
状态码：400 参数/校验错误、401 未认证、404 未找到、413 超限、500 内部错误。
