-- 报告配置（键值存储）与发送记录。
-- settings.value 存 JSON 或标量；SMTP 密码以明文存本地库，读取接口一律不回传。
CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS report_sends (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    period     TEXT NOT NULL,
    channel    TEXT NOT NULL,
    status     TEXT NOT NULL,
    detail     TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_report_sends_created ON report_sends(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_report_sends_period_kind ON report_sends(kind, period);
