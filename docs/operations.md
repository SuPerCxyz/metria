# 运维手册：数据保留、备份恢复、升级回滚

## 数据保留策略

Metria 默认**全量保留**原始事件与 rollup，无自动删除。可依据需求配置保留策略：

| 数据 | 默认 | 说明 |
|---|---|---|
| `usage_events` / `model_calls` | 永久 | 单次调用明细，统计可追溯 |
| `sessions` / `messages` | 永久 | 会话与（按 content_mode 的）消息元数据 |
| `hourly_rollups` / `daily_rollups` | 永久 | Dashboard 读取的汇总 |
| `traffic_estimates` | 永久 | 估算流量，保留版本以便重新估算对比 |
| `traffic_profile_samples` | 永久 | 自动学习样本 |
| `report_sends` | 最新 30 条 | 报告渠道、周期、状态和错误详情 |

**建议**：

- 若需限制磁盘占用，可对 `messages.content` 等大字段单独清理（保留元数据与 hash）。
- Rollup 可通过 `DELETE FROM hourly_rollups WHERE bucket < ?` 归档早期聚合；原始事件仍保留。
- 磁盘容量规划：100 万条 usage 事件约 300–500 MiB（含索引），流量估算约 100–200 MiB。

## 备份与恢复

```bash
# 在线备份（一致性快照，无需停机；默认输出 .zst）
metria backup --out /backup/metria-20260805.db.zst

# 恢复（必须先停止 Hub，避免写冲突）
docker compose -f docker/compose.yaml stop
metria restore --input /backup/metria-20260805.db.zst
docker compose -f docker/compose.yaml start
```

- 备份使用 SQLite `VACUUM INTO`，WAL 安全，生成一致性快照。
- 备份包含报告配置（`settings`）和发送历史（`report_sends`），恢复后会一并恢复 SMTP/Webhook 设置及历史记录。
- 恢复会覆盖目标数据库并清理残留 WAL/SHM 文件。
- 建议配合 cron/系统定时任务定期备份，并保留最近 N 份。

## 用量报告运维

报告配置在 Web「设置 → 用量报告」维护，配置保存于 Hub 数据库，不需要在 Agent 节点重复配置。

### 调度与投递

- 每日、每周、每月调度独立运行，按设置页保存的全局 IANA 时区汇总上一完整周期。
- 邮件渠道使用 SMTP，支持明文、STARTTLS 和 SSL/TLS；Webhook 渠道使用通用 JSON。
- 邮件包含 HTML 和纯文本正文；启用图表时，趋势图以内嵌 SVG 发送。PDF 渲染能力保留，但当前不作为邮件附件投递。
- 自动调度对同一个 `类型 + 周期` 只尝试一次。失败后本周期不自动重试；修复配置后可手动测试，下一周期会重新尝试。
- 测试发送不占用自动调度周期。发送历史最多保留 30 条，前端每页展示 10 条。

### 故障排查

1. 在设置页确认 SMTP 主机、端口、TLS 模式、发件人和收件人；Webhook 则确认 URL、Header 和密钥。
2. 点击「发送测试邮件/Webhook」，查看渠道级错误详情。
3. 如果自动报告已在本周期失败，修复配置后不要等待当前周期自动重试，直接使用测试发送验证；下一完整周期会重新发送。
4. 报告密码和 Webhook Secret 存在 Hub 本地数据库，备份文件与 `/data` 卷应按敏感数据保护。

## 升级

```bash
docker compose -f docker/compose.yaml pull
docker compose -f docker/compose.yaml up -d
```

- 启动时自动应用版本化迁移（`migrations/N_*.sql`），无需手工执行。
- 升级前建议先备份。

### 升级顺序（无状态轮询 Agent）

- **先升级 Hub，再升级各节点 Agent**：push 模式 Agent 依赖 Hub 的游标同步接口
  （`GET/POST /api/v1/collectors/cursors`）；旧版 Hub 上新版 Agent 会明确报错退出（等待升级）。
- 新版 Agent 首次启动若检测到遗留本地 spool，会自动执行一次性迁移：
  **先尽力上传积压事件（Hub 确认）→ 再推送本地游标到 Hub → 最后删除本地 spool 文件**。
  任一步失败会保留现场，下次启动重试；迁移顺序保证「游标只会在数据确认后推进」。
- 迁移成功后本地 `spool.db` 被删除（可释放数百 MB 磁盘），`metria doctor --spool`
  将显示「无状态轮询模式：游标存于 Hub」。

### 离线与数据取舍（无状态模式）

- Hub 离线期间 Agent **暂停扫描**并指数退避等待；恢复后拉取最新游标，从断点补扫补齐
  离线期间新增的数据（Hub 按 event_id 幂等去重，不重复入库）。
- 离线期间被客户端**轮转或删除**的历史文件对应的数据无法补齐（本地无缓冲），
  Agent 会在日志中说明；如需零丢失可继续使用 Pull 模式（保留本地 spool）。
- Hub 永久拒绝的事件（如校验失败）会记录 ERROR 日志（含 event_id 与原因）后跳过，
  游标继续推进，不会因坏事件阻塞采集。

### 手动清理遗留 spool

若升级 Agent 后因 Hub 长期不可达导致迁移反复失败、spool 占用磁盘：

```bash
# 1. 确认 Hub 可达且 Hub 侧已有该节点游标（或接受丢弃积压）
metria doctor --spool   # 查看积压量
# 2. 停止 Agent 后手动删除（将丢失未上传的积压事件；已上传部分不受影响）
rm -f /data/spool.db /data/spool.db-wal /data/spool.db-shm
```

## 回滚

```bash
# 1. 停止并回退镜像 tag
docker compose -f docker/compose.yaml stop
# 编辑 compose 中 image 为上一个版本

# 2. 若已应用新迁移，需先恢复旧数据库
metria restore --input /backup/upgrade-before.db.zst
docker compose -f docker/compose.yaml up -d
```

> 注意：Metria 迁移只增不减；回滚到旧版本时，旧二进制可能无法理解新 schema。
> 因此回滚必须同时恢复备份数据库。请保留升级前备份。

## 性能基准（本地，可复现）

```bash
cargo test -p metria-hub --test bench -- --ignored --nocapture
```

参考结果（测试机 8 核 / NVMe）：

| 场景 | 结果 |
|---|---|
| 10 万 usage 事件批量写入 | ~280ms（约 35 万/秒） |
| 100 万 usage 事件批量写入 | ~13s（约 7.7 万/秒） |
| Overview 查询（读 rollup） | ~20µs |
| 价格匹配 | ~312 次/ms |
| 流量重建估算 | ~29 次/ms |

Dashboard 默认读 rollup，不在每次请求时扫描全部历史事件。

## 环境变量速查

| 变量 | 作用 |
|---|---|
| `METRIA_DATABASE_URL` | Hub 数据库（`sqlite:///data/metria.db`） |
| `METRIA_ADMIN_USER` / `METRIA_ADMIN_PASSWORD` | 单 Admin 初始凭据 |
| `METRIA_COLLECTOR_TOKEN` | Collector 引导 token（Agent 注册用） |
| `METRIA_PRICING_OPENROUTER_ENABLED` | 启用 OpenRouter 价格目录 |
| `METRIA_PRICING_LITELLM_ENABLED` | 启用 LiteLLM 价格目录 |
| `METRIA_PRICING_CUSTOM_URL` / `_AUTH` | 自定义 HTTP 价格目录 |
| `METRIA_CONTENT_MODE` | `none` / `metadata` / `full` |
| `METRIA_TIMEZONE` | Web 展示与报告的环境默认时区（设置页保存值优先） |
| `METRIA_NODE_ID` / `METRIA_NODE_NAME` | Agent 节点身份 |
| `METRIA_HUB_URL` / `METRIA_AGENT_TOKEN_FILE` | Agent 连接配置 |
