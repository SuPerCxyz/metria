-- 价格目录只保留最新快照与规则，并归一化 Codex 的 input_tokens。
--
-- 背景：
--   1) 每次目录同步都会新建快照并插入全量规则，旧快照规则仅置 enabled=0 保留，
--      导致历史规则无限堆积（线上 18 万+ 条）并在列表接口重复展示。
--      用户确认不保留旧计价规则，故删除历史快照与其规则，只留每个目录的最新快照。
--   2) Codex 的 input_tokens 已包含 cached_input_tokens（且 cache_write_input_tokens
--      亦为其子集），而 Metria 约定 input 为非缓存输入。需回填为非缓存输入，
--      使总 Token、缓存命中率 `cache_read/(input+cache_read)`、费用与流量口径一致。

-- 1. 删除非最新快照的规则（用户规则 snapshot_id 为 NULL，不受影响）
DELETE FROM pricing_rules
WHERE snapshot_id IS NOT NULL
  AND snapshot_id NOT IN (
    SELECT id FROM (
      SELECT id, ROW_NUMBER() OVER (PARTITION BY catalog_id ORDER BY fetched_at DESC, id DESC) AS rn
      FROM pricing_snapshots
    ) WHERE rn = 1
  );

-- 2. 删除非最新快照本身
DELETE FROM pricing_snapshots
WHERE id NOT IN (
  SELECT id FROM (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY catalog_id ORDER BY fetched_at DESC, id DESC) AS rn
    FROM pricing_snapshots
  ) WHERE rn = 1
);

-- 3. 回填 Codex 的非缓存输入（usage_events 与 model_calls）
UPDATE usage_events
SET input_tokens = MAX(0, COALESCE(input_tokens, 0) - COALESCE(cache_read_tokens, 0) - COALESCE(cache_write_tokens, 0))
WHERE client_id = 'codex'
  AND input_tokens IS NOT NULL
  AND (COALESCE(cache_read_tokens, 0) + COALESCE(cache_write_tokens, 0)) > 0;

UPDATE model_calls
SET input_tokens = MAX(0, COALESCE(input_tokens, 0) - COALESCE(cache_read_tokens, 0) - COALESCE(cache_write_tokens, 0))
WHERE client_id = 'codex'
  AND input_tokens IS NOT NULL
  AND (COALESCE(cache_read_tokens, 0) + COALESCE(cache_write_tokens, 0)) > 0;
