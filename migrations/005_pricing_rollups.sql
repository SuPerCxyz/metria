-- Hub 价格、rollup 与杂项
CREATE TABLE IF NOT EXISTS pricing_catalogs (
    id                      TEXT PRIMARY KEY,
    name                    TEXT NOT NULL,
    kind                    TEXT NOT NULL,
    enabled                 INTEGER NOT NULL DEFAULT 1,
    base_url                TEXT,
    authentication_type     TEXT,
    refresh_interval_seconds INTEGER,
    priority                INTEGER NOT NULL DEFAULT 0,
    last_refresh_at         TEXT,
    last_success_at         TEXT,
    last_error              TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pricing_snapshots (
    id             TEXT PRIMARY KEY,
    catalog_id     TEXT NOT NULL REFERENCES pricing_catalogs(id),
    catalog_version TEXT,
    etag           TEXT,
    fetched_at     TEXT NOT NULL,
    effective_at   TEXT NOT NULL,
    content_hash   TEXT NOT NULL,
    record_count   INTEGER NOT NULL DEFAULT 0,
    status         TEXT NOT NULL,
    created_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pricing_rules (
    id               TEXT PRIMARY KEY,
    snapshot_id      TEXT,
    source           TEXT NOT NULL,
    channel          TEXT NOT NULL,
    provider_pattern TEXT NOT NULL,
    model_pattern    TEXT NOT NULL,
    client_pattern   TEXT NOT NULL DEFAULT '*',
    region_pattern   TEXT,
    service_tier     TEXT,
    currency         TEXT NOT NULL DEFAULT 'usd',
    unit             TEXT NOT NULL DEFAULT 'per_million_tokens',
    input_price      INTEGER,
    output_price     INTEGER,
    cache_read_price INTEGER,
    cache_write_price INTEGER,
    reasoning_price  INTEGER,
    request_price    INTEGER,
    effective_from   TEXT,
    effective_to     TEXT,
    priority         INTEGER NOT NULL DEFAULT 0,
    enabled          INTEGER NOT NULL DEFAULT 1,
    metadata         TEXT NOT NULL DEFAULT '{}',
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_rules_model ON pricing_rules(model_pattern);

CREATE TABLE IF NOT EXISTS pricing_matches (
    id                  TEXT PRIMARY KEY,
    usage_event_id      TEXT NOT NULL UNIQUE,
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

CREATE TABLE IF NOT EXISTS hourly_rollups (
    bucket                       TEXT NOT NULL,
    node_id                      TEXT NOT NULL,
    collector_id                 TEXT NOT NULL,
    client_id                    TEXT NOT NULL,
    source_id                    TEXT NOT NULL,
    project_id                   TEXT NOT NULL DEFAULT '',
    provider                     TEXT NOT NULL DEFAULT '',
    model                        TEXT NOT NULL DEFAULT '',
    usage_source                 TEXT NOT NULL DEFAULT '',
    usage_granularity            TEXT NOT NULL DEFAULT '',
    pricing_source               TEXT NOT NULL DEFAULT '',
    traffic_estimation_source    TEXT NOT NULL DEFAULT '',
    traffic_confidence_level     TEXT NOT NULL DEFAULT '',
    input_tokens                 INTEGER NOT NULL DEFAULT 0,
    output_tokens                INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens            INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens           INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens             INTEGER NOT NULL DEFAULT 0,
    reported_cost                INTEGER NOT NULL DEFAULT 0,
    calculated_cost              INTEGER NOT NULL DEFAULT 0,
    estimated_cost               INTEGER NOT NULL DEFAULT 0,
    estimated_request_bytes      INTEGER NOT NULL DEFAULT 0,
    estimated_response_bytes     INTEGER NOT NULL DEFAULT 0,
    estimated_total_bytes        INTEGER NOT NULL DEFAULT 0,
    estimated_lower_bound_bytes  INTEGER NOT NULL DEFAULT 0,
    estimated_upper_bound_bytes  INTEGER NOT NULL DEFAULT 0,
    session_count                INTEGER NOT NULL DEFAULT 0,
    model_call_count             INTEGER NOT NULL DEFAULT 0,
    turn_count                   INTEGER NOT NULL DEFAULT 0,
    message_count                INTEGER NOT NULL DEFAULT 0,
    tool_call_count              INTEGER NOT NULL DEFAULT 0,
    subagent_count               INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket, node_id, collector_id, client_id, source_id, project_id, provider, model, usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level)
);
CREATE INDEX IF NOT EXISTS idx_hourly_bucket ON hourly_rollups(bucket);

CREATE TABLE IF NOT EXISTS daily_rollups (
    bucket                       TEXT NOT NULL,
    node_id                      TEXT NOT NULL,
    collector_id                 TEXT NOT NULL,
    client_id                    TEXT NOT NULL,
    source_id                    TEXT NOT NULL,
    project_id                   TEXT NOT NULL DEFAULT '',
    provider                     TEXT NOT NULL DEFAULT '',
    model                        TEXT NOT NULL DEFAULT '',
    usage_source                 TEXT NOT NULL DEFAULT '',
    usage_granularity            TEXT NOT NULL DEFAULT '',
    pricing_source               TEXT NOT NULL DEFAULT '',
    traffic_estimation_source    TEXT NOT NULL DEFAULT '',
    traffic_confidence_level     TEXT NOT NULL DEFAULT '',
    input_tokens                 INTEGER NOT NULL DEFAULT 0,
    output_tokens                INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens            INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens           INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens             INTEGER NOT NULL DEFAULT 0,
    reported_cost                INTEGER NOT NULL DEFAULT 0,
    calculated_cost              INTEGER NOT NULL DEFAULT 0,
    estimated_cost               INTEGER NOT NULL DEFAULT 0,
    estimated_request_bytes      INTEGER NOT NULL DEFAULT 0,
    estimated_response_bytes     INTEGER NOT NULL DEFAULT 0,
    estimated_total_bytes        INTEGER NOT NULL DEFAULT 0,
    estimated_lower_bound_bytes  INTEGER NOT NULL DEFAULT 0,
    estimated_upper_bound_bytes  INTEGER NOT NULL DEFAULT 0,
    session_count                INTEGER NOT NULL DEFAULT 0,
    model_call_count             INTEGER NOT NULL DEFAULT 0,
    turn_count                   INTEGER NOT NULL DEFAULT 0,
    message_count                INTEGER NOT NULL DEFAULT 0,
    tool_call_count              INTEGER NOT NULL DEFAULT 0,
    subagent_count               INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket, node_id, collector_id, client_id, source_id, project_id, provider, model, usage_source, usage_granularity, pricing_source, traffic_estimation_source, traffic_confidence_level)
);
CREATE INDEX IF NOT EXISTS idx_daily_bucket ON daily_rollups(bucket);

CREATE TABLE IF NOT EXISTS upload_batches (
    batch_id    TEXT PRIMARY KEY,
    node_id     TEXT,
    collector_id TEXT,
    received_at TEXT NOT NULL,
    status      TEXT NOT NULL,
    event_count INTEGER NOT NULL DEFAULT 0,
    bytes       INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS share_links (
    id          TEXT PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    kind        TEXT NOT NULL,
    target_id   TEXT NOT NULL,
    created_by  TEXT,
    expires_at  TEXT,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS share_audits (
    id         TEXT PRIMARY KEY,
    slug       TEXT NOT NULL,
    ip         TEXT,
    viewed_at  TEXT NOT NULL
);

-- 内置价格目录种子
INSERT OR IGNORE INTO pricing_catalogs (id, name, kind, enabled, priority, created_at, updated_at)
VALUES ('builtin-1', '内置价格目录', 'builtin', 1, 0, '2026-08-05T00:00:00Z', '2026-08-05T00:00:00Z');
