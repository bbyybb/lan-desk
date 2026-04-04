use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tracing::{debug, warn};

use crate::message::DiscoveryMessage;
use crate::DEFAULT_UDP_PORT;

/// 发现到的远程设备
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    pub addr: SocketAddr,
    pub hostname: String,
    pub tcp_port: u16,
    pub os: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub device_id: String,
    pub last_seen: Instant,
}

/// 局域网设备发现服务
pub struct DiscoveryService {
    socket: UdpSocket,
    hostname: String,
    tcp_port: u16,
    screen_width: u32,
    screen_height: u32,
    device_id: String,
}

impl DiscoveryService {
    /// 绑定 UDP socket 并启用广播
    /// 如果默认端口被占用，尝试绑定到随机端口（仍可发送广播发现其他设备）
    pub async fn bind(hostname: String, tcp_port: u16) -> anyhow::Result<Self> {
        Self::bind_with_screen(hostname, tcp_port, 0, 0, String::new()).await
    }

    /// 绑定 UDP socket，带屏幕尺寸和设备 ID 信息
    pub async fn bind_with_screen(
        hostname: String,
        tcp_port: u16,
        screen_width: u32,
        screen_height: u32,
        device_id: String,
    ) -> anyhow::Result<Self> {
        let socket = match UdpSocket::bind(("0.0.0.0", DEFAULT_UDP_PORT)).await {
            Ok(s) => {
                debug!("发现服务已绑定到 UDP 端口 {}", DEFAULT_UDP_PORT);
                s
            }
            Err(e) => {
                warn!("UDP 端口 {} 被占用 ({}), 尝试随机端口", DEFAULT_UDP_PORT, e);
                let s = UdpSocket::bind("0.0.0.0:0").await?;
                debug!("发现服务已绑定到随机 UDP 端口 {:?}", s.local_addr());
                s
            }
        };
        socket.set_broadcast(true)?;

        Ok(Self {
            socket,
            hostname,
            tcp_port,
            screen_width,
            screen_height,
            device_id,
        })
    }

    /// 发送广播 Ping
    pub async fn send_ping(&self) -> anyhow::Result<()> {
        let msg = DiscoveryMessage::Ping {
            hostname: self.hostname.clone(),
            tcp_port: self.tcp_port,
            device_id: self.device_id.clone(),
        };
        let data = rmp_serde::to_vec_named(&msg)?;
        self.socket
            .send_to(&data, ("255.255.255.255", DEFAULT_UDP_PORT))
            .await?;
        debug!("已发送发现广播");
        Ok(())
    }

    /// 发送 Pong 响应到指定地址
    pub async fn send_pong(&self, target: SocketAddr) -> anyhow::Result<()> {
        let msg = DiscoveryMessage::Pong {
            hostname: self.hostname.clone(),
            tcp_port: self.tcp_port,
            os: std::env::consts::OS.to_string(),
            screen_width: self.screen_width,
            screen_height: self.screen_height,
            device_id: self.device_id.clone(),
        };
        let data = rmp_serde::to_vec_named(&msg)?;
        self.socket.send_to(&data, target).await?;
        Ok(())
    }

    /// 接收一条发现消息
    pub async fn recv(&self) -> anyhow::Result<(DiscoveryMessage, SocketAddr)> {
        // 限制接收缓冲区为 2KB，防止恶意广播包
        let mut buf = vec![0u8; 2048];
        let (len, addr) = self.socket.recv_from(&mut buf).await?;
        // MessagePack 没有内建大小限制，但 UDP 包本身已受 buf 大小限制
        let msg: DiscoveryMessage = rmp_serde::from_slice(&buf[..len])?;
        Ok((msg, addr))
    }

    /// 发送 Ping 并收集一段时间内的响应
    pub async fn discover(&self, timeout: Duration) -> anyhow::Result<Vec<DiscoveredPeer>> {
        self.send_ping().await?;

        let mut peers = Vec::new();
        let deadline = Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }

            match tokio::time::timeout(remaining, self.recv()).await {
                Ok(Ok((
                    DiscoveryMessage::Pong {
                        hostname,
                        tcp_port,
                        os,
                        screen_width,
                        screen_height,
                        device_id,
                    },
                    addr,
                ))) => {
                    peers.push(DiscoveredPeer {
                        addr: SocketAddr::new(addr.ip(), tcp_port),
                        hostname,
                        tcp_port,
                        os,
                        screen_width,
                        screen_height,
                        device_id,
                        last_seen: Instant::now(),
                    });
                }
                Ok(Ok((DiscoveryMessage::Ping { .. }, _))) => {
                    // 忽略自己的 Ping
                }
                Ok(Err(e)) => {
                    warn!("接收发现消息失败: {}", e);
                }
                Err(_) => {
                    // 超时
                    break;
                }
            }
        }

        Ok(peers)
    }
}
