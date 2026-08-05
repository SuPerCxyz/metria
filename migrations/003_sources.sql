-- Hub 来源：clients / sources / projects / source_errors
CREATE TABLE IF NOT EXISTS clients (
    id             TEXT PRIMARY KEY,
    canonical_name TEXT NOT NULL UNIQUE,
    display_name   TEXT NOT NULL,
    category       TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id              TEXT PRIMARY KEY,
    canonical_key   TEXT NOT NULL,
    display_name    TEXT,
    path_hash       TEXT NOT NULL,
    git_remote_hash TEXT,
    metadata        TEXT NOT NULL DEFAULT '{}',
    first_seen_at   TEXT NOT NULL,
    last_seen_at    TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_projects_key ON projects(canonical_key);

CREATE TABLE IF NOT EXISTS sources (
    id                TEXT PRIMARY KEY,
    node_id           TEXT NOT NULL REFERENCES nodes(id),
    collector_id      TEXT NOT NULL REFERENCES collectors(id),
    client_id         TEXT NOT NULL REFERENCES clients(id),
    adapter_id        TEXT NOT NULL,
    adapter_version   TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    source_path_hash  TEXT NOT NULL,
    client_version    TEXT,
    status            TEXT NOT NULL DEFAULT 'active',
    capabilities      TEXT NOT NULL DEFAULT '[]',
    last_scan_at      TEXT,
    last_success_at   TEXT,
    last_event_at     TEXT,
    last_error        TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    UNIQUE(node_id, source_fingerprint)
);
CREATE INDEX IF NOT EXISTS idx_sources_client ON sources(client_id);

CREATE TABLE IF NOT EXISTS source_errors (
    id            TEXT PRIMARY KEY,
    source_id     TEXT NOT NULL REFERENCES sources(id),
    phase         TEXT NOT NULL,
    severity      TEXT NOT NULL,
    pattern       TEXT NOT NULL,
    sample_count  INTEGER NOT NULL DEFAULT 0,
    first_seen_at TEXT NOT NULL,
    last_seen_at  TEXT NOT NULL,
    last_message  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_source_errors_source ON source_errors(source_id);
