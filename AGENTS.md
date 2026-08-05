# Metria 项目规则（AGENTS.md）

本文件约束所有在该仓库中工作的自动化代理。全局规则见 `~/.config/opencode/AGENTS.md`，冲突时以本文件为准。

## 0. 开工前置

1. **每次会话开始必须先阅读 `docs/development-plan-m1.md`**，按计划步骤推进，不得跳步或自行扩展范围。
2. 按阶段推进，每阶段结束必须更新计划文档中的对应进度。
3. 仓库必须始终保持可构建状态；禁止提交无法编译的中间态。

## 1. 项目定位

Metria 是轻量、可自托管的 AI 编程 Agent 用量监控、费用分析和网络流量估算平台。统一采集 Claude Code / Codex / OpenCode 的 Token、调用次数、费用与估算流量，多节点汇总展示。零侵入采集，不修改节点、客户端或网络链路。

## 2. 术语（禁止混用）

- **Node**：运行 Metria Agent 容器的 Linux 宿主机。
- **Collector**：运行在 Node 上的 Agent 容器实例（CLI 用 `metria agent`，代码/协议/库中统一 `collector`）。
- **Client**：被监控的 AI 编程客户端（Web 显示「Agent 工具」），内部字段 `client`。
- **Source**：某 Node 上某 Client 的具体本地数据源（目录/JSONL/SQLite）。
- **Model Call**：一次可识别的模型调用，尽力关联 Node/Collector/Client/Source/Session/Turn/Provider/Model/Usage/Cost/TrafficEstimate。
- 禁止混淆 Node、Collector、Client 三个概念。

## 3. 零侵入硬性约束（最高优先级）

Agent 只能：
1. 通过**只读挂载**读取客户端已有的日志、会话文件和本地数据库；
2. 在自身持久化目录写 Cursor、Spool、身份和缓存；
3. 主动连接 Hub 上传采集结果；
4. 根据已有日志/Usage/消息内容估算流量。

禁止（违反即阻塞提交）：
- 修改任何客户端配置、API Base URL、API Key、添加自定义 Header；
- 安装或使用任何本地/透明/HTTP/HTTPS 代理、中间人、自签名 CA；
- 修改路由/DNS/nftables/iptables/TC、加载 eBPF、改内核参数/Cgroup；
- 挂载 Docker Socket、Host PID、Host Network、Privileged、CAP_NET_ADMIN、CAP_BPF；
- 抓取明文请求、拦截 TLS、修改客户端源码/补丁、注入动态库或环境变量改变客户端网络行为；
- 修改客户端日志、修改客户端数据库、对第三方数据库执行 Migration 或修改 PRAGMA。

## 4. 数据诚实性硬性规则

- 流量必须标记「**估算流量**」，禁止标记为实际/精确/网卡/账单流量。
- 缺失 Token 用 `null`，**禁止默认填 0**。
- 禁止把估算 Token 冒充 reported、calculated cost 冒充 reported cost、估算流量冒充实际流量。
- 禁止把 Session 级统计伪装成单次 Model Call；call_granularity 必须诚实标注。
- 费用三口径并存：reported_cost / calculated_cost / estimated_cost，各自可追溯。
- 禁止把 Cache Token 直接等同于网络字节；禁止把 Reasoning Token 全部换算为响应字节。
- 禁止用固定「1 Token = 4 Bytes」作为唯一估算算法；系数必须版本化。
- 禁止生成下界=中值=上界的估算区间；缺数据时标记 `unavailable`，不硬造。
- 外部价格目录必须保存来源与快照；OpenRouter 价格必须标记 channel，LiteLLM 必须提示为第三方数据。
- 价格更新 / Profile 更新不得覆盖历史快照与历史估算（重新计价/重新估算保留新旧版本）。
- 金额一律整数微美元（i64），禁止浮点累计。
- 时间存储 UTC，展示用 IANA 时区，禁止依赖容器系统时区。
- 默认不上传完整绝对路径、用户名、Hostname、Git Remote、环境变量、API Key、Authorization、Cookie、SSH 私钥、数据库连接串。
- 日志中禁止输出 Token 或 Secret。

## 5. 产品边界（禁止实现）

禁止实现：恢复/继续/进入历史会话、启动 Claude Code/Codex/OpenCode、重放请求、执行历史 Tool Call、代理或拦截模型 API、管理 API Key、执行代码、提供终端。

## 6. 技术约束

- 后端与 Agent 用 Rust stable；Web 用 Preact + TypeScript + Vite + uPlot。
- 不得强制依赖 Redis / Kafka / ClickHouse / Elasticsearch / Celery / 独立消息队列 / PostgreSQL。
- 不得使用 Electron / Next.js 生产运行时 / Nuxt / Angular / 重量级图表库。
- Hub 镜像运行时**不得包含 Node.js**（前端构建产物 rust-embed 进 Hub）。
- Agent 目标空闲 RSS ≤35MiB：使用无 tokio blocking 栈（notify + rusqlite + ureq/rustls + zstd + blake3），不得在 Agent 引入 reqwest/tokio 撑大内存。
- 每个 Adapter 独立 crate，禁止把所有客户端逻辑堆进一个文件。
- 单个 Rust 文件超约 800 行时检查按职责拆分。

## 7. 质量门禁（每阶段结束必须全绿，禁止跳过）

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cd web && npm run typecheck && npm run build
docker build -f docker/Dockerfile --target hub -t metria:dev .
docker compose -f docker/compose.full.yaml config
```

不得在没有验证证据的情况下声称「通过」「完成」「可提交」。

## 8. 测试要求

- 每个 Adapter 必须有 Golden Fixture 与 Malformed Fixture（截断 JSON/未知字段/非 UTF-8/超大行/重复事件/轮转/游标失效/锁/Schema Drift/时间倒序/负数溢出）。
- 解析器必须容忍坏记录：警告 + continue，不因单条坏记录中断。
- 每次扫描不得重新解析全部历史文件；JSONL 按 offset/inode，SQLite 按 rowid 增量。
- 默认每 5 分钟 Reconcile 补偿丢失的文件系统事件。
- 提交前必须运行 fmt、clippy 与相关测试。

## 9. 数据库

- Agent Spool 与 Hub 均 SQLite：WAL、foreign_keys、busy_timeout、定期 checkpoint。
- 每个数据库变化必须有版本化 Migration（`migrations/N_name.sql`）。
- SQLite 数据源只读访问；不长时间持有锁；大文件按行流式处理，禁止全表扫描。

## 10. 提交规范

- 遵循全局 Commit 规范：首行 subject ≤50 字符、祈使句、无 `feat:` 前缀（除非项目另有约定）、正文每行 ≤72 字符、不手写 Change-Id。
- 每次提交前检查 `git status` / `git diff`，只暂存本任务相关文件，禁止提交 secrets。
