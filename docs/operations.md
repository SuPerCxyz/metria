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
- 恢复会覆盖目标数据库并清理残留 WAL/SHM 文件。
- 建议配合 cron/系统定时任务定期备份，并保留最近 N 份。

## 升级

```bash
docker compose -f docker/compose.yaml pull
docker compose -f docker/compose.yaml up -d
```

- 启动时自动应用版本化迁移（`migrations/N_*.sql`），无需手工执行。
- 升级前建议先备份。

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
| `METRIA_NODE_ID` / `METRIA_NODE_NAME` | Agent 节点身份 |
| `METRIA_HUB_URL` / `METRIA_AGENT_TOKEN_FILE` | Agent 连接配置 |
