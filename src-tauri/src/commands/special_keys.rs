use crate::state::AppState;
use lan_desk_protocol::message::{Message, SpecialKeyType};
use tauri::State;

#[tauri::command]
pub async fn send_special_key(
    key: String,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    let tx = conn.get_input_tx(&cid).ok_or("[ERR_NOT_CONNECTED]")?;
    let key_type = match key.as_str() {
        "CtrlAltDel" => SpecialKeyType::CtrlAltDel,
        "AltTab" => SpecialKeyType::AltTab,
        "AltF4" => SpecialKeyType::AltF4,
        "PrintScreen" => SpecialKeyType::PrintScreen,
        "WinKey" => SpecialKeyType::WinKey,
        "WinL" => SpecialKeyType::WinL,
        "CtrlEsc" => SpecialKeyType::CtrlEsc,
        _ => return Err(format!("未知的特殊按键: {}", key)),
    };
    tx.send(Message::SpecialKey { key: key_type })
        .await
        .map_err(|e| format!("{}", e))
}
