//! 备份与恢复：SQLite 在线备份（VACUUM INTO 一致性快照）+ zstd 压缩。

use std::path::PathBuf;

/// 执行备份：将 Hub 数据库导出为一致性快照文件（可选 zstd 压缩）。
pub fn backup(database_url: &str, out: Option<&str>) -> Result<(), String> {
    let path = sqlite_path(database_url)?;
    let conn = metria_storage::open(&path, &metria_storage::DbOptions::default())
        .map_err(|e| format!("打开数据库失败: {e}"))?;
    let dir = std::env::temp_dir().join(format!("metria-backup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let snapshot = dir.join("snapshot.db");
    // VACUUM INTO 生成一致性快照（WAL 安全）
    conn.execute(&format!("VACUUM INTO '{}'", snapshot.display()), [])
        .map_err(|e| format!("备份失败: {e}"))?;

    let out_path = out.map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(format!(
            "metria-backup-{}.db.zst",
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        ))
    });
    let raw = std::fs::read(&snapshot).map_err(|e| e.to_string())?;
    if out_path.extension().and_then(|e| e.to_str()) == Some("zst") {
        let compressed = zstd::encode_all(&raw[..], 3).map_err(|e| format!("压缩失败: {e}"))?;
        std::fs::write(&out_path, compressed).map_err(|e| e.to_string())?;
    } else {
        std::fs::write(&out_path, raw).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!("备份完成: {}", out_path.display());
    Ok(())
}

/// 执行恢复：将备份文件恢复到 Hub 数据库路径（需先停止 Hub）。
pub fn restore(database_url: &str, input: &str) -> Result<(), String> {
    let path = sqlite_path(database_url)?;
    let raw = std::fs::read(input).map_err(|e| format!("读取备份失败: {e}"))?;
    let data = if input.ends_with(".zst") {
        zstd::decode_all(&raw[..]).map_err(|e| format!("解压失败: {e}"))?
    } else {
        raw
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, data).map_err(|e| format!("写入失败: {e}"))?;
    // 清理可能残留的 WAL 文件，避免新旧数据混淆
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    println!("恢复完成: {}", path.display());
    Ok(())
}

fn sqlite_path(database_url: &str) -> Result<PathBuf, String> {
    let rest = database_url
        .strip_prefix("sqlite://")
        .ok_or("仅支持 sqlite:// 协议")?;
    Ok(PathBuf::from(rest))
}
