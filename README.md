<div align="center">

<img src="docs/logo/metria-full.png" alt="Metria" width="500" />

# Metria

**轻量、可自托管的 AI 编程 Agent 用量监控 · 费用分析 · 网络流量估算**

统一采集 **Claude Code / Codex / OpenCode** 的 Token、调用次数、费用与估算流量，多节点汇总展示。**零侵入采集**，不修改节点、客户端或网络链路。

[![CI](https://github.com/SuPerCxyz/metria/actions/workflows/ci.yml/badge.svg)](https://github.com/SuPerCxyz/metria/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

</div>

---

## 为什么用 Metria

AI 编程 Agent（Claude Code / Codex / OpenCode）的 Token、费用和网络流量分散在各自本地的日志与数据库中，难以统一查看、汇总与核算。Metria 通过**只读挂载**读取客户端已有的数据源，零侵入地完成：

- **用量监控**：Token（输入/输出/缓存/推理）、调用次数、会话数，按 Node / Collector / Client / Model / 任意时间范围聚合。
- **费用分析**：三口径并存（`reported` / `calculated` / `estimated`），结合版本化价格目录与重新计价，费用可追溯。
- **流量估算**：基于消息内容与 Usage 的**估算流量**（含上下界与置信度），明确标注「估算」，绝不以估算冒充实际。

## 特性

- **零侵入（硬约束）**：只读挂载读取日志/会话/本地数据库；不修改客户端配置，不使用代理，不注入 eBPF，不抓取明文请求。
- **诚实数据**：Token 缺失用 `null` 而非 0；费用三口径并存；流量一律标记「估算流量」并给出范围与可信度。
- **多节点汇总**：每个 Node 下展示检测到的 Client 与 Source，跨节点统一视图。
- **任意时间范围**：所有统计支持 `from/to` + IANA 时区 + 自适应时间粒度。
- **轻量部署**：单一二进制镜像（Hub 运行时无 Node.js），SQLite 存储，Docker Compose 一键起。
- **增量采集**：JSONL 按 offset/inode、SQLite 按 rowid 增量扫描，不重复解析历史，每 5 分钟 Reconcile 补偿丢失事件。

## 快速开始

前置：Hub 使用 Linux amd64/arm64 + Docker + Docker Compose；Agent 原生运行支持 Linux amd64/arm64 与 Windows amd64。

```bash
# 1. 准备环境变量（含 Admin 初始密码与可选客户端目录）
cp docker/.env.example docker/.env
vi docker/.env

# 2. 启动 Hub（Web 默认 http://localhost:8080）
docker compose -f docker/compose.yaml up -d

# 3. 启动 Agent（可选，需先配置客户端目录挂载与 Collector Token）
docker compose -f docker/compose.agent.yaml up -d
```

`compose.full.yaml` 额外包含 Demo 模式（`hub --demo`，合成数据，不读真实目录）。

> 也可直接跑 `metria hub --demo` 体验界面，无需真实客户端数据。

### 在 Web 端添加节点（推荐）

节点页点击「添加节点」，填写名称、平台、架构与 Hub 地址后，页面生成一次性专属 Token 与 **Docker / 原生** 安装命令；二进制公开下载，Token 仅用于 Agent 注册和上传。节点信息可在 Web 端编辑/删除，历史用量数据保留。详见 `docs/deployment.md`。

## 容器

```bash
# 构建
docker build -f docker/Dockerfile --target hub -t metria:dev .

# 多架构（amd64 + arm64），VERSION 替换为实际 tag（如 v0.2.0）
VERSION=v0.2.0
docker buildx build --platform linux/amd64,linux/arm64 --target hub \
  -t "ghcr.io/SuPerCxyz/metria:$VERSION" --push .

# 健康检查
docker run --rm metria:dev healthcheck
```

镜像内同一二进制多命令入口：

```
metria hub          # Hub 服务（Web UI + API）
metria agent        # Agent（Collector）
metria import       # 客户端目录导入
metria doctor       # 环境诊断
metria healthcheck  # 容器健康检查
metria version      # 版本信息
```

## 架构概览

```
┌─────────────────────────────────────────────────────────────┐
│  Client 数据源（只读挂载）                                     │
│  ~/.claude ~/.codex ~/.local/share/opencode ...              │
└──────────────────────────┬──────────────────────────────────┘
                           │ 增量扫描（JSONL offset / SQLite rowid）
┌──────────────────────────▼──────────────────────────────────┐
│  Agent（Collector，每个 Node 一个）                           │
│  解析事件 → 归一化 → Spool 缓冲 → 批传上传                     │
└──────────────────────────┬──────────────────────────────────┘
                           │ HTTPS batch（注册/心跳/事件/状态）
┌──────────────────────────▼──────────────────────────────────┐
│  Hub                                                         │
│  SQLite（WAL）· Rollup · 价格目录 · 流量 Profile · Web UI     │
└─────────────────────────────────────────────────────────────┘
```

核心概念：[Node / Collector / Client / Source](docs/data-model.md)。术语约定见 [AGENTS.md](AGENTS.md) 第 2 节。

## 开发

前置：Rust stable（本机 1.96+）、Node 24+、Docker。

```bash
# 质量门禁（每阶段必须全绿）
bash scripts/check.sh

# Rust 测试
cargo test --workspace

# 本地跑 Hub（先构建 web）
cd web && npm install && npm run build && cd ..
METRIA_DATA_DIR=/tmp/metria-dev METRIA_DATABASE_URL=sqlite:///tmp/metria-dev/h.db \
  cargo run -p metria-cli -- hub
```

## 仓库结构

```
crates/            Rust workspace（12 个 crate）
├─ metria-core       领域模型、脱敏、时间分桶
├─ metria-protocol   线协议（注册/批传/状态）
├─ metria-storage    SQLite 存储 + 迁移
├─ metria-pricing    价格目录与计价
├─ metria-traffic    流量估算 Profile 与重建
├─ metria-adapter-*  Claude Code / Codex / OpenCode Adapter
├─ metria-agent      采集 Agent（Collector）
├─ metria-hub        Hub（API + Rollup + 维护）
└─ metria-cli        多命令 CLI 入口
web/               React + Vite + Tailwind + Chart.js 前端
docker/            Dockerfile 与 Compose 示例
migrations/        SQLite 版本化迁移
fixtures/          Adapter 测试夹具（golden / malformed）
docs/              架构、数据模型、协议、部署等文档
scripts/           构建与门禁脚本
```

## 文档

- [架构](docs/architecture.md)
- [数据模型](docs/data-model.md)
- [Adapter](docs/adapters.md)
- [API](docs/api.md)
- [部署](docs/deployment.md)
- [隐私与数据诚实性](docs/privacy.md)
- [运维手册](docs/operations.md)
- [开发指南](docs/development.md)
- [开发计划](docs/development-plan-m1.md)

## 许可

[Apache-2.0](LICENSE)
