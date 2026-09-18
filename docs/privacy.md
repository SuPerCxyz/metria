# 隐私与数据诚实性

## 1. 默认不上传的内容

- 完整绝对路径（只上传 `path_hash = blake3(path)`）
- 用户名 / Hostname / Node 名（默认只由 Admin 配置）
- Git Remote（默认哈希）
- 环境变量 / API Key / Authorization / Cookie / SSH 私钥 / 数据库连接串
- Token 与 Secret（日志中禁止输出）

## 2. 内容模式（content_mode）

| 模式 | 行为 |
|---|---|
| `none` | 不上传正文，仅元数据与估算 |
| `metadata`（默认） | 元数据 + 正文长度/哈希，正文脱敏 |
| `full` | 上传正文（经脱敏过滤） |

Agent 本地先脱敏，Hub 二次脱敏（纵深防御）。

## 3. 脱敏实现（metria-core::privacy）

- 路径 → blake3
- URL 中的 token/key/secret/password/access_token/api_key/sig/signature → 擦除
- `Authorization` / `Proxy-Authorization` / `Cookie` 头 → 替换
- SSH 私钥 / 连接串关键词 → 擦除

## 4. 数据诚实性硬性规则

- 新的字节字段必须标记为 `observed_*_bytes`，仅表示临时观测链路看到的 payload/wire 字节，禁止标记为实际/精确/网卡/账单流量。
- 缺失 Token 用 `null`，禁止默认填 0。
- 禁止把估算 Token 冒充 reported、calculated cost 冒充 reported cost；旧估算流量不再进入新采集链路。
- 禁止把 Session 级统计伪装成单次 Model Call；`call_granularity` 必须诚实标注。
- 费用三口径并存：`reported_cost` / `calculated_cost` / `estimated_cost`，各自可追溯。
- 禁止把 Cache Token 直接等同于网络字节；禁止把 Reasoning Token 全部换算为响应字节。
- 观测字节不可由 Token 或固定系数推算；缺数据时标记 `unavailable`，不硬造。
- 外部价格目录必须保存来源与快照；OpenRouter 价格标记 channel，LiteLLM 提示为第三方数据。
- 价格更新不得覆盖历史快照；历史 Traffic Profile/估算表仅为升级兼容保留，不再由当前版本更新。

## 5. 运行时观测质量

- `observed`：原生临时观测链路直接看到并派生；
- `partial`：只能证明部分阶段或字节层级；
- `unavailable`：Docker/普通日志采集无法证明，保持 `null`，不以估算值替代。

## 6. 访问控制

- 单 Admin 登录（会话 token，env 注入初始凭据）。
- Collector 凭独立 token（仅存哈希，7 天有效期）。
- Share Link 为公开只读脱敏视图（不含正文与敏感信息），带查看审计。
- MCP 服务为只读查询（`metria mcp`），不暴露写入能力。

## 7. 用量报告的数据边界

- 报告只发送周期汇总指标、费用口径和排行，不包含会话正文、提示词、代码或完整客户端路径；观测字节不进入旧流量报表。
- SMTP 邮件和 Webhook 都是主动向外部目标投递数据；管理员应确认收件人、Webhook URL 和第三方服务的隐私策略。
- SMTP 密码和 Webhook Secret 保存在 Hub 本地 SQLite，读取配置的 API 不回传 SMTP 密码；应像保护数据库和备份文件一样保护 `/data` 卷。
- 邮件图表是 HTML 正文中的内嵌 SVG；PDF 渲染能力不会改变报告的数据边界，也不会作为邮件附件发送。
- 自动调度失败后当前周期不会持续重试，避免错误配置造成重复外发；修复后可通过设置页手动测试。
