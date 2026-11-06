-- D 阶段：筛选与活动查询索引。
--
-- 只新增索引，不改表结构、不回填数据。三条索引分别服务：
-- 1. `list_sessions` 的 started_at / last_activity_at OR 过滤；
-- 2. 带 client 过滤且按时间范围扫描的调用查询；
-- 3. 带 client 过滤且按时间范围扫描的用量查询。
-- 4. `resolve_session_key_by_id` / 子会话父级回填按 source_session_id 查找。
CREATE INDEX IF NOT EXISTS idx_sessions_activity ON sessions(last_activity_at);
CREATE INDEX IF NOT EXISTS idx_calls_client_time ON model_calls(client_id, started_at);
CREATE INDEX IF NOT EXISTS idx_usage_client_time ON usage_events(client_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_sessions_source_session ON sessions(source_session_id);
