-- 028：晚到补采失效小时的持久化队列 + 会话查询索引兜底。
--
-- dirty 表记录需要重新构建的小时；维护任务按小时合并重建，成功后删除。
-- `idx_sessions_source_session` 在已应用旧 027 的环境里补建；新环境 027 已建，
-- 这里 IF NOT EXISTS 为 no-op。
-- `idx_sessions_sort_at` 支撑 call_end 会话的排序/过滤表达式。
CREATE TABLE IF NOT EXISTS perf_rollup_dirty_hours (
    hour TEXT PRIMARY KEY,
    marked_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_source_session ON sessions(source_session_id);
-- call_end 会话排序/过滤使用会话结束时间表达式；表达式索引让 OR/排序有索引可用。
CREATE INDEX IF NOT EXISTS idx_sessions_sort_at
    ON sessions(COALESCE(ended_at, last_activity_at, started_at));
