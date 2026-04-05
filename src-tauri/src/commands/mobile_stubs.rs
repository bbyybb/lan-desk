//! 移动端 stub 命令
//!
//! 桌面端专属功能在移动端不可用，这些 stub 返回友好的错误信息，
//! 避免前端调用时出现 "command not found" 异常。

const NOT_SUPPORTED: &str = "This feature is not available on mobile devices";

#[tauri::command]
pub async fn start_server() -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn stop_server() -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn set_shell_enabled(_enabled: bool) -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn set_idle_timeout(_minutes: u64) -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn set_lock_on_disconnect(_enabled: bool) -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn start_shell() -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn send_shell_input(_data: String) -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn resize_shell(_cols: u16, _rows: u16) -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn close_shell() -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn list_monitors() -> Result<Vec<()>, String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn switch_monitor(_index: u32) -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn toggle_screen_blank() -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn remote_reboot() -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}

#[tauri::command]
pub async fn remote_lock_screen() -> Result<(), String> {
    Err(NOT_SUPPORTED.to_string())
}
