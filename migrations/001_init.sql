-- 基础元数据表。后续 Hub schema 迁移以更高版本号追加。
CREATE TABLE IF NOT EXISTS server_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
