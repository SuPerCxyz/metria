-- Hub 身份：users / nodes / collectors / collector_tokens
CREATE TABLE IF NOT EXISTS users (
    id             TEXT PRIMARY KEY,
    username       TEXT NOT NULL UNIQUE,
    password_hash  TEXT NOT NULL,
    must_change_password INTEGER NOT NULL DEFAULT 1,
    role           TEXT NOT NULL DEFAULT 'admin',
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS nodes (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    description   TEXT,
    labels        TEXT NOT NULL DEFAULT '[]',
    platform      TEXT,
    architecture  TEXT,
    timezone      TEXT,
    first_seen_at TEXT NOT NULL,
    last_seen_at  TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'unknown',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS collectors (
    id                   TEXT PRIMARY KEY,
    node_id              TEXT NOT NULL REFERENCES nodes(id),
    agent_version        TEXT NOT NULL,
    protocol_version     INTEGER NOT NULL,
    container_image      TEXT,
    started_at           TEXT NOT NULL,
    last_heartbeat_at    TEXT NOT NULL,
    last_upload_at       TEXT,
    status               TEXT NOT NULL DEFAULT 'unknown',
    spool_pending_events INTEGER NOT NULL DEFAULT 0,
    spool_size_bytes     INTEGER NOT NULL DEFAULT 0,
    clock_skew_seconds   INTEGER NOT NULL DEFAULT 0,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_collectors_node ON collectors(node_id);

CREATE TABLE IF NOT EXISTS collector_tokens (
    id            TEXT PRIMARY KEY,
    collector_id  TEXT NOT NULL REFERENCES collectors(id),
    token_hash    TEXT NOT NULL,
    label         TEXT,
    status        TEXT NOT NULL DEFAULT 'active',
    created_at    TEXT NOT NULL,
    revoked_at    TEXT
);
CREATE INDEX IF NOT EXISTS idx_tokens_collector ON collector_tokens(collector_id);
