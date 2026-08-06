# Metria 架构

## 1. 定位

Metria 是轻量、可自托管的 AI 编程 Agent 用量监控 / 费用分析 / 网络流量估算平台。
统一采集 Claude Code / Codex / OpenCode 的 Token、调用次数、费用与估算流量，多节点汇总展示。
**零侵入采集**：不修改任何客户端配置、网络链路或数据源，仅通过只读挂载读取客户端已有日志。

## 2. 核心概念

| 术语 | 说明 |
|---|---|
| Node | 运行 Metria Agent 容器的 Linux 宿主机 |
| Collector | 运行在 Node 上的 Agent 容器实例 |
| Client | 被监控的 AI 编程客户端（Claude Code / Codex / OpenCode） |
| Source | 某 Node 上某 Client 的具体本地数据源（JSONL 文件 / SQLite 库） |
| Model Call | 一次可识别的模型调用，关联 Node/Client/Source/Session/Provider/Model/Usage/Cost/Traffic |

## 3. 架构分层

```
┌─────────────────────────────────────────────────────────┐
│  Web（Preact + TS + uPlot）  ← rust-embed 进 Hub 镜像   │
├─────────────────────────────────────────────────────────┤
│  Hub（axum + tokio）                                     │
│  ├─ 认证（单 Admin 会话） / Collector 协议（token）      │
│  ├─ Ingest：zstd 解压 → 校验 → 幂等落库 → 增量 rollup    │
│  ├─ 查询 API（overview/timeseries/breakdown/明细）       │
│  ├─ 后台：价格目录同步 / rollup 对账 / WAL checkpoint    │
│  └─ SQLite（WAL，28 表，版本化迁移）                     │
├─────────────────────────────────────────────────────────┤
│  Agent（无 tokio blocking 栈，RSS ≤35MiB 目标）          │
│  ├─ 发现/扫描：notify 监听 + 增量扫描 + 5min reconcile   │
│  ├─ 估算：traffic（7 级来源）/ pricing（多来源优先级）   │
│  ├─ Spool：事件/游标/批次/死信，事务一致                 │
│  └─ 上传：zstd 批传 + 指数退避 + 幂等（event_id）        │
├─────────────────────────────────────────────────────────┤
│  Adapter（每客户端独立 crate，只读）                     │
│  ├─ claude-code：projects/*.jsonl（modern entry）        │
│  ├─ codex：sessions/*/rollout-*.jsonl                    │
│  └─ opencode：全局 opencode.db / project storage（只读） │
└─────────────────────────────────────────────────────────┘
```

## 4. 数据流

1. **采集**：Adapter 通过只读挂载扫描客户端日志，归一化为统一事件
   （session / source / call / usage / traffic / tool / subagent / traffic_sample）。
2. **估算**：Agent 本地做 traffic 估算（reconstructed/partial/content_bytes/token_profile）与
   pricing 计算（reported > user > catalog > builtin）。
3. **Spool**：事件 + 游标同事务写入本地 SQLite；满则停止采集并告警，不丢数据。
4. **上传**：按 min(256 事件, 256KiB 压缩) 拆批，zstd 压缩，幂等（event_id 去重），
   部分成功按 event_id 重传失败子集。
5. **落库**：Hub 校验（schema/深度/大小）→ 幂等 upsert → 增量 rollup（hourly/daily）。
6. **展示**：查询 API 读 rollup（概览）与原始表（明细）；Web 通过 SSE 增量刷新。

## 5. 关键设计决策

| 决策 | 理由 |
|---|---|
| 金额用 i64 微美元 | 禁止浮点累计误差 |
| 时间存 UTC，展示用 IANA | 不依赖容器系统时区 |
| Agent 用 blocking 栈（无 tokio） | 满足空闲 RSS ≤35MiB 目标 |
| 流量标记「估算流量」 | 数据诚实性硬性规则，不冒充实际/网卡流量 |
| 缺失 Token 用 `null` 不填 0 | 数据诚实性 |
| 会话引用统一规范键 `node:source_session_id` | 跨表 join 与幂等 |
| 版本化迁移（`migrations/N_name.sql`） | 数据库升级可控、可回滚 |

## 6. 运行栈

- **Agent**：notify + rusqlite + ureq/rustls + zstd + blake3（无 tokio/reqwest）
- **Hub**：tokio + axum + rusqlite(blocking pool) + rust-embed + SSE
- **Web**：Preact + TypeScript + Vite + uPlot，rust-embed 进 Hub 二进制
- **依赖约束**：不强制 Redis / Kafka / ClickHouse / PostgreSQL；Hub 镜像不含 Node.js

## 7. 质量与安全

- 门禁：`cargo fmt` / `clippy -D warnings` / `test` / web typecheck+build / docker build / compose config
- 零侵入硬性约束（详见 README / AGENTS.md §3）：禁止代理、中间人、改网络、挂 Docker Socket 等
- 隐私：默认 content_mode=metadata，Agent 本地脱敏 + Hub 二次脱敏；
  不上传完整路径、用户名、Hostname、Git Remote、API Key、Authorization、SSH 私钥
