//! 本地 WebSocket 二进制传输桥接
//!
//! 在 127.0.0.1 上启动 WebSocket 服务器，将高带宽的帧数据和音频数据
//! 以二进制格式直接推送给前端，绕过 Tauri 的 JSON-based IPC，
//! 消除 base64 编码的 ~33% 开销。

use std::net::SocketAddr;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

/// 二进制消息类型标识
const MSG_TYPE_FRAME: u8 = 0x01;
const MSG_TYPE_AUDIO: u8 = 0x02;
const MSG_TYPE_AUDIO_FORMAT: u8 = 0x03;

/// 光标形状枚举到 u8 的映射
fn cursor_shape_to_u8(shape: &str) -> u8 {
    match shape {
        "Arrow" => 0,
        "IBeam" => 1,
        "Hand" => 2,
        "Crosshair" => 3,
        "ResizeNS" => 4,
        "ResizeEW" => 5,
        "ResizeNESW" => 6,
        "ResizeNWSE" => 7,
        "Move" => 8,
        "Wait" => 9,
        "Help" => 10,
        "NotAllowed" => 11,
        "Hidden" => 12,
        _ => 0,
    }
}

/// WebSocket 桥接服务器
#[derive(Clone)]
pub struct WsBridge {
    port: u16,
    token: Arc<String>,
    data_tx: broadcast::Sender<Vec<u8>>,
}

impl WsBridge {
    /// 启动 WebSocket 服务器，绑定到 127.0.0.1 的随机端口
    pub async fn start() -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let port = addr.port();

        // 生成 32 字节随机 token（hex 编码为 64 字符）
        let token = {
            use rand::Rng;
            let mut bytes = [0u8; 32];
            rand::rngs::OsRng.fill(&mut bytes);
            Arc::new(hex::encode(bytes))
        };

        // 广播 channel：容量 128，允许帧丢弃（实时流可容忍丢帧）
        let (data_tx, _) = broadcast::channel::<Vec<u8>>(128);
        let data_tx_clone = data_tx.clone();
        let token_clone = token.clone();

        // 启动 accept 循环
        tokio::spawn(async move {
            Self::accept_loop(listener, data_tx_clone, token_clone).await;
        });

        info!("WebSocket 桥接服务器已启动: ws://127.0.0.1:{}", port);

        Ok(Self {
            port,
            token,
            data_tx,
        })
    }

    /// 获取服务器端口
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 获取认证 token
    pub fn token(&self) -> &str {
        &self.token
    }

    /// 接受新的 WebSocket 连接
    async fn accept_loop(
        listener: TcpListener,
        data_tx: broadcast::Sender<Vec<u8>>,
        token: Arc<String>,
    ) {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    debug!("WebSocket 新连接: {}", addr);
                    let mut rx = data_tx.subscribe();
                    let token = token.clone();
                    tokio::spawn(async move {
                        Self::handle_client(stream, addr, &mut rx, &token).await;
                    });
                }
                Err(e) => {
                    warn!("WebSocket accept 失败: {}", e);
                }
            }
        }
    }

    /// 处理单个 WebSocket 客户端
    async fn handle_client(
        stream: tokio::net::TcpStream,
        addr: SocketAddr,
        rx: &mut broadcast::Receiver<Vec<u8>>,
        expected_token: &str,
    ) {
        let ws_stream = match tokio_tungstenite::accept_async(stream).await {
            Ok(ws) => ws,
            Err(e) => {
                warn!("WebSocket 握手失败 {}: {}", addr, e);
                return;
            }
        };

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Token 认证：客户端连接后第一条消息必须是正确的 token
        let authenticated =
            match tokio::time::timeout(std::time::Duration::from_secs(5), ws_receiver.next()).await
            {
                Ok(Some(Ok(Message::Text(msg)))) => {
                    if msg.as_str() == expected_token {
                        true
                    } else {
                        warn!("WebSocket 客户端 {} token 验证失败", addr);
                        let _ = ws_sender.send(Message::Close(None)).await;
                        false
                    }
                }
                _ => {
                    warn!("WebSocket 客户端 {} 未在 5 秒内发送 token", addr);
                    let _ = ws_sender.send(Message::Close(None)).await;
                    false
                }
            };

        if !authenticated {
            return;
        }

        debug!("WebSocket 客户端 {} 认证通过", addr);

        // 启动接收任务（忽略客户端发来的消息，只用于检测断开）
        let mut recv_task = tokio::spawn(async move {
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {} // 忽略客户端消息
                }
            }
        });

        // 转发 broadcast 数据到 WebSocket
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(data) => {
                            if ws_sender.send(Message::Binary(data)).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            debug!("WebSocket 客户端 {} 滞后 {} 消息", addr, n);
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = &mut recv_task => {
                    // 客户端断开
                    break;
                }
            }
        }

        debug!("WebSocket 客户端 {} 断开", addr);
    }

    /// 发送帧数据（二进制格式）
    ///
    /// 格式：[0x01][seq:u64][ts:u64][cx:f64][cy:f64][shape:u8][count:u8][regions...]
    /// 每个 region：[x:u32][y:u32][w:u32][h:u32][enc_type:u8][enc_meta:u8][data_len:u32][data...]
    #[allow(clippy::type_complexity)]
    pub fn send_frame(
        &self,
        seq: u64,
        timestamp_ms: u64,
        regions: &[(u32, u32, u32, u32, u8, u8, &[u8])], // (x, y, w, h, enc_type, enc_meta, data)
        cursor_x: f64,
        cursor_y: f64,
        cursor_shape: &str,
    ) {
        if regions.len() > 255 {
            warn!("帧区域数 {} 超过 u8 上限 255，已截断", regions.len());
        }
        let buf = encode_frame(seq, timestamp_ms, regions, cursor_x, cursor_y, cursor_shape);
        if self.data_tx.send(buf).is_err() {
            debug!("帧数据广播失败（无活跃接收者）");
        }
    }

    /// 发送音频数据（二进制格式）
    pub fn send_audio(&self, data: &[u8]) {
        let buf = encode_audio(data);
        if self.data_tx.send(buf).is_err() {
            debug!("音频数据广播失败（无活跃接收者）");
        }
    }

    /// 发送音频格式信息
    pub fn send_audio_format(&self, sample_rate: u32, channels: u16, bits_per_sample: u16) {
        let buf = encode_audio_format(sample_rate, channels, bits_per_sample);
        if self.data_tx.send(buf).is_err() {
            debug!("音频格式广播失败（无活跃接收者）");
        }
    }
}

/// 编码帧数据为二进制格式（纯数据编码，无 I/O）
///
/// 格式：[0x01][seq:u64][ts:u64][cx:f64][cy:f64][shape:u8][count:u8][regions...]
/// 每个 region：[x:u32][y:u32][w:u32][h:u32][enc_type:u8][enc_meta:u8][data_len:u32][data...]
#[allow(clippy::type_complexity)]
pub fn encode_frame(
    seq: u64,
    timestamp_ms: u64,
    regions: &[(u32, u32, u32, u32, u8, u8, &[u8])],
    cursor_x: f64,
    cursor_y: f64,
    cursor_shape: &str,
) -> Vec<u8> {
    let data_size: usize = regions.iter().map(|r| 22 + r.6.len()).sum();
    let header_size = 1 + 8 + 8 + 8 + 8 + 1 + 1;
    let mut buf = Vec::with_capacity(header_size + data_size);

    buf.push(MSG_TYPE_FRAME);
    buf.extend_from_slice(&seq.to_le_bytes());
    buf.extend_from_slice(&timestamp_ms.to_le_bytes());
    buf.extend_from_slice(&cursor_x.to_le_bytes());
    buf.extend_from_slice(&cursor_y.to_le_bytes());
    buf.push(cursor_shape_to_u8(cursor_shape));
    let region_count = regions.len().min(255);
    buf.push(region_count as u8);

    for &(x, y, w, h, enc_type, enc_meta, data) in &regions[..region_count] {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.extend_from_slice(&w.to_le_bytes());
        buf.extend_from_slice(&h.to_le_bytes());
        buf.push(enc_type);
        buf.push(enc_meta);
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(data);
    }

    buf
}

/// 编码音频数据为二进制格式（纯数据编码，无 I/O）
///
/// 格式：[0x02][data...]
pub fn encode_audio(data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + data.len());
    buf.push(MSG_TYPE_AUDIO);
    buf.extend_from_slice(data);
    buf
}

/// 编码音频格式信息为二进制格式（纯数据编码，无 I/O）
///
/// 格式：[0x03][sample_rate:u32][channels:u16][bits_per_sample:u16]
pub fn encode_audio_format(sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(9);
    buf.push(MSG_TYPE_AUDIO_FORMAT);
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_shape_to_u8() {
        assert_eq!(cursor_shape_to_u8("Arrow"), 0);
        assert_eq!(cursor_shape_to_u8("IBeam"), 1);
        assert_eq!(cursor_shape_to_u8("Hand"), 2);
        assert_eq!(cursor_shape_to_u8("Crosshair"), 3);
        assert_eq!(cursor_shape_to_u8("ResizeNS"), 4);
        assert_eq!(cursor_shape_to_u8("ResizeEW"), 5);
        assert_eq!(cursor_shape_to_u8("ResizeNESW"), 6);
        assert_eq!(cursor_shape_to_u8("ResizeNWSE"), 7);
        assert_eq!(cursor_shape_to_u8("Move"), 8);
        assert_eq!(cursor_shape_to_u8("Wait"), 9);
        assert_eq!(cursor_shape_to_u8("Help"), 10);
        assert_eq!(cursor_shape_to_u8("NotAllowed"), 11);
        assert_eq!(cursor_shape_to_u8("Hidden"), 12);
        assert_eq!(cursor_shape_to_u8("Unknown"), 0); // 未知类型默认为 0
    }

    #[test]
    fn test_encode_audio_format() {
        let buf = encode_audio_format(48000, 2, 16);
        assert_eq!(buf[0], MSG_TYPE_AUDIO_FORMAT);
        assert_eq!(u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]), 48000);
        assert_eq!(u16::from_le_bytes([buf[5], buf[6]]), 2);
        assert_eq!(u16::from_le_bytes([buf[7], buf[8]]), 16);
        assert_eq!(buf.len(), 9);
    }

    #[test]
    fn test_encode_audio() {
        let audio_data = vec![0x10, 0x20, 0x30, 0x40];
        let buf = encode_audio(&audio_data);
        assert_eq!(buf[0], MSG_TYPE_AUDIO);
        assert_eq!(&buf[1..], &audio_data);
        assert_eq!(buf.len(), 5);
    }

    #[test]
    fn test_encode_audio_empty() {
        let buf = encode_audio(&[]);
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0], MSG_TYPE_AUDIO);
    }

    #[test]
    fn test_encode_frame_header() {
        let buf = encode_frame(
            42,     // seq
            1000,   // timestamp_ms
            &[],    // 无 region
            0.5,    // cursor_x
            0.75,   // cursor_y
            "Hand", // cursor_shape
        );

        // 验证消息类型
        assert_eq!(buf[0], MSG_TYPE_FRAME);

        // 验证 seq (offset 1..9)
        assert_eq!(u64::from_le_bytes(buf[1..9].try_into().unwrap()), 42);
        // 验证 timestamp_ms (offset 9..17)
        assert_eq!(u64::from_le_bytes(buf[9..17].try_into().unwrap()), 1000);
        // 验证 cursor_x (offset 17..25)
        assert_eq!(f64::from_le_bytes(buf[17..25].try_into().unwrap()), 0.5);
        // 验证 cursor_y (offset 25..33)
        assert_eq!(f64::from_le_bytes(buf[25..33].try_into().unwrap()), 0.75);
        // 验证 cursor_shape (offset 33)
        assert_eq!(buf[33], 2); // Hand = 2
                                // 验证 region count (offset 34)
        assert_eq!(buf[34], 0);

        // 无 region 时总长度 = 1+8+8+8+8+1+1 = 35
        assert_eq!(buf.len(), 35);
    }

    #[test]
    fn test_encode_frame_with_region() {
        let region_data = vec![0xAA, 0xBB, 0xCC];
        let regions: Vec<(u32, u32, u32, u32, u8, u8, &[u8])> =
            vec![(10, 20, 640, 480, 0, 0, &region_data)];

        let buf = encode_frame(1, 500, &regions, 0.0, 0.0, "Arrow");

        // header = 35 bytes, region = 4+4+4+4+1+1+4+3 = 25 bytes
        assert_eq!(buf.len(), 35 + 25);
        assert_eq!(buf[34], 1); // region count = 1

        // 解析 region（offset 35 开始）
        let off = 35;
        assert_eq!(
            u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()),
            10
        ); // x
        assert_eq!(
            u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap()),
            20
        ); // y
        assert_eq!(
            u32::from_le_bytes(buf[off + 8..off + 12].try_into().unwrap()),
            640
        ); // w
        assert_eq!(
            u32::from_le_bytes(buf[off + 12..off + 16].try_into().unwrap()),
            480
        ); // h
        assert_eq!(buf[off + 16], 0); // enc_type
        assert_eq!(buf[off + 17], 0); // enc_meta
        assert_eq!(
            u32::from_le_bytes(buf[off + 18..off + 22].try_into().unwrap()),
            3
        ); // data_len
        assert_eq!(&buf[off + 22..off + 25], &[0xAA, 0xBB, 0xCC]); // data
    }

    #[test]
    fn test_encode_frame_multiple_regions() {
        let d1 = vec![0x01];
        let d2 = vec![0x02, 0x03];
        let regions: Vec<(u32, u32, u32, u32, u8, u8, &[u8])> =
            vec![(0, 0, 100, 100, 0, 0, &d1), (100, 0, 200, 200, 2, 1, &d2)];

        let buf = encode_frame(5, 0, &regions, 0.0, 0.0, "Arrow");
        assert_eq!(buf[34], 2); // region count = 2

        // region1: 22 + 1 = 23 bytes, region2: 22 + 2 = 24 bytes
        assert_eq!(buf.len(), 35 + 23 + 24);
    }
}
