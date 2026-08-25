# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 语义版本规范（[SemVer](https://semver.org/lang/zh-CN/)）。

## [0.3.2] - 2026-08-25

### Added

- OIDC 白名单拒绝时记录 IdP 实际返回的 email（含验证状态）与 subject 前缀，便于排查 IdP 账号/claim 配置不一致。

## [0.3.1] - 2026-08-25

### Changed

- OIDC 默认行为变更：配置启用 OIDC 后自动禁用本地密码登录（仅保留 OIDC 入口）；如需 IdP 故障应急后备，显式设置 `METRIA_OIDC_DISABLE_PASSWORD_LOGIN=false`。
- `/api/v1/auth/me` 新增 `local_password` 字段；OIDC 账号（无本地密码）在设置页隐藏「修改密码」并显示登录方式说明。

## [0.3.0] - 2026-08-25

### Added

- Hub 支持 OIDC 单用户登录（Authorization Code Flow）：discovery、code 换 token、userinfo 身份校验；按 email / subject 白名单匹配唯一允许账号，白名单外明确拒绝。
- OIDC 回调经一次性交换码换取会话 token（不经过 URL）；授权 state 一次性且 10 分钟过期。
- Web 登录页「使用 OIDC 登录」入口与回调落地页；设置页显示当前登录方式。
- 可配置禁用本地密码登录，仅保留 OIDC 入口。

### Changed

- `GET /api/v1/system/info` 新增 `auth_mode` 字段（password / oidc / oidc+password）。
- OIDC 相关环境变量：`METRIA_OIDC_ISSUER` / `CLIENT_ID` / `CLIENT_SECRET` / `ALLOWED_EMAIL` / `ALLOWED_SUBJECT` / `REDIRECT_URL` / `DISABLE_PASSWORD_LOGIN`。

## [0.2.0] - 2026-08-24

### Added

- M2 Traffic：Profile 自动学习、用户/学习 Profile Web 管理、历史重新估算。
- M3 Pricing：OpenRouter / LiteLLM / Custom HTTP 价格目录同步（ETag/快照/失败保留）、重新计价。
- M6 分享 / 导出（JSON/NDJSON/CSV）/ MCP 只读查询 / 备份恢复。
- M7 性能基准（10 万/100 万事件）与运维文档（保留策略/备份/升级/回滚）。

### Changed

- CI 的开发镜像改用 `master` 与 commit 标签，不再覆盖 `latest`。
- Release 校验 Tag 与 workspace 版本，并分别发布 amd64/arm64 二进制。
- Agent 保持只读采集，不包含代理、客户端配置改写或请求拦截能力。
- Agent 原生发布增加 Windows amd64，节点安装命令支持 Linux/Windows 平台与动态 Hub 地址。

### Fixed

- 修复 GitHub Release 两个同名二进制资产冲突。
- 修复文档中的技术栈、默认凭据、仓库地址与质量门禁描述不一致。
- 修复 Agent 二进制下载命令的认证和架构选择问题，下载接口改为公开节点专属地址。

## [0.1.0] - 2026-08-05

### Added

- 领域模型（metria-core）：Node/Collector/Client/Source/Session/Turn/Message/ModelCall/UsageEvent/TrafficEstimate/TrafficProfile/Pricing/ToolEvent/SubagentRelation；事件 ID（blake3）；微美元金额；模型/Provider 归一化；脱敏；IANA 时间分桶；内容分类。
- 流量估算（metria-traffic）：版本化 Traffic Profile、重建/token-profile 估算、估算区间与置信度、stateful_reference/full_context 处理。
- Adapter 框架（metria-adapter-api）：SourceAdapter trait、ScanBatch、JSONL 流式解析、fixture 测试框架。
- Claude Code / Codex / OpenCode 三个 Adapter + golden/malformed fixtures。
- `metria import`：客户端目录 → 归一化 NDJSON。
- `metria doctor`：--adapter / --traffic / --hub 检查。
- 真机冒烟：Codex 真实目录导入 14876 次调用验证。

- 线协议（metria-protocol）：注册/心跳/批传/状态/配置 + 上限校验。
- 价格引擎（metria-pricing）：内置目录 + 用户规则，reported > 用户 > builtin 优先级。
- Agent（metria-agent）：本地 Spool（幂等/断网积压/满则停止采集+告警）、notify 增量扫描 + 5 分钟 reconcile、zstd 批传 + 指数退避 + 部分成功、心跳、Node ID 优先级解析。
- Hub（metria-hub）：完整 SQLite schema（27 表）、认证中间件（admin/collector 分离）、幂等 ingest + 增量 hourly/daily rollup、查询 API 子集、SSE、e2e 集成测试。
- Web（React+Vite+Tailwind+Chart.js）：登录、总览、Nodes(+Detail)、Agent 工具、模型、会话(+Detail)、调用(+Detail)、流量、数据质量、时间范围选择器、Light/Dark、SSE。
- Demo 模式：`metria hub --demo` 确定性合成数据。
- `metria doctor` --spool/--database 补全。

- Rust workspace：12 个 crate（core/protocol/storage/pricing/traffic/adapter-api/三个 adapter/agent/hub/cli）。
- `metria` CLI 骨架：hub/agent/import/doctor/config/export/backup/restore/mcp/healthcheck/version 子命令。
- metria-core：配置（ContentMode/timezone/env 解析）、分层错误类型、tracing 日志初始化。
- metria-storage：SQLite 打开与 PRAGMA（WAL/foreign_keys/busy_timeout）、版本化迁移框架（rust-embed 嵌入 `migrations/`）、Repository 抽象。
- metria-hub：axum 服务骨架（healthz + 前端静态资源 rust-embed + SPA fallback + 优雅退出）、迁移应用、容器 healthcheck。
- Web：React + Vite + Tailwind + Chart.js，light/dark 主题与 PWA manifest。
- Docker：多阶段构建（Node 构建期 / Rust 构建期 / 非 root 运行时，运行时无 Node.js）。
- Docker Compose：hub 单服务、agent 单服务、hub+agent+demo 完整示例。
- 质量门禁脚本 `scripts/check.sh`。
