-- 用户资料：头像采用轻量文字与颜色键，不保存图片或外部 URL。
ALTER TABLE users ADD COLUMN display_name TEXT;
ALTER TABLE users ADD COLUMN avatar_text TEXT;
ALTER TABLE users ADD COLUMN avatar_color TEXT NOT NULL DEFAULT 'indigo';
