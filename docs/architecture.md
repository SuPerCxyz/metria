# Metria 架构

## 1. 定位

Metria 是轻量、可自托管的 AI 编程 Agent 用量监控、费用分析与模型性能观测平台。
统一采集 Claude Code / Codex / OpenCode 的 Token、调用次数、费用和可证明的运行时性能，多节点汇总展示。
普通模式只读挂载读取客户端已有日志；原生 Agent 另提供显式、临时的运行时观测入口。

## 2. 核心概念

| 术语 | 说明 |
|---|---|
| Node | 运行 Metria Agent 容器的 Linux 宿主机 |
| Collector | 运行在 Node 上的 Agent 容器实例 |
| Client | 被监控的 AI 编程客户端（Claude Code / Codex / OpenCode） |
| Source | 某 Node 上某 Client 的具体本地数据源（JSONL 文件 / SQLite 库） |
| Model Call | 一次可识别的模型调用，关联 Node/Client/Source/Session/Provider/Model/Usage/Cost/Runtime observation |

## 3. 架构分层

| 层级 | 组件 | 主要职责 |
|---|---|---|
| Web | React 19 + Vite + Tailwind CSS + Chart.js | Web UI，构建产物由 rust-embed 嵌入 Hub |
| Hub | axum + tokio + SQLite | 认证、Ingest 校验、幂等落库、Rollup、查询 API 和报告调度 |
| Agent | blocking 采集栈 | Push/Pull 普通采集、增量游标、费用计算和原生临时运行时观测 |
| Adapter | Claude Code / Codex / OpenCode 独立 crate | 只读发现和解析各客户端数据源 |
| 报告 | SMTP + JSON Webhook | 日 / 周 / 月聚合、HTML/纯文本渲染、渠道结果记录 |

Hub 数据库使用 SQLite WAL 和版本化迁移，目前包含 32 张表；Agent 的 Push 模式将游标保存在 Hub，Pull 模式使用本地 spool。

## 4. 数据流

1. **普通采集**：Adapter 通过只读挂载扫描客户端日志/SQLite，归一化为 session / source /
   call / usage / tool / subagent 事件；Docker 与普通原生 Agent 不拦截请求，也不生成估算流量。
2. **运行时观测（原生显式入口）**：`metria observe` 为单个客户端进程设置进程级本地
   base URL，转发期间在有界内存中提取 TTFT、Token/s、可靠性、路由和观测字节，原文不落盘。
3. **上传（Push 无状态轮询）**：按周期「拉 Hub 游标 → 增量扫描 → 直传 → 确认后推游标」；
   事件确认在前、游标推进在后，失败不推进、下轮重扫（event_id 幂等去重）。
   Hub 离线时暂停扫描，恢复后按最新游标补齐。Pull 模式：事件 + 游标写本地 SQLite
   spool，Hub 拉取确认后删除；满则停止采集并告警。
4. **落库**：Hub 校验（schema/深度/大小）→ 幂等 upsert → 增量 rollup（hourly/daily）。
5. **展示**：查询 API 读 rollup（概览）与原始表（明细）；Web 通过 SSE 增量刷新。
6. **报告**：调度器按全局 IANA 时区聚合完整周期，渲染 HTML/纯文本和 JSON，投递到 SMTP 或 Webhook，并记录每个渠道的结果。

## 5. 关键设计决策

| 决策 | 理由 |
|---|---|
| 金额用 i64 微美元 | 禁止浮点累计误差 |
| 时间存 UTC，展示用 IANA | 不依赖容器系统时区 |
| Agent 用 blocking 栈（无 tokio） | 满足空闲 RSS ≤35MiB 目标 |
| 观测字节单独命名 | 区分 payload/wire 与历史估算流量，不冒充实际网卡/账单流量 |
| 缺失 Token 用 `null` 不填 0 | 数据诚实性 |
| 会话引用统一规范键 `node:source_session_id` | 跨表 join 与幂等 |
| 版本化迁移（`migrations/N_name.sql`） | 数据库升级可控、可回滚 |

## 6. 运行栈

- **Agent**：notify + rusqlite + ureq/rustls + zstd + blake3（无 tokio/reqwest）
- **Hub**：tokio + axum + rusqlite(blocking pool) + rust-embed + SSE + SMTP/Webhook 报告调度
- **Web**：React 19 + Vite + Tailwind CSS + Chart.js，rust-embed 进 Hub 二进制
- **依赖约束**：不强制 Redis / Kafka / ClickHouse / PostgreSQL；Hub 镜像不含 Node.js

## 7. 质量与安全

- 门禁：`cargo fmt` / `clippy -D warnings` / `test` / web test+build / docker build / compose config
- 零侵入硬性约束（详见 README / AGENTS.md §3）：禁止代理、中间人、改网络、挂 Docker Socket 等
- 隐私：默认 content_mode=metadata，Agent 本地脱敏 + Hub 二次脱敏；
  不上传完整路径、用户名、Hostname、Git Remote、API Key、Authorization、SSH 私钥
