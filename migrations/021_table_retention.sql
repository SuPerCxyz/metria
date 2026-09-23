-- 运维表保留期清理与默认保留设置。
--
-- 背景（2026-09-22 lstable 生产库实测）：
--   * upload_batches：386 万行 / 802MiB（含 172MiB 自动索引），Hub 侧只有 INSERT
--     （api/mod.rs record_batch），全仓库无任何读路径；约 35 万行/天，全部落在最近
--     11 天内，按“保留 30 天”等于不清理，必须按短周期保留。
--   * pricing_matches：157 万行 / 559MiB（含 265MiB 索引），其中 reprice 135 万行、
--     ingest 22 万行，去重后仅 31.8 万个 usage_event（约 4.9 行/事件），全部落在最近
--     13 天内（2026-09-09 ~ 09-22），任何按时间的保留窗口都收敛不了，必须按覆盖关系
--     清理：只保留每个 usage_event 的最新一条，即可解释当前 calculated_cost。
--
-- 副本实测（AGENTS.md §12.2 要求）：
--   * upload_batches 删 3,475,696 行 6.77s；
--   * pricing_matches 删 1,256,496 行 14.15s，EXPLAIN 走
--     SEARCH p USING INDEX idx_pricing_matches_usage (usage_event_id=?)，关联列为索引列；
--   * 无计价记录的 usage_events 清理前后均为 5,530（未增加），98.29% 的 usage_events
--     仍能被一条计价匹配解释；
--   * 合计 21s 单事务（WAL 下不阻塞读），配合启动时 auto_vacuum=INCREMENTAL + VACUUM
--     （实测 10.7s）把库从 2362MiB 收敛到 1238MiB。
--   * VACUUM 不能在事务里执行，因此不在本迁移内，由启动路径按需触发。
-- 删除行数通过 changes() 在同一语句序列内记录，避免硬编码实例相关常量。

-- 1. 上传批次只保留最近 24 小时（纯写入审计，无读路径）。
--    received_at 为 RFC3339 UTC 文本，格式与 strftime 产出的裁剪值可直接做字典序比较。
DELETE FROM upload_batches
WHERE received_at < strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now', '-1 day');

INSERT INTO settings (key, value, updated_at)
SELECT 'cleanup_upload_batches_deleted', CAST(changes() AS TEXT),
       strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now')
ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at;

-- 2. 计价匹配只保留每个 usage_event 的最新一条，删除被后续重计价覆盖的历史行。
--    每个 usage_event 至少保留一条记录，历史费用仍可追溯（usage-cost-integrity）。
DELETE FROM pricing_matches
WHERE EXISTS (
    SELECT 1 FROM pricing_matches p
    WHERE p.usage_event_id = pricing_matches.usage_event_id
      AND (p.calculated_at > pricing_matches.calculated_at
        OR (p.calculated_at = pricing_matches.calculated_at
            AND p.id > pricing_matches.id))
);

INSERT INTO settings (key, value, updated_at)
SELECT 'cleanup_pricing_matches_deleted', CAST(changes() AS TEXT),
       strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now')
ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at;

-- 3. 汇总本次清理结果与默认保留设置（已存在的保留配置不覆盖，便于运维预先配置）。
INSERT INTO settings (key, value, updated_at)
SELECT 'table_last_cleanup_deleted',
       CAST(COALESCE((SELECT CAST(value AS INTEGER) FROM settings
                      WHERE key = 'cleanup_upload_batches_deleted'), 0)
          + COALESCE((SELECT CAST(value AS INTEGER) FROM settings
                      WHERE key = 'cleanup_pricing_matches_deleted'), 0) AS TEXT),
       strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now')
ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at;

INSERT INTO settings (key, value, updated_at) VALUES
    ('table_last_cleanup_at', strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now'),
     strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now'))
ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at;

INSERT INTO settings (key, value, updated_at) VALUES
    ('upload_batch_retention_hours', '24', strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now')),
    ('pricing_match_history_days', '0', strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now'))
ON CONFLICT(key) DO NOTHING;
