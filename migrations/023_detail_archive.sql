-- 明细归档的前置准备：默认关闭 + 两个按时间删除所需的索引。
--
-- 归档语义（spec `data-archival`）：
--   * 默认关闭：未配置归档时不删除任何明细，保持设置页「全量保留」的既有承诺；
--   * 只删明细、保留聚合：hourly/daily rollup 与小时级性能预聚合不动，因此
--     总览、趋势、用量报告与热力图在归档区间内的数值与归档前完全一致；
--   * 删除前强制备份，备份失败即中止。
--
-- 这里补的两个索引是「分批、小事务删除」的前提：
--   messages 只有 session 索引、tool_events 也只有 session 索引，按时间删除会退化成
--   每批都全表扫描（messages 117MiB、实测一次全表 COUNT 需数秒），既慢又会长时间持锁。
CREATE INDEX IF NOT EXISTS idx_messages_created ON messages(created_at);
CREATE INDEX IF NOT EXISTS idx_tools_started ON tool_events(started_at);

INSERT INTO settings (key, value, updated_at) VALUES
    ('archive_enabled', '0', strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now')),
    ('archive_retention_days', '365', strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now'))
ON CONFLICT(key) DO NOTHING;
