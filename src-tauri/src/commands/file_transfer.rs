use tauri::{AppHandle, Emitter, State};

use lan_desk_protocol::message::Message;

use crate::state::AppState;

/// 全局唯一传输 ID 计数器（避免多个计数器范围重叠导致数据错乱）
static GLOBAL_TRANSFER_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

pub(crate) fn next_transfer_id() -> u32 {
    GLOBAL_TRANSFER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

// ──────────────── 传输控制 ────────────────

/// 取消正在进行的文件传输
#[tauri::command]
pub async fn cancel_transfer(transfer_id: u32, state: State<'_, AppState>) -> Result<bool, String> {
    let mut conn = state.conn.write().await;
    Ok(conn.cancel_transfer(transfer_id))
}

/// 查询路径类型（文件/目录），用于拖拽上传判断
#[tauri::command]
pub async fn stat_path(path: String) -> Result<serde_json::Value, String> {
    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(|e| format!("[ERR_FILE_METADATA] {}", e))?;
    Ok(serde_json::json!({ "is_dir": meta.is_dir() }))
}

// ──────────────── 目录传输 ────────────────

/// 递归发送目录到远程设备
#[tauri::command]
pub async fn send_directory(
    app: AppHandle,
    dir_path: String,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    use std::path::Path;

    let path = Path::new(&dir_path);
    if !path.exists() || !path.is_dir() {
        return Err(format!(
            "[ERR_FILE_NOT_FOUND] Directory not found: {}",
            dir_path
        ));
    }

    let dir_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown_dir".to_string());

    // 递归收集文件列表
    fn collect_entries(base: &Path, current: &Path) -> std::io::Result<Vec<(String, bool, u64)>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let relative = entry
                .path()
                .strip_prefix(base)
                .unwrap_or(entry.path().as_path())
                .to_string_lossy()
                .to_string();
            let meta = entry.metadata()?;
            if meta.is_dir() {
                entries.push((relative.clone(), true, 0));
                entries.extend(collect_entries(base, &entry.path())?);
            } else {
                entries.push((relative, false, meta.len()));
            }
        }
        Ok(entries)
    }

    let dir_entries = collect_entries(path, path)
        .map_err(|e| format!("[ERR_FILE_METADATA] Failed to scan directory: {}", e))?;

    let total_files = dir_entries.iter().filter(|(_, is_dir, _)| !*is_dir).count() as u32;
    let total_size: u64 = dir_entries.iter().map(|(_, _, size)| size).sum();

    let transfer_id = next_transfer_id();

    // 注册取消令牌
    let conn_arc = state.conn.clone();
    let token = {
        let mut conn = state.conn.write().await;
        let cid = connection_id.as_deref().unwrap_or("");
        let tx = conn
            .get_input_tx(cid)
            .ok_or("[ERR_NOT_CONNECTED] Not connected")?;

        // 发送目录传输开始
        tx.send(Message::DirectoryTransferStart {
            transfer_id,
            base_path: dir_name.clone(),
            total_files,
            total_size,
        })
        .await
        .map_err(|_| "[ERR_SEND_FAILED] Failed to send message")?;

        // 发送所有目录条目
        for (relative, is_dir, size) in &dir_entries {
            let _ = tx
                .send(Message::DirectoryEntry {
                    transfer_id,
                    relative_path: relative.clone(),
                    is_dir: *is_dir,
                    size: *size,
                })
                .await;
        }

        conn.register_transfer(transfer_id)
    };

    let tx_clone = {
        let conn = state.conn.read().await;
        let cid = connection_id.as_deref().unwrap_or("");
        conn.get_input_tx(cid)
            .cloned()
            .ok_or("[ERR_NOT_CONNECTED] Not connected")?
    };
    let base_path = path.to_path_buf();

    // 逐个发送文件（支持取消）
    tokio::spawn(async move {
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncReadExt;

        const CHUNK_SIZE: usize = 64 * 1024;

        // 使用全局统一计数器
        let mut files_sent = 0u32;

        'outer: for (relative, is_dir, _size) in &dir_entries {
            if *is_dir {
                continue;
            }
            if token.is_cancelled() {
                break;
            }

            let file_path = base_path.join(relative);
            let file_transfer_id = next_transfer_id();

            let meta = match tokio::fs::metadata(&file_path).await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("跳过文件 {:?}: {}", file_path, e);
                    continue;
                }
            };
            let file_size = meta.len();
            let filename = file_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let _ = tx_clone
                .send(Message::FileTransferStart {
                    filename: filename.clone(),
                    size: file_size,
                    transfer_id: file_transfer_id,
                })
                .await;

            let file = match tokio::fs::File::open(&file_path).await {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!("打开文件失败: {}", e);
                    continue;
                }
            };

            let mut reader = tokio::io::BufReader::new(file);
            let mut hasher = Sha256::new();
            let mut offset = 0u64;
            let mut buf = vec![0u8; CHUNK_SIZE];

            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        tracing::info!("目录传输已取消: {}", dir_name);
                        let _ = app.emit("file-transfer-cancelled", serde_json::json!({
                            "transfer_id": transfer_id,
                        }));
                        break 'outer;
                    }
                    result = reader.read(&mut buf) => {
                        let n = match result {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(e) => {
                                tracing::warn!("文件读取中断: {}: {}", filename, e);
                                break;
                            }
                        };

                        let chunk = &buf[..n];
                        hasher.update(chunk);

                        let msg = Message::FileTransferData {
                            transfer_id: file_transfer_id,
                            offset,
                            data: chunk.to_vec(),
                        };
                        if tx_clone.send(msg).await.is_err() {
                            break 'outer;
                        }
                        offset += n as u64;

                        let _ = app.emit("file-transfer-progress", serde_json::json!({
                            "transfer_id": file_transfer_id,
                            "filename": filename,
                            "total": file_size,
                            "transferred": offset,
                        }));
                    }
                }
            }

            if !token.is_cancelled() {
                let checksum = format!("{:x}", hasher.finalize());
                let _ = tx_clone
                    .send(Message::FileTransferComplete {
                        transfer_id: file_transfer_id,
                        checksum,
                    })
                    .await;

                files_sent += 1;
                let _ = app.emit(
                    "file-transfer-progress",
                    serde_json::json!({
                        "transfer_id": transfer_id,
                        "filename": format!("{} ({}/{})", dir_name, files_sent, total_files),
                        "total": total_size,
                        "transferred": offset,
                    }),
                );
            }
        }

        if !token.is_cancelled() {
            let _ = app.emit(
                "file-transfer-complete",
                serde_json::json!({
                    "transfer_id": transfer_id,
                }),
            );
            tracing::info!(
                "目录 {} 传输完成 ({} 个文件, {} bytes)",
                dir_name,
                total_files,
                total_size
            );
        }
        conn_arc.write().await.remove_transfer(transfer_id);
    });

    Ok(transfer_id)
}

// ──────────────── 文件传输 ────────────────

/// 选择并发送文件到远程设备
#[tauri::command]
pub async fn send_file(
    app: AppHandle,
    file_path: String,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    use std::path::Path;

    let path = Path::new(&file_path);
    if !path.exists() {
        return Err(format!(
            "[ERR_FILE_NOT_FOUND] File not found: {}",
            file_path
        ));
    }

    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let metadata = tokio::fs::metadata(&file_path)
        .await
        .map_err(|e| format!("[ERR_FILE_METADATA] Failed to read file metadata: {}", e))?;
    let size = metadata.len();

    // 用原子计数器生成唯一 transfer_id，避免时间戳截断碰撞
    let transfer_id = next_transfer_id();

    // 注册取消令牌并获取 tx
    let conn_arc = state.conn.clone();
    let token = {
        let mut conn = state.conn.write().await;
        let cid = connection_id.as_deref().unwrap_or("");
        let tx = conn
            .get_input_tx(cid)
            .ok_or("[ERR_NOT_CONNECTED] Not connected")?;

        tx.send(Message::FileTransferStart {
            filename: filename.clone(),
            size,
            transfer_id,
        })
        .await
        .map_err(|_| "[ERR_SEND_FAILED] Failed to send message")?;

        conn.register_transfer(transfer_id)
    };

    let tx_clone = {
        let conn = state.conn.read().await;
        let cid = connection_id.as_deref().unwrap_or("");
        conn.get_input_tx(cid)
            .cloned()
            .ok_or("[ERR_NOT_CONNECTED] Not connected")?
    };

    tokio::spawn(async move {
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncReadExt;

        const CHUNK_SIZE: usize = 64 * 1024;

        let file = match tokio::fs::File::open(&file_path).await {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("打开文件失败: {}", e);
                conn_arc.write().await.remove_transfer(transfer_id);
                return;
            }
        };

        let mut reader = tokio::io::BufReader::new(file);
        let mut hasher = Sha256::new();
        let mut offset = 0u64;
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut cancelled = false;

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    tracing::info!("文件传输已取消: {}", filename);
                    let _ = app.emit("file-transfer-cancelled", serde_json::json!({
                        "transfer_id": transfer_id,
                    }));
                    cancelled = true;
                    break;
                }
                result = reader.read(&mut buf) => {
                    let n = match result {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) => {
                            tracing::warn!("文件读取中断: {}: {}", filename, e);
                            break;
                        }
                    };

                    let chunk = &buf[..n];
                    hasher.update(chunk);

                    let msg = Message::FileTransferData {
                        transfer_id,
                        offset,
                        data: chunk.to_vec(),
                    };
                    if tx_clone.send(msg).await.is_err() {
                        tracing::warn!("文件传输中断: {}", filename);
                        break;
                    }
                    offset += n as u64;

                    let _ = app.emit("file-transfer-progress", serde_json::json!({
                        "transfer_id": transfer_id,
                        "filename": filename,
                        "total": size,
                        "transferred": offset,
                    }));
                }
            }
        }

        if !cancelled {
            let checksum = format!("{:x}", hasher.finalize());
            let _ = tx_clone
                .send(Message::FileTransferComplete {
                    transfer_id,
                    checksum,
                })
                .await;
            let _ = app.emit(
                "file-transfer-complete",
                serde_json::json!({
                    "transfer_id": transfer_id,
                    "filename": filename,
                }),
            );
            tracing::info!("文件 {} 传输完成 ({} bytes)", filename, size);
        }
        conn_arc.write().await.remove_transfer(transfer_id);
    });

    Ok(transfer_id)
}
