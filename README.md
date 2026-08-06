# Metria

**Metria** 是一个轻量、可自托管的 AI 编程 Agent 用量监控、费用分析和网络流量估算平台。

统一采集 Claude Code / Codex / OpenCode 的 Token、调用次数、费用与估算流量，多节点汇总展示。**零侵入采集**：不修改节点、客户端或网络链路。

> 状态：**开发中（M1）**。当前为工程骨架阶段，核心闭环（Adapter + Agent + Hub + Rollup）正在实现。

## 特性

- 统一模型：Node / Collector / Client / Source / Session / Turn / Message / Model Call / Usage Event / Traffic Estimate / Traffic Profile / Pricing。
- 零侵入：只读挂载读取客户端已有日志与本地数据库，不修改任何配置，不使用代理，不注入 eBPF。
- 诚实的数据：Token 缺失用 `null` 而非 0；费用三口径（reported / calculated / estimated）并存；流量一律标记「估算流量」并给出范围与可信度。
- 多节点汇总：每个 Node 下展示检测到的 Client 与 Source。
- 任意时间范围：所有统计支持明确的 `from/to` + IANA 时区 + 时间粒度。
- 轻量部署：单一二进制镜像（Hub 运行时无 Node.js），SQLite 存储，Docker Compose 一键起。

## 快速开始

前置：Linux amd64/arm64 + Docker + Docker Compose。

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

## 容器

```bash
# 构建
docker build -f docker/Dockerfile --target hub -t metria:dev .
# 多架构
docker buildx build --platform linux/amd64,linux/arm64 --target hub -t ghcr.io/supercxyz/metria:0.1.0 --push .

# 健康检查
docker run --rm metria:dev healthcheck
```

镜像内同一二进制多命令入口：

```
metria hub          # Hub 服务
metria agent        # Agent（Collector）
metria import       # 客户端目录导入
metria doctor       # 环境诊断
metria healthcheck  # 容器健康检查
metria version      # 版本信息
```

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
web/               Preact + TypeScript + Vite 前端
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
