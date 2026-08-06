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
| Session | 全量 spec 字段：tokens 四类+reasoning、cost 三值、traffic 三项、confidence、parent_session_id、status |
| Turn / Message | role/sequence/usage_source/granularity；content_type/content_hash/content_length/utf8_bytes/redacted |
| ModelCall | call_granularity(message/call/turn/session)、streaming/stream_completed/retry_count、usage_event_id/traffic_estimate_id |
| UsageEvent | event_id=blake3、usage 四值可 null、cost 三值、quality 三件套、不可变 |
| TrafficEstimate | request/response payload/http/wire、total/lower/upper、estimation_source(7 级)、context_transport_mode(4 级)、profile_id/version、confidence |
| TrafficProfile | p50/p75/p90、fixed、overhead ratio、cache transport factor、effective_from/to、version、source(5 类)、enabled |
| PricingCatalog/Rule/Match | 金额微美元、priority/effective 区间、source 含 builtin_catalog/client_reported/user_override |

## 2. Hub 数据库（28 表）

| 分组 | 表 |
|---|---|
| 身份 | users / nodes / collectors / collector_tokens |
| 来源 | clients / sources / projects / source_errors |
| 会话事件 | sessions / turns / messages / model_calls / usage_events / tool_events / subagent_relations / traffic_estimates |
| 流量 | traffic_profiles / traffic_profile_samples |
| 价格 | pricing_catalogs / pricing_snapshots / pricing_rules / pricing_matches |
| 汇总 | hourly_rollups / daily_rollups |
| 分享/上传 | share_links / share_audits / upload_batches |
| 系统 | server_meta / schema_migrations（由 storage 运行时建） |

### 关键约束

- `sessions.id` = 规范键 `{node_id}:{source_session_id}`，保证幂等与跨表 join。
- `usage_events.event_id` 唯一；重复上传靠它去重。
- `upload_batches.batch_id` 唯一；批次幂等。
- `collector_tokens.token_hash` 唯一且仅存哈希；`expires_at` 默认 7 天（迁移后新注册 token）。
- rollup 主键 = `(bucket, node_id, collector_id, client_id, source_id, project_id, provider, model, usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level)`。

## 3. 事件类型（Ingest 白名单）

`session / source / call / usage / traffic / tool / subagent / traffic_sample`。

各事件经过 `metria_protocol::validate_batch` 校验：schema 版本、事件数 ≤256、
单事件 ≤2MiB、JSON 深度 ≤32、解压后 ≤8MiB（zstd 解码带大小上限，防 zip bomb）。

## 4. 数据诚实性

- 缺失 Token 用 `null`，禁止默认填 0。
- 费用三口径并存：`reported_cost` / `calculated_cost` / `estimated_cost`，各自可追溯。
- 流量标记「估算流量」；`estimation_source` 7 级优先级：
  reconstructed > partial > content_bytes > token_profile > user_profile > builtin > unavailable。
- 禁止下界=中值=上界；缺数据标记 `unavailable`。
- 价格更新 / 重新估算保留历史版本（快照 + 新 traffic_estimates）。

## 5. 迁移策略

- 文件命名 `migrations/N_name.sql`（N 递增），rust-embed 编译期嵌入。
- 事务内执行，记录 `schema_migrations`；启动时 quick_check。
- 变更数据库必须先加迁移，禁止直接改表（保证升级/回滚可控）。
