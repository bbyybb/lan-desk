use std::path::PathBuf;

/// 检测是否为便携模式（exe 同目录存在 .portable 标记文件）
pub fn is_portable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(".portable").exists()))
        .unwrap_or(false)
}

/// 获取数据目录：便携模式用 exe 同级 data/，否则用系统默认
pub fn data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join(".portable").exists() {
                let data = dir.join("data");
                std::fs::create_dir_all(&data).ok();
                return data;
            }
        }
    }
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lan-desk")
}
