-- 节点级上报间隔（秒）；NULL 视为默认 60，安装命令生成时注入 METRIA_POLL_INTERVAL
ALTER TABLE nodes ADD COLUMN poll_interval_seconds INTEGER;
