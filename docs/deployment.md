# 部署指南

## 1. 快速开始（Docker Compose）

```bash
cp docker/.env.example docker/.env
# 编辑 .env：METRIA_ADMIN_PASSWORD、CLAUDE_PATH/CODEX_PATH/OPENCODE_PATH、METRIA_COLLECTOR_TOKEN
docker compose -f docker/compose.full.yaml up -d
```

- `metria-hub`：Web + API + SQLite（`/data/metria.db`）
- `metria-demo`（profile: demo）：`hub --demo` 生成确定性合成数据
- `metria-agent`（profile: agent）：采集器，客户端目录只读挂载，数据写 `/data`

## 2. 镜像

- 多架构（amd64/arm64）。master 构建使用 `:master` 与 `:<sha>`；正式版本使用 `:<tag>`，并由 Release 工作流更新 `:latest`。
- 运行时**不含 Node.js**（前端产物 rust-embed 进 Hub 二进制）。
- 非 root（UID 65532）运行；`user: "${UID}:${GID}"` 与宿主对齐以读取 700 权限目录。

## 3. 配置（环境变量）

| 变量 | 默认 | 说明 |
|---|---|---|
| `METRIA_DATABASE_URL` | `sqlite:///data/metria.db` | Hub 数据库 |
| `METRIA_LISTEN` | `0.0.0.0:8080` | Hub 监听 |
| `METRIA_TIMEZONE` | `Asia/Shanghai` | 展示时区（存储恒为 UTC） |
| `METRIA_CONTENT_MODE` | `metadata` | none/metadata/full |
| `METRIA_SESSION_SECRET` | 无安全默认值 | 会话签名密钥，生产环境必须设置随机高强度值 |
| `METRIA_ADMIN_USER` / `METRIA_ADMIN_PASSWORD` | admin / change-me-please（Compose） | 初始 Admin 凭据，部署后立即修改 |
| `METRIA_COLLECTOR_TOKEN` | 无 | Collector 共享 bootstrap token |
| `METRIA_NODE_ID` / `METRIA_NODE_NAME` | 自动 | Agent 节点身份 |
| `METRIA_HUB_URL` | `http://localhost:8080` | Agent 连接 Hub |
| `METRIA_AGENT_TOKEN` / `METRIA_AGENT_TOKEN_FILE` | 无 | Agent 认证 token |
| `METRIA_CLAUDE_PATH` / `CODEX_PATH` / `OPENCODE_PATH` | 无 | 客户端目录 |
| `METRIA_SCAN_INTERVAL` / `RECONCILE_INTERVAL` / `HEARTBEAT_INTERVAL` / `UPLOAD_INTERVAL` | 10/300/60/15s | Agent 周期 |
| `METRIA_TOKEN_REFRESH_INTERVAL` | 6 天 | Agent 重新注册续期周期（< 7 天 token 有效期） |
| `METRIA_MAX_PENDING_EVENTS` / `MAX_SPOOL_BYTES` | 200 万 / 512MiB | Spool 上限 |
| `METRIA_LOG` | `info` | 日志级别 |

## 4. 网络与安全

- 只暴露 Hub 的 8080 端口；Agent 出站仅连 Hub，客户端目录只读挂载。
- Collector token 仅存哈希；过期后需重新注册（Agent 自动每 6 天续期）。
- 不在日志输出 token/secret；默认不上传完整路径/用户名/Hostname/Git Remote/密钥。
- `METRIA_SESSION_SECRET` 修改后已有 Web 会话会失效，需要重新登录。

## 5. 升级 / 回滚

见 `docs/operations.md`：

- 升级：拉新镜像 → 重启；启动时自动应用版本化迁移。
- 回滚：回退镜像 tag；若已应用新迁移，先恢复旧数据库（VACUUM INTO + zstd 备份）。

## 6. 健康检查与诊断

- `metria healthcheck`（容器内 CMD）：连通性 + 数据库 quick_check。
- `metria doctor --adapter|--traffic|--hub|--database|--spool`：环境诊断。

## 7. 前端添加节点与安装 Agent

Web 端「节点 → 添加节点」可预先创建节点并生成安装命令，目标机运行命令即接入（Beszel 式流程）：

1. **添加节点**：填写名称/描述/标签与 Hub 地址（默认当前页面 origin，目标机需可访问）。
2. **获取安装命令**：创建后自动展示一次性专属 Token 与两种安装命令（可切换、一键复制）。
3. **目标机安装**：
   - **Docker**：`docker run -d --name metria-agent ... ghcr.io/supercxyz/metria:latest agent`（命令已注入 node_id/token/hub_url 与客户端只读挂载）。
   - **原生**：从 `GET /api/v1/agent/download`（需 Admin 会话）下载当前 Hub 架构的 metria 二进制后后台运行；生产建议用 systemd 管理。若 Hub 与 Agent 架构不同，请从 GitHub Release 下载对应的 `metria-linux-amd64` 或 `metria-linux-arm64`，并用 `SHA256SUMS` 校验。
4. **接入确认**：Agent 用专属 token 注册后，节点变为「在线」，名称保持创建/编辑时设置的值。
5. **节点维护**：列表行支持「编辑」（改名称/描述/标签/Hub 地址）与「删除」（移除身份与令牌，历史用量数据保留）。

> 说明：专属 token 明文仅创建或生成安装命令时展示，Hub 只存哈希；Token 过期后可在节点详情重新生成安装命令（自动签发新 token，不吊销正在使用的旧 token）。

## 8. 规模与性能

- Hub 与 Agent 均 SQLite；rollup 增量更新，查询读汇总表。
- 后台维护：每 6h rollup 对账（发现漂移自动重建最近 24h）+ WAL checkpoint。
- 基准参考：10 万事件写入约 280ms，overview 查询约 20µs；1M 事件基准见 `docs/operations.md`。
