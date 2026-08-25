-- Pull 模式：节点 Agent 地址与节点级加密 token、拉取状态
ALTER TABLE nodes ADD COLUMN agent_url TEXT;
ALTER TABLE nodes ADD COLUMN node_token_enc TEXT;
ALTER TABLE nodes ADD COLUMN last_pull_at TEXT;
ALTER TABLE nodes ADD COLUMN last_pull_error TEXT;
