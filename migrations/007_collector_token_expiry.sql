-- Collector token 有效期：默认 7 天。
-- 已存在的 token 标记为「永久」（expires_at IS NULL），新注册 token 写入有效期。
ALTER TABLE collector_tokens ADD COLUMN expires_at TEXT;

CREATE INDEX IF NOT EXISTS idx_tokens_expiry ON collector_tokens(status, expires_at);
