-- Hub Traffic Profile 与自动学习样本
CREATE TABLE IF NOT EXISTS traffic_profiles (
    id                          TEXT PRIMARY KEY,
    source                      TEXT NOT NULL,
    client_pattern              TEXT NOT NULL,
    client_version_pattern      TEXT NOT NULL DEFAULT '*',
    provider_pattern            TEXT NOT NULL DEFAULT '*',
    model_pattern               TEXT NOT NULL DEFAULT '*',
    content_profile             TEXT NOT NULL,
    direction                   TEXT NOT NULL,
    streaming                   INTEGER,
    input_bytes_per_token_p50   REAL NOT NULL,
    input_bytes_per_token_p75   REAL NOT NULL,
    input_bytes_per_token_p90   REAL NOT NULL,
    output_bytes_per_token_p50  REAL NOT NULL,
    output_bytes_per_token_p75  REAL NOT NULL,
    output_bytes_per_token_p90  REAL NOT NULL,
    fixed_request_bytes         INTEGER NOT NULL DEFAULT 0,
    fixed_response_bytes        INTEGER NOT NULL DEFAULT 0,
    http_overhead_ratio         REAL NOT NULL DEFAULT 0.05,
    transport_overhead_ratio    REAL NOT NULL DEFAULT 0.10,
    cache_read_transport_factor REAL NOT NULL DEFAULT 0.8,
    cache_write_transport_factor REAL NOT NULL DEFAULT 1.0,
    sample_count                INTEGER NOT NULL DEFAULT 0,
    confidence                  REAL NOT NULL DEFAULT 0.3,
    effective_from              TEXT,
    effective_to                TEXT,
    version                     INTEGER NOT NULL DEFAULT 1,
    enabled                     INTEGER NOT NULL DEFAULT 1,
    created_at                  TEXT NOT NULL,
    updated_at                  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tp_match ON traffic_profiles(client_pattern, model_pattern, direction);

CREATE TABLE IF NOT EXISTS traffic_profile_samples (
    id                      TEXT PRIMARY KEY,
    client                  TEXT NOT NULL,
    client_version          TEXT,
    provider                TEXT,
    model                   TEXT,
    content_profile         TEXT NOT NULL,
    direction               TEXT NOT NULL,
    token_count             INTEGER NOT NULL,
    payload_bytes           INTEGER NOT NULL,
    bytes_per_token         REAL NOT NULL,
    reconstruction_quality  TEXT NOT NULL,
    source_hash             TEXT NOT NULL,
    created_at              TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tps_match ON traffic_profile_samples(client, provider, model, direction);
