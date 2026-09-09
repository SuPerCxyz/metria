-- 无状态轮询：Hub 侧保存 Source 扫描游标（cursor_json 对 Hub 不透明）
-- 键：(collector_id, source_id)；collector_id 为确定性 collector-{node_id}，跨重启稳定
CREATE TABLE IF NOT EXISTS source_cursors (
    collector_id TEXT NOT NULL REFERENCES collectors(id) ON DELETE CASCADE,
    source_id    TEXT NOT NULL,
    cursor_json  TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    PRIMARY KEY (collector_id, source_id)
);
CREATE INDEX IF NOT EXISTS idx_source_cursors_collector ON source_cursors(collector_id);
