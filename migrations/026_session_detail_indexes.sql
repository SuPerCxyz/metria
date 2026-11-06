-- B9：会话详情 keyset 分页索引。
--
-- 会话明细按 (started_at, id) 稳定排序；复合索引让分页游标直接 range scan，
-- 避免先按 session_id 取出全部调用再排序/截断。
CREATE INDEX IF NOT EXISTS idx_calls_session_time_id
    ON model_calls(session_id, started_at, id);

CREATE INDEX IF NOT EXISTS idx_tools_session_time_id
    ON tool_events(session_id, started_at, id);
