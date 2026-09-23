-- 小时级性能预聚合表。
--
-- 背景（2026-09-22 lstable 实测）：一次总览页刷新并发 13 个查询、合计 3.1~3.6 CPU·秒，
-- 其中 `/usage/performance` 独占 455ms、`/usage/heatmap` 独占 325ms、`/usage/latency`
-- 与 `/overview` 的时长分位数各需把范围内 17.5 万行 `model_calls` 拉进应用层排序。
-- 这四类查询的公共前提是「按时间范围聚合」，因此按 UTC 小时固化结果：
-- 30 天范围的读取量从 17.5 万行降到约 720 行。
--
-- 结构刻意保持极简（每小时一行、payload 为 JSON）：
--   * 行数 = 小时数（一年 8760 行），不需要与 hourly_rollups 相同的 12 维主键；
--   * 维度筛选（节点/Agent/模型/项目等）无法命中预聚合，由处理器回退明细查询，
--     正确性优先，代价只落在带筛选的少数请求上；
--   * payload 版本号写在 JSON 内（`v`），语义变更时旧版本行直接重建，无需再加迁移。
--
-- payload 字段语义见 `crates/metria-hub/src/perfrollup.rs`。
CREATE TABLE IF NOT EXISTS hourly_performance_rollups (
    bucket   TEXT PRIMARY KEY,
    payload  TEXT NOT NULL,
    built_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_perf_rollups_built_at
    ON hourly_performance_rollups(built_at);
