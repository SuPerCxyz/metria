# Metria 开发计划文档（M1：核心闭环 + 精简 Web）

- 版本：v1.0
- 状态：执行中（S0 已完成 ✅）
- 范围：M1（Phase 0–2 全链路 + 基础 Traffic/Pricing + 精简 Web + Demo）
- 创建日期：2026-08-05

## 进度

| 阶段 | 状态 | 完成日期 |
|---|---|---|
| S0 工程骨架 | ✅ 完成 | 2026-08-05 |
| S1 统一领域模型 + Adapter | ✅ 完成 | 2026-08-05 |
| S2 Agent 与 Hub 闭环 | ✅ 完成 | 2026-08-05 |
| S3 精简 Web + Demo | ✅ 完成 | 2026-08-05 |
| M2 Traffic Profile 学习/管理/重估 | ✅ 完成 | 2026-08-05 |
| M3 价格目录同步/重新计价 | ✅ 完成 | 2026-08-05 |
| M6 分享/导出/MCP/备份恢复 | ✅ 完成 | 2026-08-05 |
| M7 基准/运维文档 | ✅ 完成 | 2026-08-05 |
| 生产完善：CI 多架构构建修复/验证、设置与分享页 | ✅ 完成 | 2026-08-06 |
| 收尾：token 7 天有效期、Claude 子代理、Web Waterfall/子代理树/模型切换、i18n | ✅ 完成 | 2026-08-06 |
| 查缺补漏：rollup 对账/checkpoint、协议协商、ingest 限长、前端测试、集成测试、文件拆分、docs | ✅ 完成 | 2026-08-06 |
| 全量补齐：allocation_mode/cursor 分页/排序/虚拟滚动、Agent Tools Detail、argon2+签名 token、token 轮换/吊销、Pricing 编辑、Overview 汇总、jitter、TOML 合并、incremental_vacuum、Node 分布 | ✅ 完成 | 2026-08-06 |
| UI 增强与质量修复：Models Detail、Node Collector 信息、Data Quality 增强、doctor --hub 增强、body limit 修复、handler 自死锁修复 | ✅ 完成 | 2026-08-06 |
| 计划缺口全量修复：pricing 编辑/删除 bug、CLI export、分享落地页、doctor 最近上传、ingest 关系校验、参数统一、Web spec 字段补齐 | ✅ 完成 | 2026-08-06 |
| Web 前端重构：基于 Mosaic Lite 模板（React/Vite/Tailwind/Chart.js），6 阶段渐进式重建 | ✅ 完成 | 2026-08-06 |
| Web 时间范围刷新：快捷范围按点击时刻重算，自定义范围保持固定 | ✅ 完成 | 2026-08-12 |
| Web 全量视觉 QA：全路由/主题/响应式/交互/状态检查与公共组件修复 | 🟡 前端完成，仓库既有测试阻塞 | 2026-08-13 |
| Codex 实时增量用量：恢复跨扫描上下文、刷新 Session 汇总、定向回填 | ✅ 完成 | 2026-08-13 |
| 首页排行与趋势联动修复：排行排序、图例筛选同步汇总、移除需要关注区块 | ✅ 完成 | 2026-09-07 |
| 跨平台 Agent 资产与 CI Node24：Hub 内置下载、Linux/Windows 目标选择、原生安装持久化、Docker 命令复制、Action 运行时升级 | ✅ 完成 | 2026-09-07 |
| Agent 无状态轮询：游标外置 Hub、事件确认后推进、离线补齐、spool 迁移（OpenSpec `stateless-polling-agent`） | ✅ 完成 | 2026-09-09 |
| 用量报告：设置页配置全局时区/收件人/SMTP/通用 Webhook、日/周/月三独立调度、测试发送与历史；邮件含内联趋势图（Token/请求数/模型/Agent）、加宽 760px（OpenSpec `add-usage-reports`） | 🟡 实现完成，失败周期不自动重试、单渠道测试结果去重、邮件 Y 轴与 Web 图表样式一致、历史最多 30 条每页 10 条、PDF 与邮件内容/视觉一致、邮件暂不附带 PDF 已补齐，真实 SMTP + 内联图表已验证，待提交/部署 | 2026-09-11 |

### Codex 实时增量用量修复记录（2026-08-13）

- 根因：Codex rollout 首次扫描消费 `session_meta` 后，后续扫描从 byte offset 继续但解析状态重新为空，
  导致追加的 `token_count` 无法关联会话；Hub 接收跨批次 call 后也未可靠刷新既有 Session 摘要。
- Adapter 在增量尾部解析前从文件头恢复 `session_meta`，并从游标前 1 MiB 有界窗口恢复最新
  `turn_context`；正常游标仍只消费新增完整行，不重复解析历史业务事件，找不到模型时诚实保留 null。
- Hub 每次插入或重试 call 后从已持久化调用回算 Session 的 call 数、nullable token/cost、主模型和
  last activity；不擅自把 active 会话标记 ended，不把未知 token 填 0。
- 新增“先扫描 session、再追加 message/model、最后追加 usage”的 Adapter 回归测试，以及跨批次
  Session 聚合测试；workspace test、fmt、clippy、Docker 构建通过。
- 运行时仅对 4 个经 inode、source cursor 与 Hub zero-call 三重核验的 Codex source 执行恢复；先备份
  Metria spool 的 DB/WAL/SHM，再删除对应 4 条 Metria cursor。Codex 目录全程只读，未修改源文件或配置。
- 回填后 4 个 Session 均展示 `gpt-5.6-sol`、真实 Token 与逐条调用；API 的 Session call count 与 calls
  列表一致，浏览器 Agents、Sessions、Session Detail 实际渲染验证通过，Console 与页面错误为空。

### Web 全量视觉 QA 记录（2026-08-13）

- 基于真实 Router、导航和线上数据检查 9 个主页面、5 类详情页、全部可用 Tab、表格、时间范围
  Popover、用户菜单、移动侧栏、排序、分页、搜索、刷新以及 Loading/Empty/Error/Partial/Large 状态；
- Light/Dark 覆盖全部主页面，并在 1920×1080、1600×900、1440×900、1366×768、1280×800、
  1024×768、768×1024、430×932、390×844、360×800 下完成响应式回归；
- 修复移动 Header 控件重叠与侧栏标签缺失、时间范围 Trigger 固定宽度、价格规则 2030 行无界渲染、
  趋势图 ISO 横轴、DataTable 排序/行键盘语义、反馈状态可访问性；价格规则改为搜索加 20 行分页；
- 修复后全路由无 Body 横向溢出，Console、页面错误和失败网络请求为空；前端定向测试、生产构建、
  fmt、clippy、Hub Docker 镜像及 full Compose 配置通过；Web 使用 Node.js 内置测试运行器，不声明不存在的
  TypeScript typecheck 脚本；
- `cargo test --workspace` 的既有 Hub rollup 对账测试持续失败（2 个 bucket 被判 drift），与本次前端文件无关，
  因此仓库总门禁仍标记阻塞，未在视觉 QA 范围内改动后端 rollup 逻辑。

### Web 时间范围刷新完成记录（2026-08-12）

- 时间范围状态记录快捷项来源；点击顶栏刷新时，「今天」更新为本地 00:00 至当前时间，
  最近 1/3/6/12/24 小时及 7/14/30 天按点击时刻向前重算，「昨天」保持自然日语义。
- 日历和时间控件确认的自定义范围保持固定，刷新只重新请求该固定范围的数据。
- 查询刷新延后到范围状态更新后执行；范围 key 已变化的查询不重复请求，避免旧范围请求覆盖新结果。
- 定向状态测试 4 项与 Web 生产构建、Hub Docker 镜像构建、三份 Compose 配置通过；仓库级
  `cargo test --workspace` 仍有既有 Hub rollup 对账用例失败，未误记为全量门禁通过。

### Web 前端重构完成记录（2026-08-06）

基于 Cruip Mosaic Lite（https://github.com/cruip/tailwind-dashboard-template）重建 Web 前端，
技术栈 React 19 + Vite + Tailwind CSS 4 + React Router 7 + Chart.js，替换原 Preact 实现。

- 阶段1 模板清理：删除销售/电商示例页与组件，保留布局骨架（Sidebar/Header/主题/css/图表配置）；
  建立 Metria 路由（总览/使用分析/会话/节点/Agents/模型/费用/网络流量/设置）、API 对接层
  （services/api.js）、统一数据格式化（services/format.js：数字缩写/Token/费用/流量/时长/时间）、
  全局时间范围（useTimeRange，跨页保持 + 快捷项）。
- 阶段2 总览：4 核心指标卡（总费用/总Token/网络流量/活跃会话）+ 主趋势图（Token/费用/流量/请求
  切换，降采样）+ 模型/Agent 双排行。
- 阶段3 核心列表：会话（默认 8 列/分页/搜索/排序）、节点（含 usage 汇总）、Agents、模型
  （含价格未配置标记）。通用 DataTable 组件（排序/分页/行点击）。
- 阶段4 详情页：会话详情（摘要/趋势图/Token 构成/模型调用列表/错误异常）、节点详情、
  Agent 详情、模型详情、调用详情（估算区间/Profile/缺失说明）。
- 阶段5 专项分析：使用分析（Token/请求/缓存/延迟标签）、费用页、网络流量页。
- 阶段6 体验：错误边界、加载骨架、空/错误状态、深浅色、响应式（指标卡自适应列数、
  移动端侧边栏抽屉）、URL 下钻（排行→详情页）。
- 可复用组件：MetricCard/TrendChart/RankingList/DataTable/TimeRangePicker/FilterBar/
  StatusBadge/EmptyState/ErrorState/LoadingSkeleton/DataQualityBadge/PageHeader/DetailSummary。
- 修复：SessionDetail 的 useMemo 位于条件 return 之后导致 React hooks 数量不一致
  （error #310），hooks 全部移到顶层；前端登录后 RequireAuth 监听器注册问题。
- 部署：vite base=/static/（rust-embed 集成）、HashRouter（兼容 SPA fallback）、
  docker 镜像内嵌新前端。

### 计划缺口全量修复完成记录（2026-08-06）

**🔴 真实缺陷修复**
- S3.8：pricing 规则 update/delete 的 `source` 不匹配（插入 `user_override` vs 更新删除 `user`）
  → 统一为 `user_override`；insert 返回 id；前端规则表 source 判断同步修正。
- S2.13：`metria export` 子命令实现（sessions/calls/usage × json/ndjson/csv，读 Hub SQLite）。
- S3.8：分享链接 `/s/{slug}` 前端落地页（公开只读，调 `/api/v1/share/{slug}`）；并新增
  删除/吊销（`DELETE /shares/{slug}`）与查看审计（`GET /shares/audits`）。
- S2.17：`doctor --hub` 增加最近上传/最近心跳（遍历节点 detail 取 collector）与时钟差
  （HTTP Date 头 vs 本机）。
- S2.10：ingest 增加 node/collector 关系校验（token 身份与 batch 声明一致，403），e2e 覆盖。
- S2.12：demo 数据未来时间偏移（base 回退 1h）；projects 维度表未填充（upsert_session 同步
  insert projects，用同一 conn 避免重入死锁）。

**🟠 后端参数统一**
- S2.13：`allocation_mode` 扩展到 `node_calls`/`node_sessions`；`node_calls` 补齐 cursor 分页。

**🟡 S3 spec 字段补齐（前端 + 后端）**
- S3.3 Overview：AgentTool/Model/Project 计数、Token 构成、Collector 在线状态（≤5min 心跳）。
- S3.4 Nodes：列表 Detected Clients 徽标；Detail 时间范围统计、按 Client 分布。
- S3.5 Client Detail：Project 分布、最近 Calls、Source 健康、版本分布。
- S3.6 Models：列表 Bytes per Input/Output Token；Detail 时间序列（Token/Cost/Traffic）、最近 Calls。
- S3.7 Session：Token Waterfall；Call Detail：duration/Cache Write/Estimated Cost、Profile
  （profile_id/version + 重建质量）、缺失字段说明。
- S3.8 DataQuality：confidence 占比、cursor 状态、告警。
- S3.8 Pricing：effective_from/to、编辑表单（预填 + PUT）。
- S3.8 Shares：删除/吊销、查看审计。

### UI 增强与质量修复完成记录（2026-08-06）

- S3.5 扩展：Models Detail 页（`/models/{id}`）：模型汇总（token/cost/traffic/calls）、
  raw 名称分布、命中定价规则（通配匹配）、最近会话；列表增加 `pricing_source` 列。
- S3.4 扩展：Node Detail 增加 Collector 卡片（agent_version/protocol/status/last_upload/
  clock_skew/spool 统计）；`node_detail` 返回 `collectors` 数组。
- S3.8 扩展：Data Quality 增加来源错误明细（`source_errors`）、来源扫描状态
  （`source_scan`：total/healthy/with_errors/last_scan_at）、时钟偏移告警
  （`clock_skew_warnings`，|skew|>60s）；前端对应卡片。
- S2.9 增强：`metria doctor --hub` 增加 TLS 检测与节点数检查（Admin 登录）。
- 修复：`list_models` / `model_detail` / `client_models` 在持有 `db.conn()` 锁期间
  再次调用 `load_all_rules()` / `list_pricing_rules()` 导致 std Mutex 自死锁（请求永久挂起）——
  以块作用域提前释放 conn 锁。
- 修复：axum 默认 body limit 2MiB 早于 handler 校验，导致 3MiB 请求被直接断连（客户端
  Broken pipe）而非返回 413；显式设置 `DefaultBodyLimit` 为 16MiB，解压后仍由 handler
  校验 8MiB 上限。
- 性能：Agent notify debounce 1s → 500ms。

### 全量补齐完成记录（2026-08-06）

- S2.13：Query API 支持 `allocation_mode`（call_start/call_end）与真实 `cursor` 分页
  （calls/sessions 基于时间+id 排序键，base64 游标，返回 next_cursor）。
- S3.7：Calls/Sessions 列表可点击表头排序；Sessions 列表虚拟滚动（窗口化渲染）；
  Calls 支持「加载更多」分页。
- S3.5：Agent Tools Detail 页（`/clients/{id}`）：Node 分布、模型分布、最近会话，
  后端 client_detail 补充 cost/traffic/recent_sessions。
- S2.14：Admin 密码 argon2（PHC 格式，兼容旧 prehash）；会话签名 token
  （HMAC-SHA256，`METRIA_SESSION_SECRET`），登录/校验/篡改检测测试通过。
- S2.9：token 轮换/吊销 API（`/collectors/{id}/tokens` + `/revoke`，Admin 会话），e2e 覆盖。
- S3.8：Pricing 规则编辑/停用/删除（PUT/DELETE `/pricing/rules/{id}`）+ 前端操作按钮。
- S3.3：Overview 汇总维度切换（node/client/model/provider/project，`dim` 参数）。
- S2.4：上传退避加 jitter；S2.7：SIGTERM 优雅停止 + 收尾等待。
- S0.3：`METRIA_CONFIG_FILE` TOML 合并（env 优先，TOML 兜底）+ 单测。
- S2.11：incremental_vacuum 接入维护任务。
- S3.4：Node Detail 按模型/项目分布统计。
- 豁免项（用户确认不需要）已从 plan 移除：collector config 下发、首登强制改密、多用户。


### 查缺补漏完成记录（2026-08-06）

- Rollup 对账后台任务（S2.12）：`reconcile_rollups` 逐 bucket 对比 raw 与汇总，`rebuild_drift`
  从事件表重建最近 N 天；`spawn_maintenance` 每 6h 对账 + 漂移自动重建 + `wal_checkpoint(TRUNCATE)`（§9）。
- 协议版本协商（S2.9）：`limits::PROTOCOL_VERSION`；不兼容 register 拒绝 400，e2e 覆盖。
- Ingest 校验（S2.10）：单事件 ≤2MiB、JSON 深度 ≤32 进入 `validate_batch`；`zstd_decode` 限长
  防 zip bomb（解压超 8MiB 拒绝）。e2e 覆盖 deep_nested / oversized / zstd_bomb。
- Web 前端测试（S3.11）：Vitest + jsdom，format/i18n/range/api 序列化 4 文件 26 用例。
- 集成测试补全（S2.16）：断网补传（spool 重启续传）、部分成功重传（仅失败子集）、重试耗尽转死信、
  Hub 部分成功响应、heartbeat 时钟偏移（`clock_skew_seconds` 计算与存储）。
- 文件拆分（§7）：`db/mod.rs`(639) + `db/traffic.rs` + `db/pricing.rs`；
  `api/mod.rs`(674) + `api/handlers_query.rs`(817) + `api/handlers_misc.rs`，全部 <800 行。
- docs 补齐（§1）：architecture/data-model/adapters/api/deployment/privacy/development + README 文档链接修正。


### 收尾完成记录（2026-08-06）

- Collector token 7 天有效期（migration 007）：`collector_tokens.expires_at`；注册 upsert 刷新有效期；
  鉴权拒绝过期 token；Agent 每 6 天重新注册续期（`METRIA_TOKEN_REFRESH_INTERVAL`，默认 6 天 < 7 天）。
- Claude 子代理关联：`Task` tool_use 的 `leafUuid`/`sessionId` → 派生 `SubagentRelation` + subagent_count；
  新增 fixture `subagent.jsonl` 与单测。
- Hub API：`session_calls` 增加 `estimated_total_bytes`；新增 `GET /sessions/{id}/subagents`
  （relations + 子会话摘要，按 id / source_session_id 解析）。
- Web：Session Detail 增加「每次调用估算流量」Waterfall（含模型切换标记）、子代理树、
  Model Switch 高亮；i18n 抽象（`web/src/lib/i18n.ts`，zh/en key-value + 插值 + 语言切换，默认中文）。

### S2 完成记录（2026-08-05）

- metria-protocol：注册/心跳/批传/状态/配置线协议 + 上限（256 事件 / 256KiB 压缩 / 8MiB 解压 / 深度 32 / 事件 2MiB）。
- metria-pricing：内置目录（来源标注 builtin_catalog）+ 用户规则，优先级 reported > user 精确 > user 通配 > builtin > unavailable；5 单测。
- metria-agent（blocking 栈，无 tokio）：本地 Spool（事件/游标/批次/死信/来源健康，事务一致，满则停止采集+告警，断网积压，重启续传）；notify 监听 + 增量扫描 + 每 5 分钟 reconcile；zstd 批传 + 指数退避 + 部分成功 + 幂等（event_id）；心跳；Node ID 优先级（显式 > 持久化 > 生成）。
- metria-hub：SQLite schema（004-005 迁移共 27 表）；注册/心跳/批传/status/config；认证中间件（admin 会话 + collector token 分离，token 仅存哈希）；批量 ingest 幂等（新插入/重复/失败三清单）+ 增量 rollup（hourly/daily）；查询 API 子集（overview/timeseries/breakdown/nodes/clients/models/calls/sessions/traffic/data-quality）；SSE（token query 兼容 EventSource）；argon2 占位 + 环境注入 admin。
- e2e 集成测试（metria-hub/tests/e2e.rs）：真实 HTTP 全链路（注册→上传→幂等→overview→401 未授权→非法批次 400→错误 token 401）。
- CLI 端到端验证：fixture 扫描→spool→上传→hub 落库→rollup→查询全通；断网补传语义验证。
- doctor --spool/--database/--hub 补全。

### M2/M3/M6/M7 完成记录（2026-08-05）

- M2 Traffic：agent 生成学习样本并上传；Hub 样本存储 + learned profile 聚合（P50/P75/P90）；
  profile 列表/创建/删除/匹配测试；历史重新估算（生成新版本保留旧版）；metria-traffic
  支持自定义候选 profile；Web Traffic Profiles 页。
- M3 Pricing：PricingEngine 多来源优先级（user > openrouter/custom > litellm > builtin）；
  OpenRouter/LiteLLM/Custom HTTP 目录同步（per-token→微美元/百万、模型名归一化、ETag/304、
  快照+来源保存、失败保留旧快照）；后台周期同步；目录刷新/快照/重新计价 API；Web Pricing 增强。
- M6 分享/导出/MCP/备份：Share Link（公开只读脱敏 DTO + 查看审计）；sessions/calls 导出
  JSON/NDJSON/CSV；`metria mcp` stdio 只读查询（7 个工具）；`metria backup`（VACUUM INTO +
  zstd）/ `metria restore`。
- M7 生产完善：10 万/100 万事件基准 + 价格匹配/流量重建基准（docs/operations.md 记录）；
  运维文档（保留策略/备份/升级/回滚）。

验证：fmt/clippy(-D warnings)/test 全绿；web test+build 通过；各里程碑端到端验证通过。

### S3 完成记录（2026-08-05）

- Web（React+Vite+Tailwind+Chart.js）：hash 路由、登录、侧边导航（总览/Nodes/Agent 工具/模型/会话/调用/流量/数据质量）、全局时间范围选择器（from/to/时区/粒度/快捷项/URL 持久化）、Chart.js 时间序列、统计卡片、表格、Light/Dark、SSE 增量刷新（EventSource + token）、移动端布局、空/加载/错误态。
- Demo 模式：`metria hub --demo` 确定性合成数据（3 节点 / 3 客户端 / 5 模型 / 4 项目 / 7 天），走同一 ingest 路径，不读真实目录；启动约 5s。
- 浏览器冒烟（agent-browser）：登录→总览（37.5M input / $152 cost / 163MiB 估算流量带范围）→Nodes 表格→Traffic 图表→暗色模式全部正常。

### S1 完成记录（2026-08-05）

- metria-core 领域模型全量落地：Id/EventId(blake3)/ContentHash、MicroUsd 金额、全部枚举、Node/Collector/Client/Source/SourceCursor/SourceError、Project、Session/Turn/Message、ModelCall、UsageEvent（finalize 生成稳定 event_id + token 非负校验）、TrafficEstimate/TrafficProfile/TrafficProfileSample、Pricing 模型、ToolEvent/SubagentRelation；归一化（模型/Provider/通配匹配）、脱敏（路径哈希/密钥/URL）、时间分桶（IANA）、内容分类（13 类启发式）。
- metria-traffic：版本化 Traffic Profile（builtin/user/adapter，来源优先级 + p50/p75/p90 校验）、估算核心（reconstructed/partial/content_bytes/token_profile/unavailable 优先级、full_context/stateful_reference/mixed/unknown、cache 因子、reasoning 保守处理、强制下界<中值<上界、置信度）、8 单测。
- metria-adapter-api：SourceAdapter trait（discover/scan/health/traffic_capabilities）+ ScanIdentity、ScanBatch、AdapterCapabilities、TrafficCapabilities、分类错误、JSONL 流式解析（限长/半行/非 UTF-8 容忍）、fixture 测试框架。
- Claude Code Adapter：projects/*.jsonl + 扁平布局发现；modern entry 解析（type/user/assistant/message.usage/cache_*_input_tokens/tool_use/tool_result/summary/ai-title）；turn 分组；ModelCall/UsageEvent/TrafficEstimate 关联；partial_reconstruction + FullContext/FullContentSent；golden+malformed+光标增量 8 测试。
- Codex Adapter：sessions/**/rollout-*.jsonl 发现；session_meta/user_message/token_count(last_token_usage)/response_item(message/reasoning/custom_tool_call/output) 解析；重复 token_count 去重；全零 usage 不产假调用；previous_response_id 检测→stateful_reference；golden+malformed+cursor 6 测试 + 真机冒烟（20 来源/51 调用）。
- OpenCode Adapter：全局 opencode.db + project/*/storage 双布局发现；只读打开（READ_ONLY+busy_timeout+query_only，不改 PRAGMA/不 migration）；message/part 解析（text/reasoning/tool/step）；session.cost→reported_cost(微美元)；parent_id→subagent 关系；rowid 增量游标；schema drift 检测；golden+cursor+drift+lock 4 测试。
- metria import：NDJSON 导出（session/call/usage/traffic），真机导入 14876 次调用验证。
- metria doctor：--adapter（发现/健康/扫描摘要）、--traffic（能力表）、--hub（healthz 连通性）。
- fixtures：claude（golden_full/missing_usage/malformed/non_utf8/truncated_tail）、codex（golden_full/missing_usage/malformed）。
- 门禁：fmt/clippy(-D warnings)/test(98)/web build/docker build/compose config 全绿。

已知限制：adapter 尚不产出 traffic_profile_samples（自动学习在 S2/M2）；Codex 会话级 model 聚合以 message/agent 为粒度。（Claude 子代理关联已修复：Task tool_use 的 leafUuid 推导 SubagentRelation，见 2026-08-06 记录。）

### S0 完成记录（2026-08-05）

- git init（main 分支）+ 根文件（.gitignore/.editorconfig/.dockerignore/rust-toolchain/LICENSE/CHANGELOG/SECURITY/README）。
- Rust workspace：12 个 crate，release profile（lto/opt3/codegen-units=1/strip/panic=abort）。
- `metria` CLI：clap 全子命令；`version`/`healthcheck`/`config` 可用，其余按命令参数执行。
- metria-core：配置（ContentMode/IANA 时区/env 解析）、分层错误类型、tracing 日志初始化。
- metria-storage：SQLite 打开与 PRAGMA（WAL/fk/busy_timeout/quick_check/checkpoint）、rust-embed 版本化迁移框架、Repository 抽象；6 单测通过。
- metria-hub：axum 服务骨架（healthz + 前端 rust-embed + SPA fallback + 优雅退出）、迁移应用、healthcheck 子命令。
- web：React+Vite+Tailwind+Chart.js，light/dark 主题，PWA manifest；test+build 通过。
- Docker 多阶段构建：Node 构建期 / Rust 构建期 / 非 root(65532) 运行时，运行时无 Node.js，镜像 88MB。
- Compose：compose.yaml（hub）/ compose.agent.yaml（agent）/ compose.full.yaml（hub+demo+agent）均通过 `config` 校验。
- 门禁 `scripts/check.sh` 全绿；容器内 healthz/静态资源/SPA fallback/优雅退出实测通过。

---

## 0. 总览

### 0.1 目标

交付一个**可编译、可运行、可测试、可 Docker Compose 部署、浏览器可用、可长期后台运行、可扩展 Adapter** 的 AI 编程 Agent 用量监控 / 费用分析 / 流量估算平台，覆盖「首个可用版本验收标准」中与 M1 对应的条目。

### 0.2 已确认决策

| 决策点 | 结论 |
|---|---|
| 首轮范围 | M1 核心闭环 + 精简 Web |
| 认证 | 单 Admin（env 初始凭据） |
| Spool 满 | 停止采集 + 明确告警（保「断网丢失 0」），可配置 |
| Web 语言 | 中文为主，预留 i18n 抽象 |
| 金额 | 整数微美元（i64），禁止浮点累计 |
| 时间 | 存储 UTC；展示/分桶用 IANA 时区；禁止依赖容器系统时区 |
| 事件 ID | blake3 内容哈希；Call/Usage 边界诚实标注（turn/session） |
| Agent 运行栈 | 无 tokio blocking 栈（目标空闲 RSS ≤35MiB，S2 后实测） |
| Hub 运行栈 | tokio + axum + rusqlite(blocking pool) + rust-embed + SSE |
| 上传批次 | 按 min(事件数, 压缩字节) 先到先拆；Server 双上限校验 |
| 隐私 | 默认 content_mode=metadata，Agent 本地脱敏 + Hub 二次脱敏 |

### 0.3 M1 不包含（后续里程碑）

- Traffic Profile 自动学习 / 用户 Profile Web 管理 / 历史重新估算界面（M2）
- OpenRouter / LiteLLM / Custom HTTP 价格目录同步（M3，M1 仅内置目录 + 用户规则）
- Share Link、Export、MCP、备份/恢复（M6）
- ARM64 镜像、10 万/100 万事件 benchmark、SBOM、Release Pipeline（M7）

### 0.4 总门禁（每个 S 阶段结束必须全绿）

```
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cd web && npm test && npm run build
docker build -f docker/Dockerfile --target hub -t metria:dev .
docker compose -f docker/compose.full.yaml config
```

若任一失败，修复后再进入下一步。不得跳过门禁「声称通过」。

---

## 1. 仓库结构与 Workspace

```
metria/
├── Cargo.toml / Cargo.lock / rust-toolchain.toml
├── .gitignore / .editorconfig / .dockerignore
├── README.md / LICENSE / CHANGELOG.md / SECURITY.md
├── docker/            Dockerfile + compose.yaml + compose.agent.yaml + compose.full.yaml + demo 数据说明
├── crates/
│   ├── metria-core          领域模型·ID·归一化·脱敏·时间·金额·内容分类(基础)
│   ├── metria-protocol      Agent↔Hub 线协议·序列化·校验·限长
│   ├── metria-storage       SQLite 连接/迁移/Repository 抽象
│   ├── metria-pricing       M1 最小价格引擎（内置目录+用户规则+快照+match+reprice）
│   ├── metria-traffic       M1 基础流量估算（字节统计·重建·token profile·置信区间）
│   ├── metria-adapter-api   SourceAdapter trait·类型·错误
│   ├── metria-adapter-claude
│   ├── metria-adapter-codex
│   ├── metria-adapter-opencode
│   ├── metria-agent
│   ├── metria-hub
│   └── metria-cli
├── web/               React+Vite+Tailwind+Chart.js（dist 由 hub embed）
├── migrations/        SQLite 版本化 migration SQL
├── fixtures/          claude/ codex/ opencode/ malformed/ traffic/
├── scripts/           check.sh / build.sh / demo.sh / doctor.sh
└── docs/              architecture.md data-model.md adapters.md api.md deployment.md privacy.md development.md
```

**Cargo.toml 要点**

- `[workspace] resolver = "2"`；crates 依赖方向：
  `core ← protocol ← {traffic,pricing} ← storage ← adapter-api ← {3 adapters} ← agent/hub ← cli`，禁止反向与循环依赖。
- `[profile.release] lto=true opt-level=3 codegen-units=1 strip=true panic="abort"`；
  `[profile.dev.package."*"] opt-level=1` 加速开发。
- rust-toolchain 固定 `stable`（本机 1.96.1）。

---

## 2. S0 — 工程骨架

**DoD**：骨架可编译、容器可构建、compose 可解析、CLI 五个入口可运行、web 骨架产出 dist 且能被 embed 编译。

| 步骤 | 任务 | 关键实现要点 | 验证 |
|---|---|---|---|
| S0.1 | git init + 根文件 | `.gitignore`（target/dist/data/secrets）；README/LICENSE(SECURITY 基础)；`Cargo.toml` workspace + release profile + rust-toolchain | `cargo metadata` 解析成功 |
| S0.2 | 创建 12 个 crate 骨架 | 每个 crate 最小 `lib.rs`/`main.rs` + 空 `tests/`；`metria-cli` 用 clap 定义全部子命令（hub/agent/import/doctor/config/export/backup/restore/mcp/healthcheck/version） | `cargo build --workspace`；`metria version` 输出 0.1.0 |
| S0.3 | 配置与错误类型（core） | `metria-core`：`config.rs`（env + TOML 合并，`METRIA_*` 前缀，类型化 Config struct）；`error.rs`（`thiserror` 分层：ConfigError/ModelError/StorageError/ProtocolError/AdapterError/HubError，均实现 Into\<ApiError\>） | `cargo test -p metria-core`；config 单测（env 覆盖默认值） |
| S0.4 | tracing + 日志 | 统一 `tracing_subscriber` EnvFilter；Hub 输出 JSON 可选；Agent 输出紧凑文本；日志级别 `METRIA_LOG` 默认 `info`；**日志绝不打印 token/secret**（S0 起立规） | 运行 hub/agent --version 观察日志 |
| S0.5 | Docker 多阶段构建 | `docker/Dockerfile`：stage web(node:24-alpine 构建 vite dist) → stage rust(rust:1.96-slim 构建 workspace) → stage runtime(debian-slim，非 root UID 65532，仅 `/app/metria` 二进制 + 拷贝 web dist，`CMD ["healthcheck"]` 默认)；`ARG TARGETARCH` 支持 amd64/arm64；`.dockerignore` 排除 target/dist/node_modules/data | `docker build` 成功；`docker run --rm metria:dev version` 正常 |
| S0.6 | compose | `compose.yaml`(hub 示例)、`compose.agent.yaml`(agent 示例)、`compose.full.yaml`(hub+agent+demo 合成数据)；secret 走 `secrets:` 文件；healthcheck `["CMD","/app/metria","healthcheck"]`；agent 挂载 `/data` volume + 三目录只读；`user: "${UID}:${GID}"` 与宿主对齐并注释说明 | `docker compose -f compose.full.yaml config` 通过；`metria healthcheck` 子命令容器内可用 |
| S0.7 | migrations 框架（storage） | `metria-storage`：Migration 表 + 版本化 SQL 加载器（`migrations/*.sql`，命名 `N_name.sql`），事务内执行，记录 `schema_migrations`；`SqlitePool`（rusqlite 连接，busy_timeout=5000，WAL，foreign_keys=ON，`PRAGMA journal_size_limit`）；启动时 integrity_check（仅 quick_check） | 单测：空库→最新版本；重复迁移幂等；坏 SQL 回滚 |
| S0.8 | 基础 Repository 抽象（storage） | `trait Repository` 占位 + `SqliteRepository`；为后续 S1/S2 实体提供 CRUD 骨架（S1 填充）；metrics 计数（简单原子计数） | `cargo test -p metria-storage` |
| S0.9 | web 骨架 | `web/`：Vite+React；构建产物输出到 `web/dist`；`src/` 目录（api/components/pages/hooks/utils）；`index.html` 中文 title + PWA manifest 占位；light/dark 基础 token | `npm test && npm run build` 产出 dist |
| S0.10 | web embed 接线 | hub 增加 `StaticAssets`（`rust-embed` 指向 `web/dist`），SPA fallback 路由（未命中 API 则返回 index.html）；CI 脚本 `scripts/check.sh` 串联全部门禁 | `cargo build -p metria-hub` 包含 dist；访问 `/` 返回 HTML |

---

## 3. S1 — 统一领域模型 + Adapter

**DoD**：core 全模型带单测；3 个 Adapter 可发现/扫描/游标/健康，golden+malformed fixtures 全绿；`metria import` 与 `metria doctor` 可用。

### 3.1 领域模型（metria-core）

所有模型位于 `metria-core::model`，统一 `serde Serialize/Deserialize`，金额 i64 微美元，时间 `DateTime<Utc>`。

| 步骤 | 模型 | 字段要点（严格按 spec，缺失用 Option/null 不填 0） |
|---|---|---|
| S1.1 | 标识与归一化 | `Id`(ULID-like 自实现或 uuid)、`EventId`(blake3)、NodeId、CollectorId、SourceId、CanonicalKey；`normalize::model` / `normalize::provider`（如 claude-opus-4-6→claude-opus-4.6、o3-mini 等映射表，可扩展）；token 归一化（i64，非负校验） |
| S1.2 | Node/Collector | `Node`（含 labels/platform/arch/timezone/status/时间戳）；`Collector`（agent_version/protocol_version/container_image/heartbeat/upload/spool_pending/spool_size/clock_skew_seconds）；Node 身份：显式 METRIA_NODE_ID > 持久化 > 由 METRIA_NODE_NAME 生成 |
| S1.3 | Client/Source | `Client`（canonical_name/display_name/category）；`Source`（adapter_id/version/fingerprint/path_hash/client_version/status/capabilities/scan 时间戳/last_error）；path_hash = blake3(path)，默认不上传完整路径 |
| S1.4 | Project | `Project`（canonical_key/display_name/path_hash/git_remote_hash/metadata/时间戳）；git remote 默认 hash |
| S1.5 | Session | 全量 spec 字段（tokens 四类+reasoning、cost 三值、traffic 三项、confidence、parent_session_id 等）；status 枚举；content_available |
| S1.6 | Turn/Message | `Turn`（role/sequence/usage_source/granularity/confidence/finish_reason）；`Message`（content_type/content_hash/content_length/utf8_bytes/redacted，正文存储由 content_mode 控制） |
| S1.7 | ModelCall | 全量 spec 字段 + `call_granularity`（message/call/turn/session）+ streaming/stream_completed/client_aborted/retry_count + usage_event_id/traffic_estimate_id |
| S1.8 | UsageEvent | `event_id=blake3:`；全量 spec JSON（含 usage 四值可 null、cost 三值、quality 三件套）；不可变 |
| S1.9 | TrafficEstimate | request_payload/http/wire、response 同名、total/lower/upper、estimation_source(7 级)、context_transport_mode(4 级)、request/response_reconstruction_quality、profile_id/version、confidence |
| S1.10 | TrafficProfile（基础） | 全量 spec 字段（p50/p75/p90、fixed、两个 overhead ratio、cache transport factor×2、sample_count、confidence、effective_from/to、version、source 5 类、enabled）；M1 仅内置+adapter+user 静态，不做自动学习 |
| S1.11 | TrafficProfileSample / SourceCursor / SourceError | Sample（token_count/payload_bytes/bytes_per_token/reconstruction_quality）；Cursor 双形态（JSONL：path_hash/inode/size/mtime/offset/last_event_hash/last_scan_at；SQLite：fingerprint/schema_version/table/last_rowid/last_updated_at/last_pk/last_scan_at）；SourceError（phase/severity/pattern/样例计数） |
| S1.12 | Pricing 模型（基础） | `PricingCatalog/Snapshot/Rule/Match`（全量字段；金额微美元；priority/effective 区间；source 含 builtin_catalog/client_reported/user_override）；M1 实现 builtin + user_override 两源 |
| S1.13 | 脱敏（core） | `privacy::redact`：路径→blake3、git remote→hash、URL 中 token 摘除、Authorization 头替换、SSH 私钥/连接串关键词擦除；`privacy::content_mode` none/metadata/full 的字段白名单；本地先脱敏（Agent）+ Hub 二次脱敏（Hub） |
| S1.14 | 时间与内容分类 | `time::bucket`（hourly/daily，按 IANA 时区，chrono-tz）；`content::classify`（natural_language_zh/en/source_code/json/tool_schema/tool_result/terminal_output/log/markdown/xml/base64/mixed/unknown，启发式规则，纯本地，单测覆盖中文/英文/代码/JSON/base64）；`bytes::utf8_len` |
| S1.15 | 金额 | `money` 类型：i64 微美元 + 显式运算（mul_by_u64 用 i128 中间量，防溢出）；禁止 f64 累计；单测覆盖溢出/负数 |

### 3.2 Adapter API（metria-adapter-api）

| 步骤 | 任务 | 要点 |
|---|---|---|
| S1.16 | `SourceAdapter` trait + 类型 | `id/display_name/version/capabilities/discover/scan/health/traffic_capabilities`；`DiscoveryContext`(节点信息+root 路径列表+权限)；`DiscoveredSource`(canonical_path/path_hash/fingerprint/client_version)；`ScanBatch`(sessions/turns/messages/model_calls/usage_events/tool_events/subagent_relations/traffic_estimates/traffic_profile_samples/next_cursor/warnings/source_errors)；`AdapterCapabilities` 全量 bool；`TrafficCapabilities`(context_transport_detection/cache_behavior/reconstruction)；`AdapterError` 分类（PathNotFound/NotReadable/DbLocked/SchemaDrift/Malformed/PartialRead 等） |
| S1.17 | 解析健壮性基建 | 容忍未知字段（serde deny_unknown_fields=false + 自定义 parse 保底）、坏记录→warning+continue、单行长度上限、非 UTF-8 降级 read_raw、时间倒序纠正标记 |

### 3.3 三个 Adapter（各自独立 crate + fixtures）

| 步骤 | Adapter | 实现要点 | fixtures |
|---|---|---|---|
| S1.18 | Claude Code | 发现 `{root}/projects/*/*.jsonl`；解析 assistant/user/tool_use/tool_result/requestId 分组定 call 边界；usage 取自 assistant 消息 `message.usage`；声明 partial_reconstruction（无 system prompt/tool schema 时）、response 可完整重建；context_transport 判定 full_context；compact 事件识别；cursor 按 offset+inode | `fixtures/claude/`：完整 usage、缺失 usage、tool_call、subagent、compact、截断 JSON |
| S1.19 | Codex | 发现 `{root}/sessions/<id>/{session.json,rollout-*.jsonl,history.jsonl}`；按 rollout 记录识别 `previous_response_id`→判定 stateful_reference/mixed（无引用且 messages 全量→full_context）；usage 字段（input/output/cached_input/reasoning）；父子 session；call 边界=单条 response 记录 | `fixtures/codex/`：Responses 协议、ChatCompletions 旧协议、reasoning、cached_input、父子 session |
| S1.20 | OpenCode | 双布局探测：全局 `{root}/opencode.db`(新) 与 `{root}/project/<slug>/storage/**/*.db`(旧)；只读打开（READ_ONLY + busy_timeout=2000 + query_only，不改 PRAGMA、不 migration）；读 message/part/usage/cost/project；rowid 增量游标防全表扫描；WAL 存在时允许旧快照 | `fixtures/opencode/`：合成最小 schema db、缺 usage、schema drift 样本 |
| S1.21 | fixture 统一测试框架 | `metria-adapter-api` 提供 testutil：跑 golden（逐记录断言关键字段）+ malformed（断言不 panic、warning 数、skip 行为）+ schema drift（改名列/加列） | 全部 fixture 进入 git |
| S1.22 | `metria import` | CLI：`--source {claude,codex,opencode} --path <dir> --dry-run/--out ndjson`；复用 adapter scan 全量，输出归一化事件到文件或直接入 hub（S2 打通）；不修改源文件 | 用本机 `~/.claude`、`~/.codex` 做 `--dry-run` 冒烟（脱敏验证） |
| S1.23 | `metria doctor`（部分） | `--adapter <name>`：路径存在/可读/权限、cursor 状态、可发现 source 数、最近错误、adapter 版本、schema 兼容；`--traffic`：重建能力声明；输出结构化+退出码 | 对 fixtures 与真实目录运行 |

---

## 4. S2 — Agent 与 Hub 闭环

**DoD**：agent→hub 全链路集成测试通过（含断网补传、幂等重传、部分成功）；rollup 增量正确；`doctor --spool/hub/database` 可用；上传与隐私约束满足。

### 4.1 Agent（metria-agent，blocking 栈）

| 步骤 | 任务 | 要点 |
|---|---|---|
| S2.1 | Agent 配置 | `AgentConfig`：node_id/name、hub_url、token_file、三客户端路径、content_mode、spool 上限、批量参数、scan 间隔、reconcile=5min、heartbeat 间隔、backoff 初始值 |
| S2.2 | 本地 Spool | `data/spool.db`（rusqlite）：`source_cursors`/`pending_events`/`upload_batches`/`dead_letters`/`agent_metadata`/`source_health`/`traffic_profile_samples`；cursor+事件同一事务写入；`pending_events` 主键 event_id；满则停止 ingest + 告警（写入 agent_metadata.alert 且日志 ERROR），不静默丢弃 |
| S2.3 | 发现与扫描循环 | `DiscoveryLoop`（启动 + 周期 re-discover）；`ScanLoop`：notify 监听（debounce 500ms）→ 增量 scan → 归一化 → traffic 估算（S2.5）→ pricing（S2.6）→ 写 spool；文件事件按 JSONL 规则处理追加/截断/轮转/rename/inode 变化/半行；SQLite 按 rowid 增量；reconcile 每 5min 全量对账 |
| S2.4 | 上传器 | 指数退避+抖动；按 min(256 事件, 256KiB 压缩) 拆批；zstd 压缩；Batch 幂等（batch_id 唯一）；部分成功（按 event_id 重传失败子集）；Hub 确认才删 spool 事件；重启恢复续传 |
| S2.5 | 流量估算（基础版，metria-traffic） | 三源优先级：observed/reconstructed/content_bytes/token_profile/user_profile/unavailable；request_reconstruction（按 adapter 能力重建 JSON，缺隐藏内容→partial + 降 confidence）；response_reconstruction（assistant 可见内容优先）；token_profile 公式（uncached_input×input_bpt + cache_read×factor + cache_write×factor + fixed；output 用 visible_output_tokens=output−非传输 reasoning，仅 provider 语义明确时）；强制生成 lower/upper/confidence（禁止三者相同）；估算来源缺数据→unavailable，禁止硬造 |
| S2.6 | 价格计算（基础版，metria-pricing） | 优先级：reported_cost > user 精确 > user 通配 > builtin > unavailable；micro_usd 计算；M1 内置目录含常用模型（Claude/Codex/OpenAI o 系列），标注 source=builtin_catalog + channel；每事件保存 pricing_match 引用；三口径（reported/calculated/estimated）并存 |
| S2.7 | Agent 主循环 | `main`：加载 config → 节点身份 → 初始化 spool → 注册（S2.9）→ 启动发现/扫描/上传/心跳 → SIGTERM 优雅停止（刷 spool、断 notify、关连接） |

### 4.2 Hub（metria-hub）

| 步骤 | 任务 | 要点 |
|---|---|---|
| S2.8 | Hub 配置/启动 | `HubConfig`：listen/database_url/timezone/content_mode/collector token 源/admin 初始凭据 env/协议版本；启动序列：migrations → integrity → rollup 对账后台异步（不在启动路径，保证 ≤2s 启动）→ 价格内置目录初始化 → axum serve；健康检查端点 |
| S2.9 | Collector 协议 API | `POST /api/v1/collectors/register`、`heartbeat`、`events/batch`、`GET config`、`GET status`；认证=每 Collector 独立 token（DB 只存哈希，支持轮换/吊销、7 天有效期）；register 建 node+collector 并返回 token+node_id；heartbeat 更新 last_seen/clock_skew；协议版本协商。采集参数写死（Agent 默认扫描全部客户端），不做远端 config 下发 |
| S2.10 | Ingest 处理 | 解 zstd→校验（zip bomb 限制、JSON 深度≤32、单消息/单 tool output 字节上限、事件数≤256、event_id 唯一、node/collector 关系校验）→ 幂等（event_id unique 冲突跳过）→ 写入原始事件表 → 触发增量 rollup → 部分成功响应（成功/失败/重复三清单）；Batch 幂等（upload_batches） |
| S2.11 | Hub SQLite schema | 按 spec 四十四建表（nodes/collectors/collector_tokens/clients/sources/projects/sessions/turns/messages/model_calls/usage_events/tool_events/subagent_relations/traffic_estimates/traffic_profiles/traffic_profile_samples/pricing_catalogs/pricing_snapshots/pricing_rules/pricing_matches/hourly_rollups/daily_rollups/source_errors/share_links/share_audits/upload_batches/schema_migrations/users）+ 关键唯一约束；索引（事件表按 event_id/time/node；rollup 按 time_bucket+维度前缀）；WAL+定期 checkpoint+incremental_vacuum |
| S2.12 | Rollup 引擎 | hourly/daily 按 spec 四十五维度与统计字段；事件写入后增量更新；支持按范围重建与重算（幂等，upsert）；重复上传不重复统计（靠 event_id 幂等保证）；对账任务后台周期校验 rollup 与事件数 |
| S2.13 | Query API（子集） | 实现 M1 Web 需要的端点：`/api/v1/overview`、`/usage/timeseries`、`/usage/breakdown`、`/nodes`+`:id`（含 clients/sources/usage/traffic/sessions/calls）、`/clients`+`:id`、`/models`、`/calls`+`:id`、`/traffic/summary`、`/sessions`+`:id`+`/timeline`、`/pricing/rules`、`/data-quality`；全部支持 `from/to/timezone/granularity/allocation_mode(call_start 默认)` + 维度过滤 + cursor 分页；Overview 读 rollup，Detail 读原始 |
| S2.14 | Auth | `POST /auth/login`（argon2 校验）→ 签名会话 token（secret 来自 env）；`logout/me/change-password`；中间件保护 `/api/v1/**`（排除 login 与 stream 的兼容） |
| S2.15 | SSE | `GET /api/v1/stream`：事件推送（node/collector/source/session/call/usage/traffic/pricing 相关子集）；30s 心跳；事件过滤按当前用户可见范围 |
| S2.16 | 集成测试 | Rust 集成测试（metria-hub `tests/`）：mock agent→真实 hub（内存 SQLite）全链路：注册→扫描 fixture→spool→上传→rollup→API 断言；断网场景（hub 关闭→agent 事件积压→hub 恢复→补传成功→幂等验证）；重复上传统计不重复；部分成功重传；clock skew 检测 |
| S2.17 | Doctor 补全 | `--spool`（事件数/大小/告警/最近错误/死信）、`--hub`（连通性/TLS/时间差/最近上传）、`--database`（integrity/版本/rollup 对账） |

---

## 5. S3 — 精简 Web + Demo

**DoD**：浏览器（compose 起 hub）可登录并浏览全部 M1 页面；demo 模式生成确定性多客户端数据；SSE 增量刷新可用；light/dark 与移动端可用。

### 5.1 Web 架构

| 步骤 | 任务 | 要点 |
|---|---|---|
| S3.1 | 前端基建 | `web/src/api/client.ts`（fetch 封装+错误/空/加载态）、`hooks/useQuery.ts`、`hooks/useRange.ts`（任意时间范围：from/to/时区/granularity/allocation_mode，状态进 URL query）、`components/`（Card/Table/Empty/Loading/Error/Select/DateRangePicker/Segment/Tabs）、`lib/format.ts`（字节/金额/时间 IANA 格式化）、`lib/css-vars.ts`（light/dark 主题切换，localStorage 持久化） |
| S3.2 | 路由与布局 | 登录页 + 主布局（侧边导航：总览/Nodes/Agent 工具/模型/调用/流量/会话/价格/数据质量/设置）+ 顶部全局时间范围选择器 + 主题切换；登录态守卫；`/static/` 静态资源 + SPA fallback |
| S3.3 | Overview | 统计卡片（Input/Output/CacheRead/CacheWrite/Reasoning、三 cost、估算流量+上下界、Calls/Sessions/活跃 Node/Collector/AgentTool/Model/Project）+ uPlot 时间序列（Token/Cost/Traffic）+ Token 构成 + Node/Client/Model/Provider/Project 汇总 + 最近 Session + Collector 在线状态；读 rollup API；任意时间范围；hover/loading/empty/error |
| S3.4 | Nodes | 列表（spec 四十九字段，含 Detected Clients 徽标）+ Node Detail（基本信息/Collector/Client→Source 列表含 path_hash/状态/版本/扫描时间/错误/时间范围统计/按 Model·Client·Project 分布/最近 Sessions+Calls） |
| S3.5 | Agent Tools | 列表（spec 五十字段）+ Detail（出现 Node、Token/Cost/Traffic 分布、Models/Providers/Projects、最近 Sessions/Calls、Source 健康、版本分布、数据质量） |
| S3.6 | Models | 列表（spec 五十一字段，含 Bytes per Input/Output Token、Pricing Source）+ Detail（Token/Cost/Traffic 序列、Cache Hit、Pricing 规则、最近 Sessions/Calls） |
| S3.7 | Sessions / Calls | Sessions 列表（虚拟滚动+排序+范围筛选）+ Session Detail（摘要、Token Waterfall、Traffic Waterfall、Timeline、Tool Call 分析、Subagent 树、Model Switch、每次调用估算流量）；Calls 列表（spec 五十二字段+cursor 分页+排序）+ Call Detail（全字段含估算区间/来源/Context Transport/Cache Behavior/Profile/缺失说明）；禁止任何恢复/进入会话交互 |
| S3.8 | Traffic / Pricing / Data Quality | Traffic：统计卡片+按维度切换表（spec 五十三）+「估算流量≠网卡/账单」声明横幅；Pricing：规则列表+新增/编辑/停用/优先级/生效区间+规则测试（M1 基础）+内置目录查看；Data Quality：各 usage_source/traffic source/confidence 占比+解析失败+来源扫描/cursor/告警/clock skew |
| S3.9 | SSE 接入 | `useStream.ts`：订阅 `/api/v1/stream`，按事件类型 invalidate 对应 useQuery key（增量刷新，不整站刷新） |
| S3.10 | Demo 模式 | `metria hub --demo`：确定性 RNG（seeded）生成合成事件，走同一 ingest 路径：多节点/3 客户端/多模型/多 provider/多项目、session/tool/subagent/stateful reference/cache/reasoning、高/中/低可信流量、profile+区间、pricing rule；启动时禁用真实 ingest 冲突；不读真实目录、无真实用户信息 |
| S3.11 | 前端测试 | Vitest：时间范围组件、格式化、主题切换、空/加载/错误态、请求参数序列化；demo 数据下 API 冒烟（shell 脚本验证 overview/nodes 端点返回非空） |

---

## 6. M1 验收清单（对照 spec 六十九）

- ✅ = M1 完成
- ⬜ = 后续里程碑

| # | 验收项 | 状态 |
|---|---|---|
| 1–3 | Linux amd64/arm64 镜像构建（M1 验证 amd64，arm64 走 buildx 冒烟） | ✅ |
| 4–5 | Hub 容器 / Agent 容器 | ✅ |
| 6–8 | Compose / 非 Root / 只读挂载 | ✅ |
| 9–11 | Claude Code / Codex / OpenCode Adapter（含 fixture/golden/malformed） | ✅ |
| 12–17 | Node 注册 / 心跳 / Client 发现 / Spool / 断网补传 / 幂等上传 | ✅ |
| 18–21 | SQLite Hub / ModelCall / UsageEvent / TrafficEstimate | ✅ |
| 22 | TrafficProfile（基础） | ✅ |
| 23–27 | 请求估算 / 响应估算 / Session 流量 / 任意时间范围估算 / 上下界+Confidence | ✅ |
| 28–30 | Hourly Rollup / Daily Rollup / 任意时间范围查询 | ✅ |
| 31–40 | Overview / Nodes / Node Detail / Agent Tools / Models / Calls / Traffic / Sessions / Pricing(基础) / Traffic Profiles(基础) | ✅ |
| 41–42 | OpenRouter / LiteLLM 目录 | ✅ M3 |
| 43 | 用户自定义价格（基础） | ✅ |
| 44 | 用户自定义 Traffic Profile | ✅ M2 |
| 45–47 | Data Quality / Light / Dark | ✅ |
| 48–50 | 基础登录 / SSE / Demo | ✅ |
| 51 | 完整 README | ✅ |

---

## 7. 风险与应对

- **35MiB RSS**：blocking 栈 + 依赖精简（不用 reqwest/tokio），S2 后 `/proc` 实测，若超标在 S3 前调整依赖。
- **6GB opencode.db**：只读 + rowid 游标 + busy_timeout，绝对避免全表扫描；集成测试用小 db，真实库只做 doctor 冒烟。
- **非 Root 读 700 home**：compose 提供 UID/GID 对齐与文档；doctor 检测权限并给出明确错误。
- **JSONL 轮转/半行/非 UTF-8**：S1 已含 malformed fixture 覆盖，扫描器按行流式处理。

---

## 8. 执行顺序与提交粒度

S0 → S1 → S2 → S3，每步完成后跑 0.4 总门禁；每次提交前 `fmt+clippy+test`，提交信息遵循仓库 Commit 规范（≤50 字符 subject，不写 Change-Id）。

每个 S 阶段结束输出：完成内容 / 关键设计决策 / 新增修改文件 / 数据库变化 / API 变化 / Traffic 变化 / Pricing 变化 / 测试结果 / 当前限制 / 下一阶段。

---

## 9. 延迟趋势图与指标卡 hint 单行（2026-08-16）

- 使用分析「延迟」tab 在 P50/P95/P99/平均统计卡下方新增「延迟趋势」折线图，展示 P50/P95/平均时长随时间变化，与其他 tab 图表结构一致。
- 新增 `GET /api/v1/usage/latency/timeseries`：按时间分桶（5m/15m/1h/1d，粒度与 `/usage/timeseries` 一致）聚合 `model_calls.duration_ms` 的 avg/p50/p95/p99/count，空桶补齐且统计字段为 null（诚实展示）；分位数 nearest-rank 与 `/usage/latency` 一致，样本数与 `/usage/latency` 的 count 对齐。
- MetricCard hint 统一单行（`truncate` + `title` 悬停显示完整文案），修复「调用时长」卡片 hint 在 1280 视口换行成 2 行的问题。
- 新增 e2e 测试 `latency_timeseries_buckets_aggregates_and_gapfills`：分桶聚合、缺口补齐、空桶 null、与整体延迟样本数一致。
- 验证：`cargo test --workspace` 全部通过、fmt/clippy 干净、Web 构建通过；浏览器实测延迟 tab 图表渲染、时间范围同步、调用时长 hint 单行且 hover 可见完整文案。

---

## 10. 节点支持公网 IP（2026-08-16）

- 添加/编辑节点新增必填「节点 IP」字段（IPv4/IPv6/主机名格式校验，非法返回 400），解决 Hub 部署在内网、Agent 在公网时公网 Agent 无法连通内网 Hub 的问题。
- `nodes` 表新增 `ip` 列（迁移 009，可空兼容旧数据）；创建/更新/列表/详情均读写 ip。
- `GET /nodes/{id}/install` 生成安装命令的 hub URL 优先级：显式 `hub_url` → `http://{ip}:8080` → 占位；前端创建/编辑对话框同步展示 IP 输入并回显当前值。
- 新增/更新 e2e：创建含 ip 成功、缺 ip/非法 ip 400、安装命令 IP 拼接、显式 hub_url 优先、编辑 ip 生效。
- 验证：`cargo test --workspace` 全部通过、fmt/clippy 干净、Web 构建通过；浏览器实测创建节点填 IP → 安装命令 METRIA_HUB_URL=http://<ip>:8080，编辑回显 IP。

---

## 11. OIDC 单用户登录（2026-08-25）

- Hub 新增 OIDC Authorization Code Flow 登录（`api/oidc.rs`）：`GET /auth/oidc/status|login|callback`、
  `POST /auth/oidc/exchange`；discovery + code 换 token（client_secret_post 失败自动降级 basic）
  + userinfo 身份校验（不本地验签，要求 Issuer 走 HTTPS）；单用户白名单按 email（忽略大小写、
  要求 email_verified≠false）/ subject 匹配，白名单外账号明确拒绝。
- 回调向前端传递一次性交换码（60s 过期、单次使用），前端 POST exchange 换既有 HMAC 签名会话，
  token 不进 URL/浏览器历史；授权 state 一次性且 10 分钟过期。
- 配置：`METRIA_OIDC_ISSUER/CLIENT_ID/CLIENT_SECRET/ALLOWED_EMAIL/ALLOWED_SUBJECT/
  REDIRECT_URL/DISABLE_PASSWORD_LOGIN`；三元组不完整或缺白名单启动即报错；未配置时行为不变。
- 密码登录默认保留作后备，可禁用（login 返回 403）；OIDC 首登自动建无本地密码用户
  （占位 hash 非 PHC 格式不可用于密码校验）；system_info 增加 auth_mode。
- Web：登录页按 status 渲染「使用 OIDC 登录」按钮（密码禁用时隐藏表单）、回调落地页、
  设置页显示登录方式；错误经 `/#/login?error=` 回传展示。
- 测试：单元测试 5 个（白名单匹配/state 与交换码生命周期/redirect_uri 推导）+
  e2e 5 个（内置 mock IdP：status、全链路签发会话、重放拒绝、非白名单与伪造 state 拒绝、
  禁用密码登录、未配置 404）。

---

## 12. Hub 主动拉取（Pull）模式与 Agent 最小化配置（2026-08-25）

- OpenSpec change `hub-pull-agent-mode`（proposal/specs×3/design/tasks 全量落地）。
- Agent 双模式：配置 `METRIA_HUB_URL` 走 push（行为不变）；仅配置 token 进入 pull 模式——
  std TcpListener 手写极简 HTTP（零新增依赖），`/collect`（UploadBatch+zstd、X-Batch-Id、
  空批次 204）、`/ack`（幂等确认删 spool，未拉取 404）、`/status`（版本/来源健康/spool 统计），
  Bearer 常量时间比较；spool 批次状态机支持幂等重拉（pulled_batch_id 打标）。
- Hub Pull 调度器：按节点 agent_url 周期拉取（METRIA_PULL_INTERVAL 默认 60s），复用
  process_batch（自 ingest handler 抽取）校验/幂等/rollup，成功后 ack；单节点失败指数退避
  不影响他节点；节点级 token AES-256-GCM 加密存储（HKDF 派生自 session secret）。
- Web：添加/编辑节点「Agent 地址」输入与清除；节点详情 Pull 状态横幅；安装命令按模式生成
  （pull 形态仅 token + 挂载 + `-p 8090`）。
- migration 011：nodes 加 agent_url/node_token_enc/last_pull_at/last_pull_error。
- 测试：agent spool pull 状态机 2 例、pull server HTTP 集成 1 例（认证/幂等重拉/ack 语义）、
  e2e 全链路 1 例（创建节点→起 Agent→Hub 拉取入库→ack 清空 spool→401 校验）；全量 169 通过。
- RSS 实测：release 构建 pull 模式空闲 VmRSS ≈9MiB（≤35MiB 预算 ✓）。
- kvm2 真实环境验证（两台测试 VM 跨机拓扑）：Hub VM（ubuntu24，192.168.100.229，docker v0.4.0）
  + Agent VM（debian13，192.168.100.228，二进制 + systemd pull 模式）。验证通过：Hub 主动跨机
  拉取入库、增量会话自动同步（12s 内）、ack 后 spool 清空、Agent 瞬时不可达退避后自动恢复、
  节点在线状态与最近拉取时间/错误展示。运维注意：bind mount 目录需 chown 65532；VM 根磁盘
  打满会导致 SQLite 启动失败（disk full）；首次拉取即遇瞬时失败的退避起步偏长（60s×2^n），
  后续可优化为短起步（记录为改进项，不在本 change 范围）。

---

## 13. Agent 无状态轮询：游标外置 Hub（2026-09-09）

- OpenSpec change `stateless-polling-agent`（proposal/specs×3/design/tasks 全部落地并 Archive 前验证）。
- Push 模式重构为无状态轮询：每 `METRIA_POLL_INTERVAL`（默认 60s，5~86400 可配，安装命令注入）
  执行「拉 Hub 游标 → 增量扫描 → 分批直传（413 自动二分拆批）→ 全部确认后推游标」；
  不再使用 notify 与本地 spool；Hub 离线暂停扫描退避等待，恢复后按最新游标补齐（轮转文件除外）。
- 硬性顺序：事件确认（accepted/duplicate）在前、游标推进在后；失败不推进游标，下轮重扫靠
  event_id 幂等去重；fatal 拒绝记录 event_id+原因后跳过防活锁。
- Hub：migration 012 `source_cursors(collector_id, source_id)` + 013 `nodes.poll_interval_seconds`；
  `GET/POST /api/v1/collectors/cursors`（collector token 鉴权、绑定身份、幂等 upsert）；
  删除节点级联清游标；创建/编辑节点与 Docker/Linux/Windows 安装命令支持上报间隔。
- 遗留 spool 一次性迁移：先尽力清积压（Hub 确认）→ 再推本地游标 → 后删 spool 文件；
  失败保留现场下次重试。Pull 模式保持本地 spool 语义不变。
- 测试：stateless 单测 10 项（推进顺序/失败不推进/fatal/413 拆批/迁移成功与保留/配置边界）、
  Hub e2e 游标闭环（幂等/401/删节点清理）、stateless e2e（离线追加恢复补齐精确 +32000 token、
  重复扫描幂等）；全量门禁 fmt/clippy/test/web/docker/compose 通过。
- 真实环境验证（lstable Hub + aitools Agent）：Hub 迁移 [12,13] 应用；Agent 重建（补
  `--user 1000:1000` 修复卷属主 1000 与容器 65532 不匹配导致的 readonly 报错）后自动完成迁移
  （清积压 298 条入库、165 条 fatal 孤儿事件记录后跳过、推 166 条游标、删除 ~790MB spool）；
  轮询增量采集实测 298→328 条调用、零错误零重启；Hub 游标读回 166 条且 last_scan_at 随周期刷新；
  心跳 spool 统计 0/0；本地 UI 实例浏览器验证添加节点对话框上报间隔与安装命令
  `METRIA_POLL_INTERVAL=120`。
- 注意：本地镜像直发期间 ghcr dev-latest 滞后，需推送代码触发 CI 更新镜像，避免 watchtower
  每日 06:00 将节点回退到旧镜像。

---

## 14. 可观测性、设置完整性与报告邮件（2026-09-11）

- 完成设置页：SMTP 启用即时持久化、发件人校验、测试结果容器、全局右上角 Toast、调度字段对齐和几号 1–28 约束；节点动作同步使用 Toast。
- OIDC UserInfo `picture` 仅接受 HTTPS 并持久化，头像加载失败回退文字头像；OIDC 用户名和邮箱保持只读。
- 费用：ingest 即时计价、历史规则 match 保留、未定价覆盖率可见、全历史 rollup 重建；估算流量按 model call 时间归属并补齐历史当前版本。
- 性能：适配器记录可证明的首个可观察输出和完成时间；API/前端展示覆盖率，无法证明时保持不可用。
- 图表：输入/输出/缓存、模型、Agent 等数据文案统一 TrendChart 图例样式；报告邮件图表改为内嵌 SVG，PDF 保留 PNG。
- migration 015 增加 avatar/timing 字段并保留 pricing match 历史；lstable 完成全历史修复、备份、健康检查与明细/rollup 一致性核对。
