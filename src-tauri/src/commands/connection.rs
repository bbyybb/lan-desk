use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;
use tokio_util::codec::Framed;

#[cfg(feature = "desktop")]
use lan_desk_clipboard::{ClipboardChange, ClipboardManager};
use lan_desk_protocol::codec::LanDeskCodec;
use lan_desk_protocol::message::{Message, SessionRole};
use lan_desk_protocol::PROTOCOL_VERSION;

/// 移动端剪贴板占位类型（桌面端使用 lan_desk_clipboard::ClipboardChange）
#[cfg(not(feature = "desktop"))]
struct ClipboardChange {
    pub content_type: lan_desk_protocol::message::ClipboardContentType,
    pub data: Vec<u8>,
}

use crate::state::{AppState, ConnectionInfo, SecurePin};

use super::FrameEvent;
use super::FrameRegion;

// ──────────────── TLS TOFU 证书验证器（局域网自签名）────────────────

/// 应用数据目录路径（在 Tauri setup 时初始化）
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 已信任的证书指纹缓存 (addr -> SHA-256 hex fingerprint)
static TRUSTED_FINGERPRINTS: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

/// 初始化 TOFU 数据目录（应在 Tauri setup 中调用）
pub fn init_tofu_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
    // 加载已有的受信任指纹
    if let Some(path) = DATA_DIR.get() {
        let fp_path = path.join("trusted_fingerprints.json");
        let loaded = load_trusted_fingerprints(&fp_path);
        let mut guard = TRUSTED_FINGERPRINTS
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *guard = Some(loaded);
    }
}

/// 从磁盘加载受信任的指纹
fn load_trusted_fingerprints(path: &Path) -> HashMap<String, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// 将受信任的指纹保存到磁盘
fn save_trusted_fingerprints(path: &Path, map: &HashMap<String, String>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(map) {
        if let Err(e) = std::fs::write(path, json) {
            tracing::warn!("保存受信任指纹失败: {}", e);
        }
    }
}

/// TOFU 证书验证器：首次连接信任并记录指纹，后续连接校验一致性
#[derive(Debug)]
pub(crate) struct TofuCertVerifier {
    /// 连接的实际地址（IP:port），用作指纹存储的键
    addr: String,
}

impl TofuCertVerifier {
    fn cert_fingerprint(cert_der: &[u8]) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        Sha256::digest(cert_der).to_vec()
    }
}

impl rustls::client::danger::ServerCertVerifier for TofuCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls_pki_types::CertificateDer<'_>,
        _intermediates: &[rustls_pki_types::CertificateDer<'_>],
        _server_name: &rustls_pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls_pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let fingerprint = Self::cert_fingerprint(end_entity.as_ref());
        let fp_hex = hex::encode(&fingerprint);
        let name = self.addr.clone();

        // 使用写锁原子检查+插入，避免 TOCTOU 竞态
        let mut guard = TRUSTED_FINGERPRINTS
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let trusted = guard.get_or_insert_with(HashMap::new);

        if let Some(known_hex) = trusted.get(&name) {
            if *known_hex == fp_hex {
                return Ok(rustls::client::danger::ServerCertVerified::assertion());
            }
            // 指纹变更：拒绝连接，防止中间人攻击
            // 前端可识别 [TOFU_MISMATCH] 前缀并提示用户确认
            tracing::warn!(
                "TLS 证书指纹变更，拒绝连接! 目标: {}, 已知: {}..., 当前: {}...",
                name,
                &known_hex[..16.min(known_hex.len())],
                &fp_hex[..16.min(fp_hex.len())]
            );
            return Err(rustls::Error::General(format!(
                "[TOFU_MISMATCH] host={} old={} new={}",
                name,
                &known_hex[..16.min(known_hex.len())],
                &fp_hex[..16.min(fp_hex.len())]
            )));
        }

        // 首次连接：记录指纹 (TOFU)
        trusted.insert(name.clone(), fp_hex);
        tracing::info!("TOFU: 首次信任证书指纹 ({})", name);

        // 持久化到磁盘
        if let Some(dir) = DATA_DIR.get() {
            let fp_path = dir.join("trusted_fingerprints.json");
            save_trusted_fingerprints(&fp_path, trusted);
        }

        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls_pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls_pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

// ──────────────── 连接（控制端）────────────────

/// 安全处理文件名（委托给 protocol crate 共享实现）
fn sanitize_filename(name: &str) -> String {
    lan_desk_protocol::sanitize_filename(name, "unnamed_file")
}

#[tauri::command]
pub async fn connect_to_peer(
    app: AppHandle,
    addr: String,
    pin: String,
    role: Option<String>,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // 生成或使用给定的 connection_id
    let conn_id = {
        let mut conn = state.conn.write().await;
        if let Some(id) = connection_id {
            if !id.is_empty() {
                id
            } else {
                // 兼容旧代码：无 connection_id 时检查默认连接
                if conn.info.is_some() {
                    return Err(
                        "[ERR_ALREADY_CONNECTED] Active connection exists, please disconnect first"
                            .to_string(),
                    );
                }
                String::new()
            }
        } else {
            // 无 connection_id 时检查默认连接
            if conn.info.is_some() {
                return Err(
                    "[ERR_ALREADY_CONNECTED] Active connection exists, please disconnect first"
                        .to_string(),
                );
            }
            conn.create_connection()
        }
    };

    // 保存 PIN 用于重连
    state.auth.write().await.last_pin = SecurePin::new(pin.clone());

    let stream = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("[ERR_CONNECT_FAILED] Connection to {} failed: {}", addr, e))?;
    stream.set_nodelay(true).ok();

    // TLS 连接（信任所有证书，局域网自签名模型）
    let tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(TofuCertVerifier { addr: addr.clone() }))
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(tls_config));
    let domain = rustls_pki_types::ServerName::try_from("lan-desk.local")
        .map_err(|e| format!("[ERR_TLS_DOMAIN] TLS domain error: {}", e))?;
    let tls_stream = connector
        .connect(domain, stream)
        .await
        .map_err(|e| format!("[ERR_TLS_HANDSHAKE] TLS handshake failed: {}", e))?;

    let mut framed = Framed::new(tls_stream, LanDeskCodec::new());

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // Hello 握手（带 PIN + 随机 salt + 角色）
    let requested_role = match role.as_deref() {
        Some("viewer") => SessionRole::Viewer,
        _ => SessionRole::Controller,
    };
    let pin_salt = lan_desk_protocol::generate_salt();

    #[cfg(feature = "desktop")]
    let dpi_scale = lan_desk_capture::get_dpi_scale();
    #[cfg(not(feature = "desktop"))]
    let dpi_scale = 100u32;

    framed
        .send(Message::Hello {
            version: PROTOCOL_VERSION,
            hostname,
            screen_width: 0,
            screen_height: 0,
            pin: lan_desk_protocol::hash_pin(&pin, &pin_salt),
            pin_salt,
            dpi_scale,
            requested_role,
        })
        .await
        .map_err(|e| format!("[ERR_SEND_HELLO] Failed to send Hello: {}", e))?;

    let msg = framed
        .next()
        .await
        .ok_or("[ERR_HANDSHAKE_CLOSED] Connection closed during handshake")?
        .map_err(|e| format!("[ERR_RECV_ACK] Failed to receive HelloAck: {}", e))?;

    match msg {
        Message::HelloAck {
            accepted: true,
            granted_role,
            ..
        } => {
            tracing::info!("连接到 {} 成功，角色: {:?}", addr, granted_role);
            if let Err(e) = app.emit("role-granted", format!("{:?}", granted_role)) {
                tracing::debug!("发送角色授予事件失败: {}", e);
            }
        }
        Message::HelloAck {
            accepted: false,
            reject_reason,
            ..
        } => {
            return Err(format!(
                "[ERR_REJECTED] Connection rejected: {}",
                reject_reason
            ));
        }
        _ => {
            return Err("[ERR_UNEXPECTED_RESPONSE] Unexpected handshake response".to_string());
        }
    }

    // 创建输入 channel
    let (input_tx, mut input_rx) = mpsc::channel::<Message>(256);

    {
        let mut conn = state.conn.write().await;
        // 存储到多连接映射和默认字段（向后兼容）
        if !conn_id.is_empty() {
            if let Some(sc) = conn.get_connection_mut(&conn_id) {
                sc.input_tx = Some(input_tx.clone());
                sc.info = Some(ConnectionInfo {
                    remote_addr: addr.clone(),
                });
            }
        }
        conn.input_tx = Some(input_tx);
        conn.info = Some(ConnectionInfo {
            remote_addr: addr.clone(),
        });
    }

    let conn_handle = state.conn.clone();
    let conn_id_clone = conn_id.clone();

    // 启动 WebSocket 二进制桥接
    let ws_bridge = match crate::ws_bridge::WsBridge::start().await {
        Ok(bridge) => {
            let mut conn = state.conn.write().await;
            conn.ws_bridge = Some(bridge.clone());
            if !conn_id.is_empty() {
                if let Some(sc) = conn.get_connection_mut(&conn_id) {
                    sc.ws_bridge = Some(bridge.clone());
                }
            }
            Some(bridge)
        }
        Err(e) => {
            tracing::warn!("WebSocket 桥接启动失败，回退 Tauri emit: {}", e);
            None
        }
    };

    // 剪贴板
    #[cfg(feature = "desktop")]
    let (mut clipboard_local_rx, clipboard_remote_tx) = {
        let (mut clipboard_mgr, local_rx, remote_tx) = ClipboardManager::new();
        tokio::spawn(async move {
            clipboard_mgr.run().await;
        });
        (local_rx, remote_tx)
    };
    #[cfg(not(feature = "desktop"))]
    let (mut clipboard_local_rx, clipboard_remote_tx) = {
        // 移动端不支持剪贴板同步，创建不会产生数据的占位 channel
        let (_tx, rx) = tokio::sync::mpsc::channel::<ClipboardChange>(1);
        let (remote_tx, _remote_rx) = tokio::sync::mpsc::channel::<ClipboardChange>(1);
        (rx, remote_tx)
    };

    // 帧接收 + 输入发送循环
    tokio::spawn(async move {
        let base64_engine = base64::engine::general_purpose::STANDARD;
        let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
        #[cfg(feature = "desktop")]
        let mut opus_decoder: Option<lan_desk_audio::opus_codec::OpusDecoder> = None;
        #[cfg(not(feature = "desktop"))]
        let mut opus_decoder: Option<()> = None; // 移动端不支持 Opus 解码
        let mut file_transfers: std::collections::HashMap<u32, std::path::PathBuf> =
            std::collections::HashMap::new();

        loop {
            tokio::select! {
                msg = framed.next() => {
                    match msg {
                        Some(Ok(Message::FrameData { seq, timestamp_ms, regions, cursor_x, cursor_y, cursor_shape })) => {
                            if let Some(ref bridge) = ws_bridge {
                                // WebSocket 二进制传输（零 base64 开销）
                                #[allow(clippy::type_complexity)]
                                let ws_regions: Vec<(u32, u32, u32, u32, u8, u8, &[u8])> = regions.iter().map(|r| {
                                    let (enc_type, enc_meta) = match r.encoding {
                                        lan_desk_protocol::message::FrameEncoding::Jpeg { quality } => (0u8, quality),
                                        lan_desk_protocol::message::FrameEncoding::Raw => (1u8, 0u8),
                                        lan_desk_protocol::message::FrameEncoding::H264 { is_keyframe } => (2u8, if is_keyframe { 1u8 } else { 0u8 }),
                                        lan_desk_protocol::message::FrameEncoding::H265 { is_keyframe } => (3u8, if is_keyframe { 1u8 } else { 0u8 }),
                                        lan_desk_protocol::message::FrameEncoding::Av1 { is_keyframe } => (4u8, if is_keyframe { 1u8 } else { 0u8 }),
                                    };
                                    (r.x, r.y, r.width, r.height, enc_type, enc_meta, r.data.as_slice())
                                }).collect();
                                let shape_str = format!("{:?}", cursor_shape);
                                bridge.send_frame(seq, timestamp_ms, &ws_regions, cursor_x, cursor_y, &shape_str);
                            } else {
                                // 回退：base64 + Tauri emit
                                let frame_regions: Vec<FrameRegion> = regions
                                    .into_iter()
                                    .map(|r| {
                                        let is_jpeg = matches!(r.encoding, lan_desk_protocol::message::FrameEncoding::Jpeg { .. });
                                        let b64 = base64_engine.encode(&r.data);
                                        let data_url = if is_jpeg {
                                            format!("data:image/jpeg;base64,{}", b64)
                                        } else {
                                            b64 // Raw 格式保持纯 base64
                                        };
                                        FrameRegion {
                                            x: r.x,
                                            y: r.y,
                                            width: r.width,
                                            height: r.height,
                                            encoding: format!("{:?}", r.encoding),
                                            data_url,
                                        }
                                    })
                                    .collect();

                                let _ = app.emit("frame-update", FrameEvent {
                                    seq,
                                    timestamp_ms,
                                    regions: frame_regions,
                                    cursor_x,
                                    cursor_y,
                                    cursor_shape: format!("{:?}", cursor_shape),
                                });
                            }
                        }
                        Some(Ok(Message::ClipboardUpdate { content_type, data })) => {
                            let _ = clipboard_remote_tx.send(ClipboardChange {
                                content_type,
                                data,
                            }).await;
                        }
                        Some(Ok(Message::Ping { timestamp_ms })) => {
                            let _ = framed.send(Message::Pong { timestamp_ms }).await;
                        }
                        Some(Ok(Message::SystemInfo { cpu_usage, memory_usage, memory_total_mb })) => {
                            let _ = app.emit("system-info", serde_json::json!({
                                "cpu": cpu_usage,
                                "memory": memory_usage,
                                "memory_total_mb": memory_total_mb,
                            }));
                        }
                        Some(Ok(Message::AudioFormat { sample_rate, channels, bits_per_sample, encoding })) => {
                            // 如果远端使用 Opus 编码，创建解码器（仅桌面端支持）
                            #[cfg(feature = "desktop")]
                            if encoding == lan_desk_protocol::message::AudioEncoding::Opus {
                                match lan_desk_audio::opus_codec::OpusDecoder::new(sample_rate, channels) {
                                    Ok(dec) => {
                                        opus_decoder = Some(dec);
                                        tracing::info!("Opus 解码器已创建: {}Hz {}ch", sample_rate, channels);
                                    }
                                    Err(e) => {
                                        tracing::warn!("创建 Opus 解码器失败: {}", e);
                                    }
                                }
                            }
                            if let Some(ref bridge) = ws_bridge {
                                // WebSocket 二进制传输音频格式
                                bridge.send_audio_format(sample_rate, channels, bits_per_sample);
                            }
                            // 同时通过 Tauri emit 发送（前端根据 wsConnection 决定是否处理）
                            let _ = app.emit("audio-format", serde_json::json!({
                                "sample_rate": sample_rate,
                                "channels": channels,
                                "bits_per_sample": bits_per_sample,
                            }));
                        }
                        Some(Ok(Message::AudioData { data, encoding })) => {
                            // 根据编码格式解码
                            let pcm_data = if encoding == lan_desk_protocol::message::AudioEncoding::Opus {
                                #[cfg(feature = "desktop")]
                                {
                                    if let Some(ref mut dec) = opus_decoder {
                                        match dec.decode_opus(&data) {
                                            Ok(pcm) => pcm,
                                            Err(e) => {
                                                tracing::debug!("Opus 解码失败: {}", e);
                                                continue;
                                            }
                                        }
                                    } else {
                                        tracing::warn!("收到 Opus 音频但解码器不可用，丢弃帧");
                                        continue;
                                    }
                                }
                                #[cfg(not(feature = "desktop"))]
                                {
                                    // 移动端不支持 Opus 解码，丢弃
                                    tracing::debug!("移动端不支持 Opus 解码，丢弃帧");
                                    continue;
                                }
                            } else {
                                data // PCM 直接使用
                            };
                            if let Some(ref bridge) = ws_bridge {
                                // WebSocket 二进制传输音频数据（零 base64 开销）
                                bridge.send_audio(&pcm_data);
                            } else {
                                // 回退：base64 + Tauri emit
                                let b64 = base64_engine.encode(&pcm_data);
                                let _ = app.emit("audio-data", b64);
                            }
                        }
                        Some(Ok(Message::Pong { timestamp_ms })) => {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            let rtt = now.saturating_sub(timestamp_ms);
                            let _ = app.emit("network-rtt", rtt);
                        }
                        Some(Ok(Message::ShellStartAck { success, error })) => {
                            let _ = app.emit("shell-start-ack", serde_json::json!({
                                "success": success, "error": error,
                            }));
                        }
                        Some(Ok(Message::ShellData { data })) => {
                            let b64 = base64_engine.encode(&data);
                            let _ = app.emit("shell-output", b64);
                        }
                        Some(Ok(Message::FileTransferStart { filename, size, transfer_id })) => {
                            // 文件大小限制：最大 2GB
                            const MAX_FILE_SIZE: u64 = 2_u64 * 1024 * 1024 * 1024;
                            if size > MAX_FILE_SIZE {
                                tracing::warn!("文件传输拒绝: {} ({} bytes) 超过 2GB 限制", filename, size);
                                continue;
                            }
                            tracing::info!("收到文件传输: {} ({} bytes) id={}", filename, size, transfer_id);
                            let downloads = dirs_next::download_dir()
                                .unwrap_or_else(|| std::path::PathBuf::from("."));
                            // 安全处理：防止路径遍历攻击和 Windows 保留设备名
                            let safe_name = sanitize_filename(&filename);
                            let save_path = downloads.join(&safe_name);
                            let _ = tokio::fs::write(&save_path, b"").await;
                            file_transfers.insert(transfer_id, save_path);
                            let _ = app.emit("file-transfer-start", serde_json::json!({
                                "transfer_id": transfer_id,
                                "filename": filename,
                                "size": size,
                            }));
                        }
                        Some(Ok(Message::FileTransferData { transfer_id, offset, data })) => {
                            use tokio::io::{AsyncWriteExt, AsyncSeekExt};
                            if let Some(path) = file_transfers.get(&transfer_id) {
                                if let Ok(mut f) = tokio::fs::OpenOptions::new()
                                    .write(true)
                                    .open(path)
                                    .await
                                {
                                    // 按 offset 定位写入，支持乱序到达的数据块
                                    if f.seek(std::io::SeekFrom::Start(offset)).await.is_ok() {
                                        let _ = f.write_all(&data).await;
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::FileTransferComplete { transfer_id, checksum })) => {
                            if let Some(path) = file_transfers.remove(&transfer_id) {
                                // 流式 SHA-256 校验，避免大文件占满内存
                                match tokio::fs::File::open(&path).await {
                                    Ok(mut file) => {
                                        use sha2::{Sha256, Digest};
                                        use tokio::io::AsyncReadExt;
                                        let mut hasher = Sha256::new();
                                        let mut buf = vec![0u8; 64 * 1024];
                                        loop {
                                            match file.read(&mut buf).await {
                                                Ok(0) => break,
                                                Ok(n) => hasher.update(&buf[..n]),
                                                Err(e) => {
                                                    tracing::warn!("读取文件校验失败: {:?}: {}", path, e);
                                                    break;
                                                }
                                            }
                                        }
                                        let actual = format!("{:x}", hasher.finalize());
                                        if actual == checksum {
                                            tracing::info!("文件传输完成并校验通过: {:?}", path);
                                        } else {
                                            tracing::warn!("文件校验失败: {:?} (期望: {}..., 实际: {}...)",
                                                path, &checksum[..checksum.len().min(8)], &actual[..actual.len().min(8)]);
                                            // 校验失败，删除不完整的文件
                                            let _ = tokio::fs::remove_file(&path).await;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("打开文件校验失败: {:?}: {}", path, e);
                                    }
                                }
                            }
                            let _ = app.emit("file-transfer-complete", serde_json::json!({
                                "transfer_id": transfer_id,
                            }));
                        }
                        Some(Ok(Message::FileListResponse { request_id, path, entries, error })) => {
                            let _ = app.emit("file-list-response", serde_json::json!({
                                "request_id": request_id,
                                "path": path,
                                "entries": entries.iter().map(|e| serde_json::json!({
                                    "name": e.name,
                                    "is_dir": e.is_dir,
                                    "size": e.size,
                                    "modified_ms": e.modified_ms,
                                })).collect::<Vec<_>>(),
                                "error": error,
                            }));
                        }
                        Some(Ok(Message::DirectoryTransferStart { transfer_id, base_path, total_files, total_size })) => {
                            tracing::info!("收到目录传输开始: {} ({} files, {} bytes)", base_path, total_files, total_size);
                            let downloads = dirs_next::download_dir()
                                .unwrap_or_else(|| std::path::PathBuf::from("."));
                            let safe_name = sanitize_filename(&base_path);
                            let dir_path = downloads.join(&safe_name);
                            let _ = tokio::fs::create_dir_all(&dir_path).await;
                            file_transfers.insert(transfer_id, dir_path);
                            let _ = app.emit("directory-transfer-start", serde_json::json!({
                                "transfer_id": transfer_id,
                                "base_path": base_path,
                                "total_files": total_files,
                                "total_size": total_size,
                            }));
                        }
                        Some(Ok(Message::DirectoryEntry { transfer_id, relative_path, is_dir, size: _ })) => {
                            if let Some(base_path) = file_transfers.get(&transfer_id) {
                                // 逐路径组件过滤，防止路径遍历攻击
                                let safe_relative: std::path::PathBuf = relative_path
                                    .replace('\\', "/")
                                    .split('/')
                                    .filter(|c| !c.is_empty() && *c != "." && *c != "..")
                                    .collect();
                                let target = base_path.join(&safe_relative);
                                if is_dir {
                                    let _ = tokio::fs::create_dir_all(&target).await;
                                }
                            }
                        }
                        Some(Ok(Message::FileTransferResume { transfer_id, offset })) => {
                            let _ = app.emit("file-transfer-resume", serde_json::json!({
                                "transfer_id": transfer_id,
                                "offset": offset,
                            }));
                        }
                        Some(Ok(Message::MonitorList { monitors })) => {
                            // 缓存到 ConnState（解决前端 RemoteView 尚未挂载时事件丢失的竞态）
                            {
                                let mut conn = conn_handle.write().await;
                                conn.remote_monitors = monitors.clone();
                            }
                            let _ = app.emit("monitor-list", serde_json::json!({
                                "monitors": monitors.iter().map(|m| serde_json::json!({
                                    "index": m.index,
                                    "name": m.name,
                                    "width": m.width,
                                    "height": m.height,
                                    "is_primary": m.is_primary,
                                    "left": m.left,
                                    "top": m.top,
                                })).collect::<Vec<_>>()
                            }));
                        }
                        Some(Ok(Message::ChatMessage { text, sender, timestamp_ms })) => {
                            let _ = app.emit("chat-message", serde_json::json!({
                                "text": text, "sender": sender, "timestamp_ms": timestamp_ms
                            }));
                        }
                        Some(Ok(Message::RebootPending { estimated_seconds })) => {
                            tracing::info!("收到远程重启通知，预计 {} 秒后重启", estimated_seconds);
                            let _ = app.emit("reboot-pending", serde_json::json!({
                                "estimated_seconds": estimated_seconds
                            }));
                        }
                        Some(Err(e)) => {
                            tracing::warn!("接收消息错误: {}", e);
                            break;
                        }
                        None => {
                            tracing::info!("被控端断开连接");
                            break;
                        }
                        _ => {}
                    }
                }
                Some(input_msg) = input_rx.recv() => {
                    if framed.send(input_msg).await.is_err() {
                        break;
                    }
                }
                Some(change) = clipboard_local_rx.recv() => {
                    let msg = Message::ClipboardUpdate {
                        content_type: change.content_type,
                        data: change.data,
                    };
                    if framed.send(msg).await.is_err() {
                        break;
                    }
                }
                _ = heartbeat.tick() => {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if framed.send(Message::Ping { timestamp_ms: ts }).await.is_err() {
                        break;
                    }
                }
            }
        }

        let mut conn = conn_handle.write().await;
        conn.input_tx = None;
        conn.info = None;
        conn.remote_monitors.clear();
        if !conn_id_clone.is_empty() {
            conn.remove_connection(&conn_id_clone);
        }
        if let Err(e) = app.emit(
            "connection-closed",
            serde_json::json!({
                "connection_id": conn_id_clone,
            }),
        ) {
            tracing::debug!("发送连接关闭事件失败: {}", e);
        }
    });

    Ok(conn_id)
}

// ──────────────── 重连 ────────────────

/// 使用上次保存的 PIN 重新连接到对端
#[tauri::command]
pub async fn reconnect_to_peer(
    app: AppHandle,
    addr: String,
    role: String,
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let last_pin = state.auth.read().await.last_pin.clone();
    if last_pin.is_empty() {
        return Err("[ERR_NO_PIN] No PIN available, please connect normally first".to_string());
    }
    let pin = last_pin.as_str().to_string();
    let cid = connection_id.clone().unwrap_or_default();

    // 先断开已有连接
    {
        let conn = state.conn.read().await;
        if let Some(tx) = conn.get_input_tx(&cid) {
            let _ = tx.send(Message::Disconnect).await;
        } else if let Some(tx) = conn.input_tx.as_ref() {
            let _ = tx.send(Message::Disconnect).await;
        }
    }
    // 等待连接清理
    tokio::time::sleep(Duration::from_millis(200)).await;
    {
        let mut conn = state.conn.write().await;
        conn.input_tx = None;
        conn.info = None;
        if !cid.is_empty() {
            conn.remove_connection(&cid);
        }
    }

    let role_opt = if role.is_empty() { None } else { Some(role) };
    connect_to_peer(app, addr, pin, role_opt, connection_id, state).await
}

// ──────────────── 断开/状态 ────────────────

#[tauri::command]
pub async fn disconnect(
    connection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    let cid = connection_id.unwrap_or_default();
    if let Some(tx) = conn.get_input_tx(&cid) {
        let _ = tx.send(Message::Disconnect).await;
    } else if let Some(tx) = conn.input_tx.as_ref() {
        let _ = tx.send(Message::Disconnect).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<String, String> {
    let conn = state.conn.read().await;
    if let Some(info) = conn.info.as_ref() {
        Ok(format!("connected:{}", info.remote_addr))
    } else {
        Ok("disconnected".to_string())
    }
}

/// WebSocket 桥接信息（端口 + 认证 token）
#[derive(Debug, Clone, Serialize)]
pub struct WsBridgeInfo {
    pub port: u16,
    pub token: String,
}

/// 获取 WebSocket 桥接服务器端口和认证 token
#[tauri::command]
pub async fn get_ws_port(state: State<'_, AppState>) -> Result<WsBridgeInfo, String> {
    let conn = state.conn.read().await;
    conn.ws_bridge
        .as_ref()
        .map(|b| WsBridgeInfo {
            port: b.port(),
            token: b.token().to_string(),
        })
        .ok_or_else(|| "[ERR_WS_NOT_STARTED] WebSocket bridge not started".to_string())
}

#[tauri::command]
pub async fn set_clipboard_sync(
    state: State<'_, AppState>,
    enabled: bool,
    _connection_id: Option<String>,
) -> Result<(), String> {
    let conn = state.conn.read().await;
    conn.clipboard_enabled
        .store(enabled, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("剪贴板同步已{}", if enabled { "启用" } else { "禁用" });
    Ok(())
}

/// 获取缓存的远程显示器列表（解决 MonitorList 事件竞态：消息可能在前端挂载前到达）
#[tauri::command]
pub async fn get_remote_monitors(
    state: State<'_, AppState>,
) -> Result<Vec<lan_desk_protocol::message::MonitorInfo>, String> {
    let conn = state.conn.read().await;
    Ok(conn.remote_monitors.clone())
}

// ──────────────── TOFU 证书管理 ────────────────

/// 已信任主机信息
#[derive(Debug, Clone, Serialize)]
pub struct TrustedHostInfo {
    pub host: String,
    pub fingerprint: String,
}

/// 列出所有已信任的主机及其证书指纹
#[tauri::command]
pub async fn list_trusted_hosts(app: tauri::AppHandle) -> Result<Vec<TrustedHostInfo>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("[ERR_DATA_DIR] Failed to get data directory: {}", e))?;
    let fp_path = data_dir.join("trusted_fingerprints.json");
    let map = load_trusted_fingerprints(&fp_path);
    let hosts: Vec<TrustedHostInfo> = map
        .into_iter()
        .map(|(host, fingerprint)| TrustedHostInfo { host, fingerprint })
        .collect();
    Ok(hosts)
}

/// 移除指定主机的信任记录
#[tauri::command]
pub async fn remove_trusted_host(app: tauri::AppHandle, host: String) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("[ERR_DATA_DIR] Failed to get data directory: {}", e))?;
    let fp_path = data_dir.join("trusted_fingerprints.json");

    // 更新内存缓存
    {
        let mut guard = TRUSTED_FINGERPRINTS
            .write()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut map) = *guard {
            if map.remove(&host).is_none() {
                return Err(format!("[ERR_HOST_NOT_FOUND] Host not found: {}", host));
            }
            // 持久化到磁盘
            save_trusted_fingerprints(&fp_path, map);
        } else {
            return Err("[ERR_TRUST_NOT_INIT] Trust list not initialized".to_string());
        }
    }

    tracing::info!("已移除信任主机: {}", host);
    Ok(())
}
