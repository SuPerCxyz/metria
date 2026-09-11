-- OIDC 头像仅保存已校验的 HTTPS URL；调用 timing 字段记录源日志观测口径。
ALTER TABLE users ADD COLUMN avatar_url TEXT;
ALTER TABLE model_calls ADD COLUMN timing_source TEXT;
ALTER TABLE model_calls ADD COLUMN timing_quality TEXT;

-- 旧表把 usage_event_id 声明为 UNIQUE，导致重新计价覆盖旧 match；重建表以
-- 保留每次计价版本，同时用普通索引维持按事件查询性能。
ALTER TABLE pricing_matches RENAME TO pricing_matches_legacy;
CREATE TABLE pricing_matches (
    id                  TEXT PRIMARY KEY,
    usage_event_id      TEXT NOT NULL,
    pricing_rule_id     TEXT,
    pricing_snapshot_id TEXT,
    match_type          TEXT NOT NULL,
    calculated_at       TEXT NOT NULL,
    input_cost          INTEGER,
    output_cost         INTEGER,
    cache_read_cost     INTEGER,
    cache_write_cost    INTEGER,
    reasoning_cost      INTEGER,
    request_cost        INTEGER,
    total_cost          INTEGER
);
INSERT INTO pricing_matches SELECT * FROM pricing_matches_legacy;
DROP TABLE pricing_matches_legacy;
CREATE INDEX idx_pricing_matches_usage ON pricing_matches(usage_event_id, calculated_at);
