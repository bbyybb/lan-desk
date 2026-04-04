use tauri::State;

use lan_desk_protocol::message::Message;

use crate::state::AppState;

// ──────────────── 远程终端 ────────────────

#[tauri::command]
pub async fn start_shell(
    cols: u16,
    rows: u16,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        tx.send(Message::ShellStart { cols, rows })
            .await
            .map_err(|e| format!("[ERR_SEND_SHELL] Failed to send ShellStart: {}", e))?;
    } else {
        return Err("[ERR_NOT_CONNECTED] Not connected".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn send_shell_input(
    data: Vec<u8>,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        let _ = tx.try_send(Message::ShellData { data });
    }
    Ok(())
}

#[tauri::command]
pub async fn resize_shell(
    cols: u16,
    rows: u16,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        let _ = tx.try_send(Message::ShellResize { cols, rows });
    }
    Ok(())
}

#[tauri::command]
pub async fn close_shell(
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        let _ = tx.send(Message::ShellClose).await;
    }
    Ok(())
}
