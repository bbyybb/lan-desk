use tauri::State;

use lan_desk_protocol::message::Message;

use super::file_transfer::next_transfer_id;
use crate::state::AppState;

/// 请求远程文件列表
#[tauri::command]
pub async fn request_file_list(
    path: String,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    let tx = conn
        .get_input_tx(&cid)
        .ok_or("[ERR_NOT_CONNECTED] Not connected")?;

    static REQUEST_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let request_id = REQUEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    tx.send(Message::FileListRequest { path, request_id })
        .await
        .map_err(|_| "[ERR_SEND_FAILED] Failed to send message".to_string())
}

/// 请求下载远程文件
#[tauri::command]
pub async fn download_remote_file(
    path: String,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    let tx = conn
        .get_input_tx(&cid)
        .ok_or("[ERR_NOT_CONNECTED] Not connected")?;

    let transfer_id = next_transfer_id();

    tx.send(Message::FileDownloadRequest { path, transfer_id })
        .await
        .map_err(|_| "[ERR_SEND_FAILED] Failed to send message".to_string())?;

    Ok(transfer_id)
}

/// 请求下载远程目录（反向目录传输）
#[tauri::command]
pub async fn download_remote_directory(
    path: String,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    let tx = conn
        .get_input_tx(&cid)
        .ok_or("[ERR_NOT_CONNECTED] Not connected")?;

    let transfer_id = next_transfer_id();

    tx.send(Message::DirectoryDownloadRequest { path, transfer_id })
        .await
        .map_err(|_| "[ERR_SEND_FAILED] Failed to send message".to_string())?;

    Ok(transfer_id)
}
