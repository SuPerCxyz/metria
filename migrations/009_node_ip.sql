-- 节点管理：为 nodes 增加 ip 列，记录节点所在公网主机的 IP/主机名，
-- 供 Hub 部署在内网时生成公网 Agent 可连通的安装命令（METRIA_HUB_URL）。
ALTER TABLE nodes ADD COLUMN ip TEXT;
