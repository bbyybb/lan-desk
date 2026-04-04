use crate::state::AppState;
use lan_desk_protocol::message::Message;
use tauri::State;

#[tauri::command]
pub async fn send_chat_message(
    text: String,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // 尝试通过控制端连接通道发送（控制端 → 被控端）
    {
        let conn = state.conn.read().await;
        let cid = connection_id.unwrap_or_default();
        if let Some(tx) = conn.get_input_tx(&cid) {
            let msg = Message::ChatMessage {
                text,
                sender: hostname,
                timestamp_ms,
            };
            return tx.send(msg).await.map_err(|e| format!("发送失败: {}", e));
        }
    }

    // 回退：通过被控端服务器广播通道发送（被控端 → 远程控制端）
    #[cfg(feature = "desktop")]
    {
        let server = state.server.read().await;
        if let Some(ref tx) = server.host_chat_tx {
            let _ = tx.send((text, hostname, timestamp_ms));
            return Ok(());
        }
    }

    Err("[ERR_NOT_CONNECTED] No active connection or server".to_string())
}
