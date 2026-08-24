-- 节点管理：为 nodes 增加 hub_url 列，供前端创建/编辑节点时记录目标机可访问的 Hub 地址，
-- 安装命令使用该地址而非依赖 Host 头（反代/容器场景更可靠）。
ALTER TABLE nodes ADD COLUMN hub_url TEXT;
