use crate::state::AppState;
use lan_desk_protocol::message::Message;
use tauri::State;

#[tauri::command]
pub async fn remote_reboot(
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    let tx = conn.get_input_tx(&cid).ok_or("[ERR_NOT_CONNECTED]")?;
    tx.send(Message::RemoteReboot)
        .await
        .map_err(|e| format!("{}", e))
}

#[tauri::command]
pub async fn remote_lock_screen(
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    let tx = conn.get_input_tx(&cid).ok_or("[ERR_NOT_CONNECTED]")?;
    tx.send(Message::LockScreen)
        .await
        .map_err(|e| format!("{}", e))
}
