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

- 多架构（amd64/arm64），`ghcr.io/supercxyz/metria:latest` 与 `:<sha>`。
- 运行时**不含 Node.js**（前端产物 rust-embed 进 Hub 二进制）。
- 非 root（UID 65532）运行；`user: "${UID}:${GID}"` 与宿主对齐以读取 700 权限目录。

## 3. 配置（环境变量）

| 变量 | 默认 | 说明 |
|---|---|---|
| `METRIA_DATABASE_URL` | `sqlite:///data/metria.db` | Hub 数据库 |
| `METRIA_LISTEN` | `0.0.0.0:8080` | Hub 监听 |
| `METRIA_TIMEZONE` | `Asia/Shanghai` | 展示时区（存储恒为 UTC） |
| `METRIA_CONTENT_MODE` | `metadata` | none/metadata/full |
| `METRIA_ADMIN_USER` / `METRIA_ADMIN_PASSWORD` | admin / metria-admin | 初始 Admin 凭据（务必修改） |
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

## 5. 升级 / 回滚

见 `docs/operations.md`：

- 升级：拉新镜像 → 重启；启动时自动应用版本化迁移。
- 回滚：回退镜像 tag；若已应用新迁移，先恢复旧数据库（VACUUM INTO + zstd 备份）。

## 6. 健康检查与诊断

- `metria healthcheck`（容器内 CMD）：连通性 + 数据库 quick_check。
- `metria doctor --adapter|--traffic|--hub|--database|--spool`：环境诊断。

## 7. 规模与性能

- Hub 与 Agent 均 SQLite；rollup 增量更新，查询读汇总表。
- 后台维护：每 6h rollup 对账（发现漂移自动重建最近 24h）+ WAL checkpoint。
- 基准参考：10 万事件写入约 280ms，overview 查询约 20µs；1M 事件基准见 `docs/operations.md`。
