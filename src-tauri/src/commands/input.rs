use tauri::State;

use lan_desk_protocol::message::{Message, MouseBtn};

use crate::state::AppState;

// ──────────────── 鼠标/键盘输入转发 ────────────────

#[tauri::command]
pub async fn send_mouse_move(
    x: f64,
    y: f64,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        if tx.try_send(Message::MouseMove { x, y }).is_err() {
            tracing::trace!("鼠标移动事件发送失败（channel 已满或关闭）");
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn send_mouse_button(
    button: String,
    pressed: bool,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let btn = match button.as_str() {
        "left" => MouseBtn::Left,
        "right" => MouseBtn::Right,
        "middle" => MouseBtn::Middle,
        _ => return Ok(()), // 未知按钮类型，忽略
    };
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        if tx
            .try_send(Message::MouseButton {
                button: btn,
                pressed,
            })
            .is_err()
        {
            tracing::trace!("鼠标按键事件发送失败（channel 已满或关闭）");
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn send_mouse_scroll(
    dx: f64,
    dy: f64,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        if tx.try_send(Message::MouseScroll { dx, dy }).is_err() {
            tracing::trace!("鼠标滚轮事件发送失败（channel 已满或关闭）");
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn send_key_event(
    code: String,
    pressed: bool,
    modifiers: u8,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        if tx
            .try_send(Message::KeyEvent {
                code,
                pressed,
                modifiers,
            })
            .is_err()
        {
            tracing::trace!("键盘事件发送失败（channel 已满或关闭）");
        }
    }
    Ok(())
}

// ──────────────── 带宽控制 ────────────────

#[tauri::command]
pub async fn set_bandwidth_limit(
    mbps: f64,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let bytes_per_sec = if mbps <= 0.0 {
        0
    } else {
        (mbps * 1_000_000.0 / 8.0) as u64
    };
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        if tx
            .try_send(Message::SetBandwidthLimit { bytes_per_sec })
            .is_err()
        {
            tracing::trace!("带宽限制事件发送失败（channel 已满或关闭）");
        }
    }
    Ok(())
}

// ──────────────── 捕获设置 ────────────────

/// 实时应用捕获参数（画质、帧率、端口），端口变更需要重启才能生效
#[tauri::command]
pub async fn apply_capture_settings(
    jpeg_quality: u8,
    max_fps: u32,
    port: u16,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // 通过 input channel 向被控端会话发送设置更新
    {
        let conn = state.conn.read().await;
        let cid = connection_id.unwrap_or_default();
        if let Some(tx) = conn.get_input_tx(&cid) {
            if tx
                .try_send(Message::CaptureSettings {
                    jpeg_quality,
                    max_fps,
                })
                .is_err()
            {
                tracing::trace!("捕获设置事件发送失败（channel 已满或关闭）");
            }
        }
    }
    // 端口变更保存到状态，下次启动服务器时生效
    #[cfg(feature = "desktop")]
    {
        state.server.write().await.port = port;
    }
    tracing::info!(
        "捕获设置已更新: 画质={}, 帧率={}, 端口={} (端口需重启生效)",
        jpeg_quality,
        max_fps,
        port
    );
    Ok(())
}

// ──────────────── 多显示器 ────────────────

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn list_monitors() -> Result<Vec<lan_desk_protocol::message::MonitorInfo>, String> {
    lan_desk_capture::list_monitors()
        .map_err(|e| format!("[ERR_LIST_MONITORS] Failed to enumerate monitors: {}", e))
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn switch_monitor(
    index: u32,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        if tx.try_send(Message::SwitchMonitor { index }).is_err() {
            tracing::trace!("切换显示器事件发送失败（channel 已满或关闭）");
        }
    }
    Ok(())
}
