-- 回填旧 Agent 期间入库的 Codex 行：input 补扣缓存、output 补扣推理。
--
-- 背景：
--   017（input 归一化为非缓存）与 018（output 归一化为不含推理）都只回填「迁移执行时刻
--   已存在的行」；而节点 aitools 的 Agent 一直运行旧镜像，直到 2026-09-16T05:40Z 才更新。
--   因此 017 应用后至 Agent 更新前入库的 Codex 行 input 仍含缓存，018 应用后至 Agent 更新
--   前入库的行 output 仍含推理。Hub 不做 ingest 归一化，这些行必须回填，否则缓存会按输入价
--   被重复计费（实测今日 gpt-5.6-luna 因此虚高约 9 倍）。
--
-- 边界：
--   - 下界取 schema_migrations 中 017 / 018 的 applied_at（由本库自证，不硬编码实例时间）；
--   - 上界取 Agent 更新时刻 2026-09-16T05:40:00Z（本次 rollout 的固定事实）；
--   - 另加 input >= cache_read + cache_write、output >= reasoning 的必要条件守卫，
--     已归一化的行在这些条件下可能仍成立，所以窗口是必需的主判据，守卫用于兜底避免二次扣减。
--   新装实例上 017/018/019 同时应用，窗口为空，不会误改数据。
--
-- 性能：usage_events 侧必须写成 `event_id IN (SELECT usage_event_id ...)`，让窗口子查询只扫
--   model_calls 一次并物化少量 id；写成相关子查询 EXISTS(...) 会按 usage_events 每行全表扫
--   model_calls（model_calls.usage_event_id 无索引），在线上 30 万级行数下会跑到分钟级以上。

UPDATE usage_events
SET input_tokens = MAX(
        0,
        COALESCE(input_tokens, 0) - COALESCE(cache_read_tokens, 0) - COALESCE(cache_write_tokens, 0)
    )
WHERE client_id = 'codex'
  AND COALESCE(input_tokens, 0) >= COALESCE(cache_read_tokens, 0) + COALESCE(cache_write_tokens, 0)
  AND event_id IN (
        SELECT usage_event_id FROM model_calls
        WHERE client_id = 'codex'
          AND usage_event_id IS NOT NULL
          AND julianday(created_at) > julianday((SELECT applied_at FROM schema_migrations WHERE version = 17))
          AND julianday(created_at) <= julianday('2026-09-16T05:40:00Z')
    );

UPDATE model_calls
SET input_tokens = MAX(
        0,
        COALESCE(input_tokens, 0) - COALESCE(cache_read_tokens, 0) - COALESCE(cache_write_tokens, 0)
    )
WHERE client_id = 'codex'
  AND COALESCE(input_tokens, 0) >= COALESCE(cache_read_tokens, 0) + COALESCE(cache_write_tokens, 0)
  AND julianday(created_at) > julianday((SELECT applied_at FROM schema_migrations WHERE version = 17))
  AND julianday(created_at) <= julianday('2026-09-16T05:40:00Z');

UPDATE usage_events
SET output_tokens = MAX(0, COALESCE(output_tokens, 0) - COALESCE(reasoning_tokens, 0))
WHERE client_id = 'codex'
  AND COALESCE(output_tokens, 0) >= COALESCE(reasoning_tokens, 0)
  AND event_id IN (
        SELECT usage_event_id FROM model_calls
        WHERE client_id = 'codex'
          AND usage_event_id IS NOT NULL
          AND julianday(created_at) > julianday((SELECT applied_at FROM schema_migrations WHERE version = 18))
          AND julianday(created_at) <= julianday('2026-09-16T05:40:00Z')
    );

UPDATE model_calls
SET output_tokens = MAX(0, COALESCE(output_tokens, 0) - COALESCE(reasoning_tokens, 0))
WHERE client_id = 'codex'
  AND COALESCE(output_tokens, 0) >= COALESCE(reasoning_tokens, 0)
  AND julianday(created_at) > julianday((SELECT applied_at FROM schema_migrations WHERE version = 18))
  AND julianday(created_at) <= julianday('2026-09-16T05:40:00Z');
