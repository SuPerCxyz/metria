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

- 流量必须标记「估算流量」，禁止标记为实际/精确/网卡/账单流量。
- 缺失 Token 用 `null`，禁止默认填 0。
- 禁止把估算 Token 冒充 reported、calculated cost 冒充 reported cost、估算流量冒充实际流量。
- 禁止把 Session 级统计伪装成单次 Model Call；`call_granularity` 必须诚实标注。
- 费用三口径并存：`reported_cost` / `calculated_cost` / `estimated_cost`，各自可追溯。
- 禁止把 Cache Token 直接等同于网络字节；禁止把 Reasoning Token 全部换算为响应字节。
- 禁止用固定「1 Token = 4 Bytes」作为唯一估算算法；系数必须版本化（TrafficProfile 版本化）。
- 禁止生成下界=中值=上界的估算区间；缺数据时标记 `unavailable`，不硬造。
- 外部价格目录必须保存来源与快照；OpenRouter 价格标记 channel，LiteLLM 提示为第三方数据。
- 价格更新 / Profile 更新不得覆盖历史快照与历史估算（重新计价/重新估算保留新旧版本）。

## 5. 估算来源（estimation_source，7 级）

```
reconstructed > partial > content_bytes > token_profile > user_profile > builtin > unavailable
```

- `reconstructed`：基于请求/响应内容重建
- `partial`：部分重建（缺隐藏内容，降 confidence）
- `content_bytes`：基于可见内容字节
- `token_profile`：Token × bytes-per-token 系数（版本化 Profile，含 cache/reasoning 因子）
- `user_profile` / `builtin`：用户或内置 Profile 估算
- `unavailable`：缺数据，不硬造

## 6. 访问控制

- 单 Admin 登录（会话 token，env 注入初始凭据）。
- Collector 凭独立 token（仅存哈希，7 天有效期）。
- Share Link 为公开只读脱敏视图（不含正文与敏感信息），带查看审计。
- MCP 服务为只读查询（`metria mcp`），不暴露写入能力。
