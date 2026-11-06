-- 性能优化 A 阶段：重计价传播与未计价扫描的索引。
--
-- `model_calls.usage_event_id` 原先无索引，重计价结束后的费用传播需要按该列
-- 关联 `usage_events`；在线上大表上会退化为逐行相关子查询。
CREATE INDEX IF NOT EXISTS idx_calls_usage_event ON model_calls(usage_event_id);

-- 仅索引尚未完成计价的 usage 行；后台增量重计价只扫描这部分候选。
-- 使用 event_id 作为键，配合 `WHERE event_id > ? ORDER BY event_id LIMIT ?`
-- 的 keyset 分页，避免每次同步全表扫描已计价行。
CREATE INDEX IF NOT EXISTS idx_usage_unpriced ON usage_events(event_id)
    WHERE reported_cost_micro_usd IS NULL
      AND calculated_cost_micro_usd IS NULL
      AND estimated_cost_micro_usd IS NULL;
