#[cfg(feature = "desktop")]
use std::sync::Arc;
#[cfg(feature = "desktop")]
use std::time::Duration;

#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter, Listener, State};
#[cfg(feature = "desktop")]
use tokio::sync::mpsc;

#[cfg(feature = "desktop")]
use lan_desk_server::AuthRequest;

#[cfg(feature = "desktop")]
use crate::state::AppState;

#[cfg(feature = "desktop")]
use super::AuthRequestEvent;

// ──────────────── 设备 ID ────────────────

/// 生成 9 位数字设备 ID（基于跨平台机器标识的 SHA-256 哈希）
#[cfg(feature = "desktop")]
#[tauri::command]
pub fn get_device_id() -> String {
    use sha2::{Digest, Sha256};

    let raw_id = lan_desk_server::tls::get_machine_id();
    let hash = Sha256::digest(raw_id.as_bytes());
    let n = u64::from_be_bytes(hash[0..8].try_into().unwrap_or([0; 8]));
    let id = n % 1_000_000_000;
    format!("{:09}", id)
}

/// 移动端设备 ID（基于持久化 UUID 的 SHA-256 哈希，避免 hostname 重复问题）
#[cfg(not(feature = "desktop"))]
#[tauri::command]
pub fn get_device_id() -> String {
    use sha2::{Digest, Sha256};

    // 优先从本地文件读取已生成的 UUID，否则生成并持久化
    let uuid_path = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("lan-desk")
        .join("device_uuid");
    let raw_id = std::fs::read_to_string(&uuid_path)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            // 生成新 UUID 并持久化
            use rand::Rng;
            let mut bytes = [0u8; 16];
            rand::rngs::OsRng.fill(&mut bytes);
            let uuid = bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            if let Some(parent) = uuid_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&uuid_path, &uuid);
            uuid
        });
    let hash = Sha256::digest(raw_id.trim().as_bytes());
    let n = u64::from_be_bytes(hash[0..8].try_into().unwrap_or([0; 8]));
    let id = n % 1_000_000_000;
    format!("{:09}", id)
}

// ──────────────── 被控端服务器 ────────────────

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn start_server(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // 读取服务器状态并标记为运行中，同时创建停止信号
    let (stop_rx, port) = {
        let mut server = state.server.write().await;
        if server.running {
            // 幂等：服务器已在运行时静默返回成功，不报错
            return Ok(());
        }
        server.running = true;
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        server.stop_tx = Some(stop_tx);
        (stop_rx, server.port)
    };

    // 读取认证配置（PIN 使用 Zeroizing 保护，drop 时自动清零）
    let (control_pin, view_pin, auto_accept) = {
        let auth = state.auth.read().await;
        (
            auth.control_pin.to_zeroizing(),
            auth.view_pin.to_zeroizing(),
            auth.auto_accept,
        )
    };

    let shell_enabled_flag = {
        let server = state.server.read().await;
        server.shell_enabled.clone()
    };

    let server_handle = state.server.clone();

    // 创建授权回调：通过 Tauri 事件通知前端，等待用户点击
    let app_handle = app.clone();
    let auth_callback: lan_desk_server::AuthCallback = Arc::new(move |request: AuthRequest| {
        let app = app_handle.clone();
        Box::pin(async move {
            // 发送授权请求事件到前端
            let event = AuthRequestEvent {
                hostname: request.hostname.clone(),
                addr: request.addr.clone(),
                granted_role: request.granted_role.clone(),
            };
            if let Err(e) = app.emit("auth-request", event) {
                tracing::warn!("发送授权请求事件失败: {}", e);
            }

            // 注册一次性事件监听等待用户响应
            let (result_tx, mut result_rx) = mpsc::channel::<bool>(1);
            let result_tx_clone = result_tx.clone();

            // 监听 auth-response 事件
            let listener = app.listen_any("auth-response", move |event: tauri::Event| {
                let approved = event
                    .payload()
                    .trim_matches('"')
                    .parse::<bool>()
                    .unwrap_or(false);
                let _ = result_tx_clone.try_send(approved);
            });

            // 等待响应（30 秒超时）
            let approved = tokio::time::timeout(Duration::from_secs(30), result_rx.recv())
                .await
                .unwrap_or(None)
                .unwrap_or(false);

            // 清理监听器
            app.unlisten(listener);

            approved
        })
    });

    // 获取应用数据目录用于 TLS 证书持久化
    let data_dir = Some(crate::portable::data_dir());

    // 从前端 localStorage 读取空闲超时和锁屏设置（通过 app_handle invoke 无法直接读，
    // 故通过 ServerState 扩展字段传入，或者用固定默认值）
    // 这里从 ServerState 中读取扩展字段
    let (idle_timeout_secs, lock_on_disconnect) = {
        let server = state.server.read().await;
        (server.idle_timeout_secs, server.lock_on_disconnect)
    };

    let app_for_chat = app.clone();
    tokio::spawn(async move {
        let shell_enabled = shell_enabled_flag.load(std::sync::atomic::Ordering::Relaxed);
        match lan_desk_server::Server::bind(
            port,
            control_pin,
            view_pin,
            auto_accept,
            data_dir.as_deref(),
            shell_enabled,
            idle_timeout_secs,
            lock_on_disconnect,
        )
        .await
        {
            Ok(mut server) => {
                server.set_auth_callback(auth_callback);
                // 共享 shell_enabled 原子标志：前端 set_shell_enabled 修改后，Server 每次 accept 时读取最新值
                server.shell_enabled_flag = Some(shell_enabled_flag);

                // 存储被控端聊天广播通道（供 send_chat_message 使用）
                {
                    let mut srv = server_handle.write().await;
                    srv.host_chat_tx = Some(server.host_chat_tx());
                }

                // 聊天消息转发：服务器 → Tauri 前端事件
                let (chat_tx, mut chat_rx) =
                    tokio::sync::mpsc::channel::<(String, String, u64)>(32);
                server.chat_tx = Some(chat_tx);
                let chat_app = app_for_chat.clone();
                tokio::spawn(async move {
                    while let Some((text, sender, timestamp_ms)) = chat_rx.recv().await {
                        let _ = chat_app.emit(
                            "chat-message",
                            serde_json::json!({
                                "text": text,
                                "sender": sender,
                                "timestamp_ms": timestamp_ms,
                            }),
                        );
                    }
                });

                // 会话事件转发：服务器 → Tauri 前端事件（连接/断开通知）
                let (session_event_tx, mut session_event_rx) =
                    tokio::sync::mpsc::channel::<(String, String, String, String)>(16);
                server.session_event_tx = Some(session_event_tx);
                let session_app = app_for_chat.clone();
                tokio::spawn(async move {
                    while let Some((event_type, hostname, addr, role)) =
                        session_event_rx.recv().await
                    {
                        let event_name = if event_type == "connected" {
                            "session-connected"
                        } else {
                            "session-disconnected"
                        };
                        let _ = session_app.emit(
                            event_name,
                            serde_json::json!({
                                "hostname": hostname,
                                "addr": addr,
                                "role": role,
                            }),
                        );
                    }
                });

                tokio::select! {
                    result = server.run() => {
                        if let Err(e) = result {
                            tracing::error!("服务器运行异常: {}", e);
                        }
                    }
                    _ = stop_rx => {
                        tracing::info!("服务器收到停止信号");
                    }
                }
            }
            Err(e) => {
                tracing::error!("服务器启动失败: {}", e);
            }
        }
        server_handle.write().await.running = false;
    });

    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn stop_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut server = state.server.write().await;
    if let Some(tx) = server.stop_tx.take() {
        let _ = tx.send(());
        server.running = false;
        Ok(())
    } else {
        Err("[ERR_SERVER_NOT_RUNNING] Server is not running".to_string())
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn set_shell_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let server = state.server.read().await;
    server
        .shell_enabled
        .store(enabled, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("远程终端已{}", if enabled { "启用" } else { "禁用" });
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn set_idle_timeout(state: State<'_, AppState>, minutes: u64) -> Result<(), String> {
    let mut server = state.server.write().await;
    server.idle_timeout_secs = minutes * 60;
    tracing::info!("空闲超时设为 {} 分钟", minutes);
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn set_lock_on_disconnect(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let mut server = state.server.write().await;
    server.lock_on_disconnect = enabled;
    tracing::info!("断开时自动锁屏已{}", if enabled { "启用" } else { "禁用" });
    Ok(())
}
