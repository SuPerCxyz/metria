# 数据模型

## 1. 领域对象（metria-core::model）

金额统一 `i64` 微美元；时间统一 `DateTime<Utc>`；ID 用 `Id`（ULID-like）与
`EventId`（blake3 内容哈希）。

| 模型 | 关键字段 |
|---|---|
| Node | id/name/labels/platform/arch/timezone/first_seen/last_seen/status |
| Collector | id/agent_version/protocol_version/last_heartbeat/clock_skew_seconds/spool_pending/spool_size |
| Client | canonical_name/display_name/category |
| Source | adapter_id/version/fingerprint/path_hash/client_version/status/capabilities/scan 时间戳/last_error |
| Session | tokens 四类+reasoning、cost 三值、content_available、parent_session_id、status |
| Turn / Message | role/sequence/usage_source/granularity；content_type/content_hash/content_length/utf8_bytes/redacted |
| ModelCall | call_granularity(message/call/turn/session)、streaming/stream_completed/retry_count、TTFT/首字节/生成时长/Token 每秒、ITL/停顿、可靠性、路由、`observed_*_bytes`、usage_event_id；旧 `traffic_estimate_id` 仅为兼容字段 |
| UsageEvent | event_id=blake3、usage 四值可 null、cost 三值、quality 三件套、不可变 |
| Runtime observation | `first_byte_at`/`first_token_at`/`last_output_at`、`ttft_ms`、`generation_duration_ms`、`output_tokens_per_second_milli`、ITL/停顿、`observability_source`/`quality`、状态/限流/finish reason 与观测 payload/wire 字节 |
| Legacy traffic | `traffic_estimates`、`traffic_profiles`、`traffic_profile_samples` 及旧字段仅为升级兼容保留；当前 Agent 不生成，Hub 不写入、不查询、不展示 |
| PricingCatalog/Rule/Match | 金额微美元、priority/effective 区间、source 含 builtin_catalog/client_reported/user_override |

### 1.1 Token 口径

Adapter 落库前统一归一化，各客户端语义一致：

- `input_tokens` 为**非缓存输入**（扣除 `cache_read_tokens` 与 `cache_write_tokens`）。
- `output_tokens` 为**不含推理的生成 Token**（存储口径）；页面与报告的「输出」按 `output_tokens + reasoning_tokens` 展示（**含推理**，与 OpenAI/Codex 及 ccswitch 一致），推理在明细中另标为“其中推理”，不重复相加。
- `reasoning_tokens` 单独计列；计费按推理单价，未配置推理价时回退输出价（推理按输出价计费）；不换算为响应字节。
- 总 Token（v2，2026-09-16 起）= `input + output + reasoning + cache_read + cache_write`，含缓存读写，等价于 ccswitch 用量面板的消耗总量；缓存命中率与缓存节省费用仍单独展示。v1（2026-09-15 之前）为 `input + output + reasoning`（不含缓存），跨版本数值不可比。
- 缓存命中率 = `cache_read / (input + cache_write + cache_read)`；无缓存数据或分母为 0 时标记为不可用。

## 2. Hub 数据库（32 表）

| 分组 | 表 |
|---|---|
| 身份 | users / nodes / collectors / collector_tokens |
| 来源 | clients / sources / projects / source_errors |
| 会话事件 | sessions / turns / messages / model_calls / usage_events / tool_events / subagent_relations |
| 历史兼容 | traffic_estimates / traffic_profiles / traffic_profile_samples（不再新增写入） |
| 价格 | pricing_catalogs / pricing_snapshots / pricing_rules / pricing_matches |
| 汇总 | hourly_rollups / daily_rollups |
| 分享/上传 | share_links / share_audits / upload_batches |
| 报告与状态 | settings / report_sends / source_cursors |
| 系统 | server_meta / schema_migrations（由 storage 运行时建） |

`settings` 使用键值存储保存全局时区和报告配置；SMTP 密码不会通过读取接口回传。
`report_sends` 记录每个报告渠道的周期、状态和错误详情，数据库内只保留最新 30 条。

### 关键约束

- `sessions.id` = 规范键 `{node_id}:{source_session_id}`，保证幂等与跨表 join。
- `usage_events.event_id` 唯一；重复上传靠它去重。
- `upload_batches.batch_id` 唯一；批次幂等。
- `collector_tokens.token_hash` 唯一且仅存哈希；`expires_at` 默认 7 天（迁移后新注册 token）。
- rollup 主键 = `(bucket, node_id, collector_id, client_id, source_id, project_id, provider, model, usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level)`。

## 3. 事件类型（Ingest 白名单）

当前事件为 `session / source / call / usage / tool / subagent`。为兼容旧 Agent，Hub
过渡期仍识别 `traffic / traffic_sample`，但会标记为已接受并丢弃，不写入数据库或 rollup。

各事件经过 `metria_protocol::validate_batch` 校验：schema 版本、事件数 ≤256、
单事件 ≤2MiB、JSON 深度 ≤32、解压后 ≤8MiB（zstd 解码带大小上限，防 zip bomb）。

## 4. 数据诚实性

- 缺失 Token 用 `null`，禁止默认填 0。
- 费用三口径并存：`reported_cost` / `calculated_cost` / `estimated_cost`，各自可追溯。
- 运行时字节必须标记为 `observed_*_bytes`，区分 payload bytes 与 wire bytes；它们只表示
  观测器看到的转发数据，不冒充网卡或账单流量。
- Docker/日志采集无法证明 TTFT、Token/s 或字节时保持 `null`，不使用估算值补齐。
- 价格更新保留历史快照；旧估算流量表不再参与新的费用或性能查询。

## 5. 迁移策略

- 文件命名 `migrations/N_name.sql`（N 递增），rust-embed 编译期嵌入。
- 事务内执行，记录 `schema_migrations`；启动时 quick_check。
- 变更数据库必须先加迁移，禁止直接改表（保证升级/回滚可控）。
