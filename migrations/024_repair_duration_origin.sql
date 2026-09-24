-- 修复调用时长的存量脏数据：opencode turn 起点虚增 + 无请求起点口径的诚实置空。
--
-- 背景：
--   旧版 opencode adapter 把「同回合上一条 user prompt 的时刻」当作单条消息调用的左端点：
--   同一 turn 内每条调用共享该起点，duration_ms 随调用数成倍虚增（生产实测最极端一个起点
--   被 10,212 条调用共享）。2026-09-24 生产快照：全库 SUM(duration_ms)=101,963h，其中
--   opencode 占 101,332.6h；>6h 的 4,750 条全部来自 opencode（最长 24.2h），而正确实现的
--   codex 最大 40 分钟、>6h 为 0。采集侧已改为取 data.time.created（该条消息的请求发起
--   时刻），本迁移修迁移前入库的存量行。
--
-- 修法：
--   ① opencode 可重算行（first_response_at 与 completed_at 都在且时序合法，实测 256,644 条）：
--        started_at     = first_response_at（v1 即 time.created；v2 是首字节流时刻，比真实
--                         请求起点略晚，重算是诚实的下界，绝不会虚增）
--        duration_ms    = completed_at - first_response_at（毫秒）
--        timing_quality = 'observed'（两端都是观测值）
--   ② claude-code 全部带 duration 的行（实测 5,364 条、282.4h）：JSONL 无请求发起时刻，存量
--      时长是 turn 跨度而非单次调用耗时；新采集行为已确认该口径不可用 → duration 置 NULL、
--      timing_quality='unavailable'、timing_source 统一为 'legacy_unavailable'（与
--      repair_legacy_call_timings 在 duration 缺失时的既有词汇一致，避免残留
--      legacy_bounded_duration 这种与 NULL 时长矛盾的标签）。
--   ③ 兜底：opencode 修复后 fr→completed 跨度仍 >6h 的行（实测 16 条，修复前也 >6h，
--      成因是历史回填把 turn 起点写进了 first_response_at（指纹：started==fr）或挂死的
--      流共用 completed——单条消息持续 6 小时以上不可信，两种成因都无法与合法行区分）；
--      任何客户端 >24h（与采集侧 insert_call 的 24h 软降级同口径）；opencode 缺字段/
--      时序倒挂却带 duration 的行（实测 0 条）→ duration 置 NULL + 'unavailable'
--      （缺失用 null、禁止保留不可信值）。started_at 保持现状：时长缺失后它们只影响
--      计数分桶，真实起点已不可考，不硬造。
--
-- 边界：
--   - codex 的正常行不动：其起点本就正确，动它只会把正确时长改小并挪动分桶；
--   - 时序校验用 julianday 实值比较，不依赖字符串字面序（同列存在 'Z' 与 '+00:00'
--     两种写法，字符串比较会误判 25 条）；负时长 0 条；
--   - 不触碰 Token / 费用 / usage_events / pricing_matches / traffic / completed_at
--     （时长修复与它们无关，completed_at 不变因此 sessions 活跃字段也不受影响）；
--   - 幂等：重跑写入相同值或空转；新装实例没有存量，三条 UPDATE 空转；
--   - 与 repair_legacy_call_timings 的先后关系（同一次启动内先迁移后回填）已核对：
--     该回填的 opencode 分支要求 timing_source IS NULL 且 turns 表恒空，重算结果与本
--     迁移一致；claude-code 分支因 ② 已写 source 而被排除，不会把 NULL 时长改回 turn 跨度。
--
-- 性能：全部是 model_calls 单表 UPDATE（谓词单趟扫描、无跨表相关子查询），刻意避开
--   曾在线上跑分钟级的「按行 EXISTS 全表扫无索引列」形态；已在生产库副本上完成计时验证。
--   started_at 后移会重排 hourly/daily_rollups 的 model_call_count：由启动期门控重建补齐
--   （settings 键 duration_origin_repair_version → rebuild_rollups(36_500)）；
--   hourly_performance_rollups 每次启动全量重建；usage/traffic 聚合用自己的时间戳来源，
--   不受影响，无需重建。

-- ① opencode 可重算行
UPDATE model_calls
SET started_at = first_response_at,
    duration_ms = CAST(ROUND(
        (julianday(completed_at) - julianday(first_response_at)) * 86400000.0
    ) AS INTEGER),
    timing_quality = 'observed'
WHERE client_id = 'opencode'
  AND duration_ms IS NOT NULL
  AND first_response_at IS NOT NULL
  AND completed_at IS NOT NULL
  AND julianday(completed_at) >= julianday(first_response_at);

-- ② claude-code：无请求起点，turn 跨度口径不可用
UPDATE model_calls
SET duration_ms = NULL,
    timing_quality = 'unavailable',
    timing_source = 'legacy_unavailable'
WHERE client_id = 'claude-code'
  AND duration_ms IS NOT NULL;

-- ③ 兜底：>24h（全客户端）或 opencode >6h / 无法安全重算的行
UPDATE model_calls
SET duration_ms = NULL,
    timing_quality = 'unavailable'
WHERE duration_ms IS NOT NULL
  AND (
    duration_ms > 86400000
    OR (client_id = 'opencode'
        AND (duration_ms > 21600000
             OR first_response_at IS NULL
             OR completed_at IS NULL
             OR julianday(completed_at) < julianday(first_response_at)))
  );
