use futures::SinkExt;
use tracing::{debug, info, warn};

use lan_desk_protocol::message::{FileEntry, Message, SessionRole};

use super::Session;

const MAX_CONCURRENT_TRANSFERS: usize = 5;

/// 路径安全校验：只允许访问用户目录下的文件
fn is_safe_path(path: &std::path::Path) -> bool {
    // 必须是绝对路径
    if !path.is_absolute() {
        return false;
    }
    // 不能包含 .. 组件
    for comp in path.components() {
        if matches!(comp, std::path::Component::ParentDir) {
            return false;
        }
    }
    // 解析符号链接后再验证，防止符号链接穿越到受限目录外
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    // 必须在用户目录下
    if let Some(home) = dirs_next::home_dir() {
        resolved.starts_with(&home)
    } else if let Some(dl) = dirs_next::download_dir() {
        resolved.starts_with(&dl)
    } else {
        false
    }
}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static> Session<S> {
    /// 处理文件传输相关消息，返回 true 表示消息已被处理
    pub(super) async fn handle_file_message(&mut self, msg: &Message) -> anyhow::Result<bool> {
        match msg {
            Message::FileTransferStart {
                filename,
                size,
                transfer_id,
            } if self.granted_role == SessionRole::Controller => {
                // 文件大小限制：最大 2GB
                const MAX_FILE_SIZE: u64 = 2_u64 * 1024 * 1024 * 1024;
                if *size > MAX_FILE_SIZE {
                    warn!(
                        target: "audit",
                        op = "file_transfer_rejected",
                        addr = %self.addr,
                        role = ?self.granted_role,
                        filename = %filename,
                        size = size,
                        transfer_id = transfer_id,
                        reason = "size_limit_exceeded",
                        "文件传输拒绝: 超过 2GB 限制"
                    );
                    // Notify sender of rejection
                    let _ = self
                        .framed
                        .send(Message::FileTransferComplete {
                            transfer_id: *transfer_id,
                            checksum: String::new(),
                        })
                        .await;
                    return Ok(true);
                } else if self.file_transfers.len() >= MAX_CONCURRENT_TRANSFERS {
                    warn!("并发传输数量已达上限: {}", MAX_CONCURRENT_TRANSFERS);
                    return Ok(true);
                } else {
                    info!(
                        target: "audit",
                        op = "file_transfer_start",
                        addr = %self.addr,
                        role = ?self.granted_role,
                        filename = %filename,
                        size = size,
                        transfer_id = transfer_id,
                        "文件传输开始"
                    );
                    let downloads =
                        dirs_next::download_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                    // 安全处理：只取文件名部分，防止路径遍历攻击
                    let safe_name = sanitize_filename(filename, *transfer_id);
                    let save_path = downloads.join(&safe_name);
                    if let Err(e) = tokio::fs::write(&save_path, b"").await {
                        warn!("创建文件失败: {:?}: {}", save_path, e);
                    }
                    self.file_transfers.insert(*transfer_id, (save_path, *size));
                }
                Ok(true)
            }
            // Viewer 的文件传输请求统一拦截
            Message::FileTransferData { .. } | Message::FileTransferComplete { .. }
                if self.granted_role == SessionRole::Viewer =>
            {
                warn!(
                    target: "audit",
                    op = "file_transfer_rejected",
                    addr = %self.addr,
                    role = ?self.granted_role,
                    reason = "viewer_role",
                    "文件传输拒绝: Viewer 角色无文件传输权限"
                );
                Ok(true)
            }
            Message::FileTransferData {
                transfer_id,
                offset,
                data,
            } => {
                debug!(
                    target: "audit",
                    op = "file_transfer_data",
                    addr = %self.addr,
                    transfer_id = transfer_id,
                    offset = offset,
                    data_len = data.len(),
                    "文件传输数据接收"
                );
                if let Some((path, declared_size)) = self.file_transfers.get(transfer_id) {
                    if *offset + data.len() as u64 > *declared_size {
                        warn!(
                            "文件传输数据超出声明大小: offset={} + data_len={} > declared_size={}",
                            offset,
                            data.len(),
                            declared_size
                        );
                        return Ok(true);
                    }
                    use tokio::io::{AsyncSeekExt, AsyncWriteExt};
                    if let Ok(mut f) = tokio::fs::OpenOptions::new().write(true).open(path).await {
                        if f.seek(std::io::SeekFrom::Start(*offset)).await.is_ok() {
                            if let Err(e) = f.write_all(data).await {
                                warn!("文件写入失败 (transfer_id={}): {}", transfer_id, e);
                            }
                        }
                    }
                }
                Ok(true)
            }
            Message::FileTransferComplete {
                transfer_id,
                checksum,
            } => {
                if let Some((path, _declared_size)) = self.file_transfers.remove(transfer_id) {
                    // 流式 SHA-256 校验
                    match tokio::fs::File::open(&path).await {
                        Ok(mut file) => {
                            use sha2::{Digest, Sha256};
                            use tokio::io::AsyncReadExt;
                            let mut hasher = Sha256::new();
                            let mut buf = vec![0u8; 64 * 1024];
                            loop {
                                match file.read(&mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => hasher.update(&buf[..n]),
                                    Err(e) => {
                                        warn!("读取文件校验失败: {:?}: {}", path, e);
                                        break;
                                    }
                                }
                            }
                            let actual = format!("{:x}", hasher.finalize());
                            if actual == *checksum {
                                info!(
                                    target: "audit",
                                    op = "file_transfer_complete",
                                    addr = %self.addr,
                                    transfer_id = transfer_id,
                                    filename = ?path,
                                    checksum = %actual,
                                    "文件传输完成并校验通过"
                                );
                            } else {
                                warn!(
                                    target: "audit",
                                    op = "file_transfer_checksum_failed",
                                    addr = %self.addr,
                                    transfer_id = transfer_id,
                                    filename = ?path,
                                    expected = %&checksum[..checksum.len().min(8)],
                                    actual = %&actual[..actual.len().min(8)],
                                    "文件校验失败"
                                );
                                if let Err(e2) = tokio::fs::remove_file(&path).await {
                                    warn!("删除校验失败的文件失败: {:?}: {}", path, e2);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("打开文件校验失败: {:?}: {}", path, e);
                        }
                    }
                }
                Ok(true)
            }
            // === 远程文件浏览 ===
            Message::FileListRequest { path, request_id }
                if self.granted_role == SessionRole::Controller =>
            {
                info!(
                    target: "audit",
                    op = "file_list_request",
                    addr = %self.addr,
                    path = %path,
                    request_id = request_id,
                    "文件列表请求"
                );
                let target_path = if path.is_empty() {
                    dirs_next::download_dir().unwrap_or_else(|| std::path::PathBuf::from("."))
                } else {
                    std::path::PathBuf::from(path)
                };

                if !is_safe_path(&target_path) {
                    let _ = self
                        .framed
                        .send(Message::FileListResponse {
                            request_id: *request_id,
                            path: target_path.to_string_lossy().to_string(),
                            entries: vec![],
                            error: "Access denied: path is outside user directory".to_string(),
                        })
                        .await;
                    return Ok(true);
                }

                let (entries, error) = match tokio::fs::read_dir(&target_path).await {
                    Ok(mut dir) => {
                        let mut entries = Vec::new();
                        while let Ok(Some(entry)) = dir.next_entry().await {
                            let meta = entry.metadata().await.ok();
                            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                            let modified_ms = meta
                                .as_ref()
                                .and_then(|m| m.modified().ok())
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            entries.push(FileEntry {
                                name: entry.file_name().to_string_lossy().to_string(),
                                is_dir,
                                size,
                                modified_ms,
                            });
                        }
                        // 目录优先，然后按名称排序
                        entries.sort_by(|a, b| {
                            b.is_dir
                                .cmp(&a.is_dir)
                                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                        });
                        (entries, String::new())
                    }
                    Err(e) => {
                        warn!("读取目录失败: {:?}: {}", target_path, e);
                        (vec![], format!("Failed to read directory: {}", e))
                    }
                };

                let _ = self
                    .framed
                    .send(Message::FileListResponse {
                        request_id: *request_id,
                        path: target_path.to_string_lossy().to_string(),
                        entries,
                        error,
                    })
                    .await;
                Ok(true)
            }

            // === 文件下载请求（反向传输：被控端 -> 控制端）===
            Message::FileDownloadRequest { path, transfer_id }
                if self.granted_role == SessionRole::Controller =>
            {
                info!(
                    target: "audit",
                    op = "file_download_request",
                    addr = %self.addr,
                    path = %path,
                    transfer_id = transfer_id,
                    "文件下载请求"
                );
                let file_path = std::path::PathBuf::from(path);

                if !is_safe_path(&file_path) {
                    warn!("文件下载拒绝: 路径不安全 {:?}", file_path);
                    let _ = self
                        .framed
                        .send(Message::FileTransferComplete {
                            transfer_id: *transfer_id,
                            checksum: String::new(),
                        })
                        .await;
                    return Ok(true);
                }

                let meta = match tokio::fs::metadata(&file_path).await {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("文件不存在或无法访问: {:?}: {}", file_path, e);
                        let _ = self
                            .framed
                            .send(Message::FileTransferComplete {
                                transfer_id: *transfer_id,
                                checksum: String::new(),
                            })
                            .await;
                        return Ok(true);
                    }
                };

                let filename = file_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let size = meta.len();
                let tid = *transfer_id;

                // 发送 FileTransferStart（反向：从被控端发给控制端）
                let _ = self
                    .framed
                    .send(Message::FileTransferStart {
                        filename: filename.clone(),
                        size,
                        transfer_id: tid,
                    })
                    .await;

                // 读取文件并发送数据块
                match tokio::fs::File::open(&file_path).await {
                    Ok(mut file) => {
                        use sha2::{Digest, Sha256};
                        use tokio::io::AsyncReadExt;
                        const CHUNK_SIZE: usize = 64 * 1024;
                        let mut hasher = Sha256::new();
                        let mut offset = 0u64;
                        let mut buf = vec![0u8; CHUNK_SIZE];

                        loop {
                            let n = match file.read(&mut buf).await {
                                Ok(0) => break,
                                Ok(n) => n,
                                Err(e) => {
                                    warn!("读取文件失败: {:?}: {}", file_path, e);
                                    break;
                                }
                            };
                            let chunk = &buf[..n];
                            hasher.update(chunk);

                            let _ = self
                                .framed
                                .send(Message::FileTransferData {
                                    transfer_id: tid,
                                    offset,
                                    data: chunk.to_vec(),
                                })
                                .await;
                            offset += n as u64;
                        }

                        let checksum = format!("{:x}", hasher.finalize());
                        let _ = self
                            .framed
                            .send(Message::FileTransferComplete {
                                transfer_id: tid,
                                checksum,
                            })
                            .await;
                        info!("文件下载完成: {} ({} bytes)", filename, size);
                    }
                    Err(e) => {
                        warn!("打开文件失败: {:?}: {}", file_path, e);
                        let _ = self
                            .framed
                            .send(Message::FileTransferComplete {
                                transfer_id: tid,
                                checksum: String::new(),
                            })
                            .await;
                    }
                }
                Ok(true)
            }

            // === 目录下载（被控端 -> 控制端）===
            Message::DirectoryDownloadRequest { path, transfer_id }
                if self.granted_role == SessionRole::Controller =>
            {
                info!(
                    target: "audit",
                    op = "directory_download_request",
                    addr = %self.addr,
                    path = %path,
                    transfer_id = transfer_id,
                    "目录下载请求"
                );
                let dir_path = std::path::PathBuf::from(path);

                if !is_safe_path(&dir_path) || !dir_path.is_dir() {
                    warn!("目录下载拒绝: 路径不安全或不是目录 {:?}", dir_path);
                    return Ok(true);
                }

                // 递归收集目录条目
                fn collect_entries_sync(
                    base: &std::path::Path,
                    current: &std::path::Path,
                ) -> std::io::Result<Vec<(String, bool, u64)>> {
                    let mut entries = Vec::new();
                    for entry in std::fs::read_dir(current)? {
                        let entry = entry?;
                        // 统一使用 '/' 作为路径分隔符，确保跨平台兼容
                        let relative = entry
                            .path()
                            .strip_prefix(base)
                            .unwrap_or(entry.path().as_path())
                            .to_string_lossy()
                            .replace('\\', "/");
                        let meta = entry.metadata()?;
                        if meta.is_dir() {
                            entries.push((relative.clone(), true, 0));
                            entries.extend(collect_entries_sync(base, &entry.path())?);
                        } else {
                            entries.push((relative, false, meta.len()));
                        }
                    }
                    Ok(entries)
                }

                let dir_entries = match collect_entries_sync(&dir_path, &dir_path) {
                    Ok(e) => e,
                    Err(e) => {
                        warn!("目录扫描失败: {:?}: {}", dir_path, e);
                        return Ok(true);
                    }
                };

                let dir_name = dir_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown_dir".to_string());
                let total_files =
                    dir_entries.iter().filter(|(_, is_dir, _)| !*is_dir).count() as u32;
                let total_size: u64 = dir_entries.iter().map(|(_, _, size)| size).sum();
                let tid = *transfer_id;

                // 发送 DirectoryTransferStart
                let _ = self
                    .framed
                    .send(Message::DirectoryTransferStart {
                        transfer_id: tid,
                        base_path: dir_name.clone(),
                        total_files,
                        total_size,
                    })
                    .await;

                // 发送所有 DirectoryEntry
                for (relative, is_dir_entry, size) in &dir_entries {
                    let _ = self
                        .framed
                        .send(Message::DirectoryEntry {
                            transfer_id: tid,
                            relative_path: relative.clone(),
                            is_dir: *is_dir_entry,
                            size: *size,
                        })
                        .await;
                }

                // 逐文件发送数据
                use sha2::{Digest, Sha256};
                use tokio::io::AsyncReadExt;
                const CHUNK_SIZE: usize = 64 * 1024;

                static DIR_DL_FILE_COUNTER: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(1);

                for (relative, is_dir_entry, _) in &dir_entries {
                    if *is_dir_entry {
                        continue;
                    }
                    let file_path = dir_path.join(relative);
                    let file_tid =
                        DIR_DL_FILE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                    let meta = match tokio::fs::metadata(&file_path).await {
                        Ok(m) => m,
                        Err(_) => continue,
                    };
                    let file_size = meta.len();
                    let filename = file_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let _ = self
                        .framed
                        .send(Message::FileTransferStart {
                            filename,
                            size: file_size,
                            transfer_id: file_tid,
                        })
                        .await;

                    match tokio::fs::File::open(&file_path).await {
                        Ok(mut file) => {
                            let mut hasher = Sha256::new();
                            let mut offset = 0u64;
                            let mut buf = vec![0u8; CHUNK_SIZE];
                            loop {
                                let n = match file.read(&mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => n,
                                    Err(_) => break,
                                };
                                hasher.update(&buf[..n]);
                                let _ = self
                                    .framed
                                    .send(Message::FileTransferData {
                                        transfer_id: file_tid,
                                        offset,
                                        data: buf[..n].to_vec(),
                                    })
                                    .await;
                                offset += n as u64;
                            }
                            let checksum = format!("{:x}", hasher.finalize());
                            let _ = self
                                .framed
                                .send(Message::FileTransferComplete {
                                    transfer_id: file_tid,
                                    checksum,
                                })
                                .await;
                        }
                        Err(e) => {
                            warn!("打开文件失败: {:?}: {}", file_path, e);
                            let _ = self
                                .framed
                                .send(Message::FileTransferComplete {
                                    transfer_id: file_tid,
                                    checksum: String::new(),
                                })
                                .await;
                        }
                    }
                }
                info!("目录下载完成: {} ({} 个文件)", dir_name, total_files);
                Ok(true)
            }

            // === 目录传输 ===
            Message::DirectoryTransferStart {
                transfer_id,
                base_path,
                total_files,
                total_size,
            } if self.granted_role == SessionRole::Controller => {
                info!(
                    target: "audit",
                    op = "directory_transfer_start",
                    addr = %self.addr,
                    transfer_id = transfer_id,
                    base_path = %base_path,
                    total_files = total_files,
                    total_size = total_size,
                    "目录传输开始"
                );
                let downloads =
                    dirs_next::download_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                let safe_name = sanitize_filename(base_path, *transfer_id);
                let dir_path = downloads.join(&safe_name);
                let _ = tokio::fs::create_dir_all(&dir_path).await;
                // 记录传输中的目录基路径
                self.file_transfers
                    .insert(*transfer_id, (dir_path, *total_size));
                Ok(true)
            }

            Message::DirectoryEntry {
                transfer_id,
                relative_path,
                is_dir,
                size: _,
            } => {
                if let Some((base_path, _)) = self.file_transfers.get(transfer_id) {
                    // 按路径组件逐一过滤，拒绝 ".." 和绝对路径前缀，防止路径遍历攻击
                    let safe_relative: std::path::PathBuf = std::path::Path::new(relative_path)
                        .components()
                        .filter(|c| matches!(c, std::path::Component::Normal(_)))
                        .collect();
                    let target = base_path.join(&safe_relative);
                    if *is_dir {
                        let _ = tokio::fs::create_dir_all(&target).await;
                        debug!("创建目录: {:?}", target);
                    }
                    // 文件会通过后续的 FileTransferStart/Data/Complete 流程传输
                }
                Ok(true)
            }

            // === 断点续传 ===
            Message::FileTransferResume {
                transfer_id,
                offset,
            } => {
                info!(
                    target: "audit",
                    op = "file_transfer_resume",
                    addr = %self.addr,
                    transfer_id = transfer_id,
                    offset = offset,
                    "断点续传请求"
                );
                // 此消息由接收端发送，通知发送端从 offset 继续
                // 在被控端作为服务器时，此消息转发给控制端处理
                // （实际续传逻辑在发送端的 Tauri 命令中实现）
                Ok(true)
            }

            _ => Ok(false),
        }
    }
}

/// 安全处理文件名（委托给 protocol crate 共享实现）
fn sanitize_filename(filename: &str, transfer_id: u32) -> String {
    lan_desk_protocol::sanitize_filename(filename, &format!("file_{}", transfer_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_normal() {
        // 正常文件名应原样返回
        assert_eq!(sanitize_filename("test.txt", 1), "test.txt".to_string());
        assert_eq!(
            sanitize_filename("my file.pdf", 2),
            "my file.pdf".to_string()
        );
    }

    #[test]
    fn test_sanitize_filename_strips_path_traversal() {
        // 路径遍历应被剥离，只保留最终文件名部分
        assert_eq!(sanitize_filename("../etc/passwd", 1), "passwd".to_string());
        assert_eq!(sanitize_filename("foo/bar.txt", 2), "bar.txt".to_string());
        assert_eq!(sanitize_filename("foo\\bar.txt", 3), "bar.txt".to_string());
    }

    #[test]
    fn test_sanitize_filename_fallback_on_empty() {
        // 空字符串无法提取文件名，应回退到 file_{transfer_id}
        assert_eq!(sanitize_filename("", 42), "file_42".to_string());
    }

    #[test]
    fn test_sanitize_filename_fallback_on_dots_only() {
        // ".." 没有有效文件名部分，应回退
        assert_eq!(sanitize_filename("..", 99), "file_99".to_string());
    }

    #[test]
    fn test_sanitize_filename_blocks_windows_reserved_names() {
        // Windows 保留设备名应被替换为安全的 file_{id}
        assert_eq!(sanitize_filename("CON", 1), "file_1");
        assert_eq!(sanitize_filename("NUL", 2), "file_2");
        assert_eq!(sanitize_filename("AUX", 3), "file_3");
        assert_eq!(sanitize_filename("PRN", 4), "file_4");
        assert_eq!(sanitize_filename("COM1", 5), "file_5");
        assert_eq!(sanitize_filename("LPT1", 6), "file_6");
        // 大小写不敏感
        assert_eq!(sanitize_filename("con", 7), "file_7");
        assert_eq!(sanitize_filename("Nul.txt", 8), "file_8");
        assert_eq!(sanitize_filename("com3.log", 9), "file_9");
    }

    #[test]
    fn test_sanitize_filename_allows_similar_but_safe_names() {
        // 类似但不是保留名的文件名应被允许
        assert_eq!(sanitize_filename("CONSOLE.txt", 1), "CONSOLE.txt");
        assert_eq!(sanitize_filename("COM10.dat", 2), "COM10.dat");
        assert_eq!(sanitize_filename("auxiliary.rs", 3), "auxiliary.rs");
        assert_eq!(sanitize_filename("null_value.json", 4), "null_value.json");
    }
}
