# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 语义版本规范（[SemVer](https://semver.org/lang/zh-CN/)）。

## [Unreleased]

## [0.1.0] - 2026-08-05

### Added

- 领域模型（metria-core）：Node/Collector/Client/Source/Session/Turn/Message/ModelCall/UsageEvent/TrafficEstimate/TrafficProfile/Pricing/ToolEvent/SubagentRelation；事件 ID（blake3）；微美元金额；模型/Provider 归一化；脱敏；IANA 时间分桶；内容分类。
- 流量估算（metria-traffic）：版本化 Traffic Profile、重建/token-profile 估算、估算区间与置信度、stateful_reference/full_context 处理。
- Adapter 框架（metria-adapter-api）：SourceAdapter trait、ScanBatch、JSONL 流式解析、fixture 测试框架。
- Claude Code / Codex / OpenCode 三个 Adapter + golden/malformed fixtures。
- `metria import`：客户端目录 → 归一化 NDJSON。
- `metria doctor`：--adapter / --traffic / --hub 检查。
- 真机冒烟：Codex 真实目录导入 14876 次调用验证。

## [0.1.0] - 2026-08-05

### Added

- 线协议（metria-protocol）：注册/心跳/批传/状态/配置 + 上限校验。
- 价格引擎（metria-pricing）：内置目录 + 用户规则，reported > 用户 > builtin 优先级。
- Agent（metria-agent）：本地 Spool（幂等/断网积压/满则停止采集+告警）、notify 增量扫描 + 5 分钟 reconcile、zstd 批传 + 指数退避 + 部分成功、心跳、Node ID 优先级解析。
- Hub（metria-hub）：完整 SQLite schema（27 表）、认证中间件（admin/collector 分离）、幂等 ingest + 增量 hourly/daily rollup、查询 API 子集、SSE、e2e 集成测试。
- Web（Preact+TS+uPlot）：登录、总览、Nodes(+Detail)、Agent 工具、模型、会话(+Detail)、调用(+Detail)、流量、数据质量、时间范围选择器、Light/Dark、SSE。
- Demo 模式：`metria hub --demo` 确定性合成数据。
- `metria doctor` --spool/--database 补全。

## [0.1.0] - 2026-08-05

### Added

- Rust workspace：12 个 crate（core/protocol/storage/pricing/traffic/adapter-api/三个 adapter/agent/hub/cli）。
- `metria` CLI 骨架：hub/agent/import/doctor/config/export/backup/restore/mcp/healthcheck/version 子命令。
- metria-core：配置（ContentMode/timezone/env 解析）、分层错误类型、tracing 日志初始化。
- metria-storage：SQLite 打开与 PRAGMA（WAL/foreign_keys/busy_timeout）、版本化迁移框架（rust-embed 嵌入 `migrations/`）、Repository 抽象。
- metria-hub：axum 服务骨架（healthz + 前端静态资源 rust-embed + SPA fallback + 优雅退出）、迁移应用、容器 healthcheck。
- Web：Preact + TypeScript + Vite 骨架，light/dark 主题 CSS Variables，PWA manifest。
- Docker：多阶段构建（Node 构建期 / Rust 构建期 / 非 root 运行时，运行时无 Node.js）。
- Docker Compose：hub 单服务、agent 单服务、hub+agent+demo 完整示例。
- 质量门禁脚本 `scripts/check.sh`。
