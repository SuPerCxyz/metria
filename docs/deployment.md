# 部署指南

## 1. 快速开始（Docker Compose）

```bash
cp docker/.env.example docker/.env
# 编辑 .env：METRIA_ADMIN_PASSWORD、CLAUDE_PATH/CODEX_PATH/OPENCODE_PATH、METRIA_COLLECTOR_TOKEN
docker compose -f docker/compose.full.yaml --profile demo --profile agent up -d
```

- `metria-hub`：Web + API + SQLite（`/data/metria.db`）
- `metria-demo`（profile: demo）：`hub --demo` 生成确定性合成数据；不需要 Demo 时可不启用该 profile
- `metria-agent`（profile: agent）：采集器，客户端目录只读挂载，数据写 `/data`；生产部署可单独使用 `docker/compose.agent.yaml`

## 2. 镜像

- Hub 镜像支持 Linux amd64/arm64；Agent 原生发布产物支持 Linux amd64/arm64 与 Windows amd64。普通分支提交使用 `:dev-latest` 与 `:<sha>`；正式版本使用 `:<tag>`，并由 Release 工作流将 `:latest` 与 `:master` 绑定到最新正式版本。
- 运行时**不含 Node.js**（前端产物 rust-embed 进 Hub 二进制）。
- 非 root（UID 65532）运行；`user: "${UID}:${GID}"` 与宿主对齐以读取 700 权限目录。

## 3. 配置（环境变量）

| 变量 | 默认 | 说明 |
|---|---|---|
| `METRIA_DATABASE_URL` | `sqlite:///data/metria.db` | Hub 数据库 |
| `METRIA_LISTEN` | `0.0.0.0:8080` | Hub 监听 |
| `METRIA_TIMEZONE` | `Asia/Shanghai` | 展示与报告的环境默认时区（存储恒为 UTC；设置页保存值优先） |
| `METRIA_CONTENT_MODE` | `metadata` | none/metadata/full |
| `METRIA_SESSION_SECRET` | 无安全默认值 | 会话签名密钥，生产环境必须设置随机高强度值 |
| `METRIA_ADMIN_USER` / `METRIA_ADMIN_PASSWORD` | admin / change-me-please（Compose） | 初始 Admin 凭据，部署后立即修改 |
| `METRIA_OIDC_ISSUER` | 无 | OIDC Issuer URL（如 `https://idp.example.com/realms/metria`），与 `CLIENT_ID`/`CLIENT_SECRET` 同时配置即启用 OIDC 登录 |
| `METRIA_OIDC_CLIENT_ID` / `METRIA_OIDC_CLIENT_SECRET` | 无 | OIDC 客户端凭据（confidential client） |
| `METRIA_OIDC_ALLOWED_EMAIL` | 无 | 唯一允许登录的邮箱（忽略大小写；要求 email_verified ≠ false） |
| `METRIA_OIDC_ALLOWED_SUBJECT` | 无 | 可选：唯一允许登录的 subject（与 ALLOWED_EMAIL 任一命中即可） |
| `METRIA_OIDC_REDIRECT_URL` | 从请求 Host 推导 | 显式回调地址 `{origin}/api/v1/auth/oidc/callback`；反代场景建议显式配置 |
| `METRIA_OIDC_DISABLE_PASSWORD_LOGIN` | true（启用 OIDC 时） | 启用 OIDC 后默认禁用本地密码登录；显式设置 `false` 保留密码作为 IdP 故障后备 |
| `METRIA_COLLECTOR_TOKEN` | 无 | Collector 共享 bootstrap token |
| `METRIA_NODE_ID` / `METRIA_NODE_NAME` | 自动 | Agent 节点身份 |
| `METRIA_HUB_URL` | `http://localhost:8080` | Agent 连接 Hub |
| `METRIA_AGENT_TOKEN` / `METRIA_AGENT_TOKEN_FILE` | 无 | Agent 认证 token |
| `METRIA_AGENT_BINARIES_DIR` | `/app/agent-binaries`（Hub 镜像） | Agent 跨平台二进制目录；自定义 Hub 镜像可覆盖 |
| `METRIA_AGENT_DOWNLOAD_BASE_URL` | 无 | 可选：内置资产缺失时的跨平台 Agent Release 下载回退地址 |
| `METRIA_CLAUDE_PATH` / `CODEX_PATH` / `OPENCODE_PATH` | 无 | 客户端目录 |
| `METRIA_SCAN_INTERVAL` / `RECONCILE_INTERVAL` / `HEARTBEAT_INTERVAL` / `UPLOAD_INTERVAL` | 10/300/60/15s | Agent 周期（Pull 模式与本地扫描使用） |
| `METRIA_POLL_INTERVAL` | 60s | **Push 模式**无状态轮询周期（拉游标→扫描→上传→推游标，5~86400 秒；可在「添加节点」时配置，随安装命令注入） |
| `METRIA_TOKEN_REFRESH_INTERVAL` | 6 天 | Agent 重新注册续期周期（< 7 天 token 有效期） |
| `METRIA_MAX_PENDING_EVENTS` / `MAX_SPOOL_BYTES` | 200 万 / 512MiB | Spool 上限 |
| `METRIA_LOG` | `info` | 日志级别 |

## 4. 网络与安全

- 只暴露 Hub 的 8080 端口；Agent 出站仅连 Hub，客户端目录只读挂载。
- Collector token 仅存哈希；过期后需重新注册（Agent 自动每 6 天续期）。
- 不在日志输出 token/secret；默认不上传完整路径/用户名/Hostname/Git Remote/密钥。
- `METRIA_SESSION_SECRET` 修改后已有 Web 会话会失效，需要重新登录。

### 4.1 Agent 双模式：Push 与 Pull

Agent 支持两种采集模式（同一镜像/二进制，按环境变量自动判定）：

| | Push 模式（默认，无状态轮询） | Pull 模式 |
|---|---|---|
| 触发条件 | 配置 `METRIA_HUB_URL` | 仅配置 `METRIA_AGENT_TOKEN`（无 HUB_URL） |
| 数据流向 | Agent → Hub 主动上报 | Hub → Agent 主动拉取 |
| 适用拓扑 | Hub 公网可达 | **Hub 在内网、Agent 在公网** |
| 所需配置 | HUB_URL + NODE_ID + TOKEN | 仅 TOKEN（+ 客户端路径） |
| 本地状态 | **无**（游标外置 Hub，无本地 spool） | 本地 spool（拉取-确认队列） |

**Push 模式（无状态轮询）工作方式**：

1. 每个轮询周期（`METRIA_POLL_INTERVAL`，默认 60s）从 Hub 拉取各 Source 最新游标；
2. 增量扫描客户端日志/数据库，事件直传 Hub（批量上限与 zstd 压缩同旧模式）；
3. 该 Source 全部事件被 Hub 确认（accepted/duplicate）**之后**才推进游标；
   上传失败不推进，下轮从旧游标重扫，Hub 按 event_id 幂等去重；
4. Hub 离线时暂停扫描并退避等待；恢复后按最新游标补齐离线期间的数据
   （离线期间被轮转/删除的历史文件无法补齐，见 operations 文档）；
5. 不写任何本地持久化状态：`/data` 卷仅用于旧版升级时的一次性迁移，可为空卷。

**Pull 模式工作方式**：

1. Web「添加节点」时填写 **Agent 地址**（如 `<agent-ip>:8090`，不需要填写协议）；Hub 地址默认使用当前网页地址；
2. 安装命令只包含 Token 与客户端只读挂载（无需 Hub 地址与 Node ID）；
3. Agent 本地监听 `8090`（`METRIA_LISTEN_PORT` 可调），Hub 按 `METRIA_PULL_INTERVAL`（默认 60s，最小 5s）周期拉取；
4. Hub 拉取成功后确认，Agent 删除本地暂存；失败自动退避重试，数据不丢失。

**安全提示**：Pull 模式下 Agent 端口若暴露公网，务必注意：API 已要求 Bearer Token 认证，
但传输默认为明文 HTTP；生产建议在 Agent 节点用 Caddy/Nginx 反代加 TLS（已有 HTTPS 配置仍可保留），
或使用 WireGuard 等隧道。节点 Token 可在 Web 重新生成安装命令时轮换。

> 节点级 Token 使用 `METRIA_SESSION_SECRET` 派生密钥加密存储于 Hub；轮换该密钥后需在 Web
> 重新生成安装命令以刷新节点 Token。

### 4.2 OIDC 单用户登录

Metria 支持通过任意标准 OIDC Provider（Keycloak / Authentik / Auth0 / Google / Entra 等）登录 Web 控制台，**仅允许一个白名单账号**（单用户模式，不支持多用户）。

**IdP 侧配置**（以 Keycloak 为例）：

1. 创建 confidential client（如 `metria`），启用 Standard Flow（Authorization Code）。
2. Valid redirect URI：`https://<metria-origin>/api/v1/auth/oidc/callback`。
3. 记录 Client Secret。

**Hub 侧配置**：

```yaml
# docker compose 环境变量示例
METRIA_OIDC_ISSUER=https://idp.example.com/realms/metria
METRIA_OIDC_CLIENT_ID=metria
METRIA_OIDC_CLIENT_SECRET=<secret>
METRIA_OIDC_ALLOWED_EMAIL=owner@example.com
# 可选：反代后建议显式指定回调地址
# METRIA_OIDC_REDIRECT_URL=https://metria.example.com/api/v1/auth/oidc/callback
# 可选：保留密码登录作为 IdP 故障后备（默认已禁用）
# METRIA_OIDC_DISABLE_PASSWORD_LOGIN=false
```

**行为说明**：

- 未配置 OIDC 时行为不变（本地密码登录）。
- 配置后登录页出现「使用 OIDC 登录」按钮；回调成功后 Hub 校验 userinfo 身份并复用既有签名会话机制；OIDC 首次登录自动创建无本地密码的用户记录。
- 白名单外账号即使通过 IdP 认证也会被明确拒绝。
- 启用 OIDC 后默认**仅保留 OIDC 入口**（本地密码登录被禁用）；如需 IdP 故障时的应急后备，显式设置 `METRIA_OIDC_DISABLE_PASSWORD_LOGIN=false`。
- 安全提示：身份校验使用 IdP userinfo 端点（未做本地 JWKS 验签），要求 Issuer 必须走 HTTPS；state 一次性且 10 分钟过期，交换码一次性且 60 秒过期，会话 token 不经过 URL。

## 5. 用量报告

报告在 Web「设置 → 用量报告」中配置，配置写入 Hub 的 SQLite `settings` 表，不需要重新部署 Agent，也没有对应的必填环境变量。

### 5.1 渠道与内容

- 邮件渠道支持 SMTP 明文、STARTTLS 和 SSL/TLS；发件人、收件人和 SMTP 密码在设置页配置。
- Webhook 渠道使用通用 JSON，可配置 URL、自定义 Header 和 `X-Metria-Secret`。
- 收件人留空时，Hub 会尝试解析当前 OIDC 用户邮箱；解析不到时邮件渠道会报告无有效收件人。
- 邮件同时发送 HTML 和纯文本正文；启用图表时，趋势图以内嵌 SVG 放在 HTML 正文中。
- 报告只包含汇总指标，不包含会话正文、提示词或代码；PDF 当前不作为邮件附件。

SMTP 密码和 Webhook Secret 只保存在 Hub 本地数据库，读取 API 不会回传 SMTP 密码。请将 `/data` 卷、备份文件和 Webhook 目标都按敏感配置保护。

### 5.2 调度与失败处理

每日、每周、每月调度相互独立，按设置页保存的全局 IANA 时区计算上一完整周期。每月日期限制为 1–28，避免短月份产生歧义。

同一个自动调度的 `类型 + 周期` 只允许一次投递尝试。失败后当前周期不会自动持续重试；修复配置后可点击「发送测试邮件/Webhook」，下一周期会正常重新发送。发送历史最多保留最新 30 条，设置页每页显示 10 条。

## 6. 升级 / 回滚

见 `docs/operations.md`：

- 升级：拉新镜像 → 重启；启动时自动应用版本化迁移。
- **升级顺序：先 Hub，后各节点 Agent**（push 模式 Agent 依赖 Hub 游标同步接口；
  旧 Hub 上新 Agent 会明确报错等待，升级 Hub 后重启 Agent 即可）。
- Agent 升级时若存在遗留本地 spool 会自动一次性迁移（清积压 → 推游标 → 删除本地文件）。
- 回滚：回退镜像 tag；若已应用新迁移，先恢复旧数据库（VACUUM INTO + zstd 备份）。

## 7. 健康检查与诊断

- `metria healthcheck`（容器内 CMD）：连通性 + 数据库 quick_check。
- `metria doctor --adapter|--traffic|--hub|--database|--spool`：环境诊断。

## 8. 前端添加节点与安装 Agent

Web 端「节点 → 添加节点」可预先创建节点并生成安装命令，目标机运行命令即接入（Beszel 式流程）：

1. **添加节点**：填写名称/描述/标签、平台（Linux/Windows）、架构（amd64/arm64）；Hub 地址自动使用当前页面 origin，Pull 模式下再填写 Agent IP/域名及可选端口。
2. **获取安装命令**：创建后自动展示一次性专属 Token 与两种安装命令（可切换、一键复制）。
3. **目标机安装**：
   - **Docker**：`docker run -d --name metria-agent ... ghcr.io/supercxyz/metria:latest agent`（命令已注入 node_id/token/hub_url 与客户端只读挂载）。
   - **原生 Linux**：页面命令从节点专属地址 `/api/v1/nodes/{node_id}/agent/download` 公开下载对应架构的 `metria-linux-amd64` 或 `metria-linux-arm64`，Hub 镜像已内置资产，无需额外配置或 Token；命令会强制安装 `metria-agent.service`，将配置保存到 root-only 的 `/etc/metria/metria-agent.env`，并通过 systemd 设置开机启动。
   - **原生 Windows**：页面生成的 PowerShell 命令从当前 Hub 下载内置的 `metria-windows-amd64.exe` 到 `%LOCALAPPDATA%\Metria`，生成持久化启动配置，并注册 `Metria Agent` 登录自启动计划任务。
   - **命令复制**：Docker 命令预览为带 shell 续行符的多行格式，复制按钮会折叠为单行；原生 Linux/Windows 命令保留多行脚本格式。
4. **接入确认**：Agent 用专属 token 注册后，节点变为「在线」，名称保持创建/编辑时设置的值。
5. **节点维护**：列表行支持「编辑」（改名称/描述/标签/Agent 地址）与「删除」（移除身份与令牌，历史用量数据保留）。

> 说明：二进制下载接口公开，只根据节点 ID 读取平台/架构并选择文件，不接收 Token；Hub 镜像包含 Linux amd64、Linux arm64、Windows amd64 三个资产。专属 Token 明文仅创建或生成安装命令时展示，Hub 只存哈希。Token 过期后可在节点详情重新生成安装命令（自动签发新 token，不吊销正在使用的旧 token）。

## 9. 规模与性能

- Hub 与 Agent 均 SQLite；rollup 增量更新，查询读汇总表。
- 后台维护：每 6h rollup 对账（发现漂移自动重建最近 24h）+ WAL checkpoint。
- 基准参考：10 万事件写入约 280ms，overview 查询约 20µs；1M 事件基准见 `docs/operations.md`。
