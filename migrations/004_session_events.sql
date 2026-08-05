-- Hub 事件：sessions / turns / messages / model_calls / usage_events /
-- tool_events / subagent_relations / traffic_estimates
CREATE TABLE IF NOT EXISTS sessions (
    id                         TEXT PRIMARY KEY,
    source_session_id          TEXT NOT NULL,
    node_id                    TEXT NOT NULL,
    collector_id               TEXT NOT NULL,
    source_id                  TEXT NOT NULL,
    client_id                  TEXT NOT NULL,
    project_id                 TEXT,
    parent_session_id          TEXT,
    title                      TEXT,
    working_directory_hash     TEXT,
    started_at                 TEXT NOT NULL,
    ended_at                   TEXT,
    last_activity_at           TEXT,
    provider_raw               TEXT,
    provider_normalized        TEXT,
    primary_model_raw          TEXT,
    primary_model_normalized   TEXT,
    status                     TEXT NOT NULL DEFAULT 'unknown',
    message_count              INTEGER NOT NULL DEFAULT 0,
    tool_call_count            INTEGER NOT NULL DEFAULT 0,
    subagent_count             INTEGER NOT NULL DEFAULT 0,
    model_call_count           INTEGER NOT NULL DEFAULT 0,
    input_tokens               INTEGER,
    output_tokens              INTEGER,
    cache_read_tokens          INTEGER,
    cache_write_tokens         INTEGER,
    reasoning_tokens           INTEGER,
    reported_cost_micro_usd    INTEGER,
    calculated_cost_micro_usd  INTEGER,
    estimated_cost_micro_usd   INTEGER,
    estimated_request_bytes    INTEGER,
    estimated_response_bytes   INTEGER,
    estimated_total_bytes      INTEGER,
    traffic_confidence         REAL,
    content_available          INTEGER NOT NULL DEFAULT 0,
    created_at                 TEXT NOT NULL,
    updated_at                 TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(started_at);
CREATE INDEX IF NOT EXISTS idx_sessions_node ON sessions(node_id);
CREATE INDEX IF NOT EXISTS idx_sessions_client ON sessions(client_id);
CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source_id);

CREATE TABLE IF NOT EXISTS turns (
    id                 TEXT PRIMARY KEY,
    session_id         TEXT NOT NULL,
    source_turn_id     TEXT,
    sequence           INTEGER NOT NULL,
    role               TEXT NOT NULL,
    started_at         TEXT NOT NULL,
    ended_at           TEXT,
    provider_raw       TEXT,
    provider_normalized TEXT,
    model_raw          TEXT,
    model_normalized   TEXT,
    finish_reason      TEXT,
    usage_source       TEXT NOT NULL,
    usage_granularity  TEXT NOT NULL,
    usage_confidence   REAL,
    created_at         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id);

CREATE TABLE IF NOT EXISTS messages (
    id             TEXT PRIMARY KEY,
    turn_id        TEXT,
    session_id     TEXT NOT NULL,
    source_message_id TEXT,
    sequence       INTEGER NOT NULL,
    role           TEXT NOT NULL,
    content_type   TEXT NOT NULL,
    content        TEXT,
    content_hash   TEXT,
    content_length INTEGER NOT NULL DEFAULT 0,
    utf8_bytes     INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    redacted       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, sequence);

CREATE TABLE IF NOT EXISTS model_calls (
    id                        TEXT PRIMARY KEY,
    source_call_id            TEXT,
    node_id                   TEXT NOT NULL,
    collector_id              TEXT NOT NULL,
    client_id                 TEXT NOT NULL,
    source_id                 TEXT NOT NULL,
    project_id                TEXT,
    session_id                TEXT NOT NULL,
    turn_id                   TEXT,
    provider_raw              TEXT,
    provider_normalized       TEXT,
    model_raw                 TEXT,
    model_normalized          TEXT,
    started_at                TEXT NOT NULL,
    first_response_at         TEXT,
    completed_at              TEXT,
    duration_ms               INTEGER,
    status                    TEXT NOT NULL,
    status_code               INTEGER,
    streaming                 INTEGER NOT NULL DEFAULT 0,
    stream_completed          INTEGER,
    client_aborted            INTEGER NOT NULL DEFAULT 0,
    retry_count               INTEGER NOT NULL DEFAULT 0,
    call_granularity          TEXT NOT NULL,
    input_tokens              INTEGER,
    output_tokens             INTEGER,
    cache_read_tokens         INTEGER,
    cache_write_tokens        INTEGER,
    reasoning_tokens          INTEGER,
    reported_cost_micro_usd   INTEGER,
    calculated_cost_micro_usd INTEGER,
    estimated_cost_micro_usd  INTEGER,
    usage_event_id            TEXT,
    traffic_estimate_id       TEXT,
    created_at                TEXT NOT NULL,
    updated_at                TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_calls_time ON model_calls(started_at);
CREATE INDEX IF NOT EXISTS idx_calls_session ON model_calls(session_id);
CREATE INDEX IF NOT EXISTS idx_calls_node ON model_calls(node_id);
CREATE INDEX IF NOT EXISTS idx_calls_model ON model_calls(model_normalized);

CREATE TABLE IF NOT EXISTS usage_events (
    event_id           TEXT PRIMARY KEY,
    schema_version     INTEGER NOT NULL,
    node_id            TEXT NOT NULL,
    collector_id       TEXT NOT NULL,
    source_id          TEXT NOT NULL,
    client_id          TEXT NOT NULL,
    adapter_id         TEXT NOT NULL,
    adapter_version    TEXT NOT NULL,
    session_id         TEXT,
    turn_id            TEXT,
    model_call_id      TEXT,
    timestamp          TEXT NOT NULL,
    provider_raw       TEXT,
    provider_normalized TEXT,
    model_raw          TEXT,
    model_normalized   TEXT,
    input_tokens       INTEGER,
    output_tokens      INTEGER,
    cache_read_tokens  INTEGER,
    cache_write_tokens INTEGER,
    reasoning_tokens   INTEGER,
    reported_cost_micro_usd    INTEGER,
    calculated_cost_micro_usd  INTEGER,
    estimated_cost_micro_usd   INTEGER,
    pricing_rule_id    TEXT,
    pricing_snapshot_id TEXT,
    usage_source       TEXT NOT NULL,
    usage_granularity  TEXT NOT NULL,
    usage_confidence   REAL
);
CREATE INDEX IF NOT EXISTS idx_usage_time ON usage_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_usage_node ON usage_events(node_id);
CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_events(model_normalized);
CREATE INDEX IF NOT EXISTS idx_usage_call ON usage_events(model_call_id);

CREATE TABLE IF NOT EXISTS tool_events (
    id                    TEXT PRIMARY KEY,
    session_id            TEXT NOT NULL,
    model_call_id         TEXT,
    turn_id               TEXT,
    source_tool_id        TEXT,
    name                  TEXT NOT NULL,
    tool_type             TEXT NOT NULL,
    status                TEXT NOT NULL,
    input_content_hash    TEXT,
    output_content_hash   TEXT,
    input_length          INTEGER NOT NULL DEFAULT 0,
    output_length         INTEGER NOT NULL DEFAULT 0,
    started_at            TEXT NOT NULL,
    completed_at          TEXT,
    duration_ms           INTEGER,
    error                 TEXT,
    created_at            TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tools_session ON tool_events(session_id);

CREATE TABLE IF NOT EXISTS subagent_relations (
    id                   TEXT PRIMARY KEY,
    session_id           TEXT NOT NULL,
    parent_model_call_id TEXT,
    child_session_id     TEXT NOT NULL,
    relation             TEXT NOT NULL,
    created_at           TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS traffic_estimates (
    id                              TEXT PRIMARY KEY,
    model_call_id                   TEXT NOT NULL,
    node_id                         TEXT NOT NULL,
    client_id                       TEXT NOT NULL,
    session_id                      TEXT,
    turn_id                         TEXT,
    provider                        TEXT,
    model                           TEXT,
    request_payload_bytes           INTEGER,
    response_payload_bytes          INTEGER,
    estimated_request_http_bytes    INTEGER,
    estimated_response_http_bytes   INTEGER,
    estimated_request_wire_bytes    INTEGER,
    estimated_response_wire_bytes   INTEGER,
    estimated_total_wire_bytes      INTEGER,
    lower_bound_bytes               INTEGER,
    upper_bound_bytes               INTEGER,
    estimation_source               TEXT NOT NULL,
    context_transport_mode          TEXT NOT NULL,
    cache_transport_behavior        TEXT NOT NULL,
    request_reconstruction_quality  TEXT NOT NULL,
    response_reconstruction_quality TEXT NOT NULL,
    profile_id                      TEXT,
    profile_version                 INTEGER,
    confidence                      REAL,
    calculated_at                   TEXT NOT NULL,
    created_at                      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_traffic_call ON traffic_estimates(model_call_id);
CREATE INDEX IF NOT EXISTS idx_traffic_time ON traffic_estimates(calculated_at);
