-- 发送历史只保留最新 30 条，升级时清理既有旧记录。
DELETE FROM report_sends
WHERE rowid NOT IN (
    SELECT rowid
    FROM report_sends
    ORDER BY created_at DESC, rowid DESC
    LIMIT 30
);
