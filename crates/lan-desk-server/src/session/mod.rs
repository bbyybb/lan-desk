mod auth;
pub mod capture;
mod file_transfer;
mod platform;
mod reboot;
mod screen_blank;
mod shell;

pub use capture::EncodedFrame;

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;
use tracing::{debug, info, warn};

use tokio::sync::mpsc as tokio_mpsc;

use lan_desk_capture::create_capturer;
use lan_desk_clipboard::{ClipboardChange, ClipboardManager};
use lan_desk_input::{create_injector, InputInjector};
use lan_desk_protocol::codec::LanDeskCodec;
use lan_desk_protocol::message::{Message, SessionRole};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{AuthCallback, RateLimiter};

use capture::{CaptureCommand, FrameSource};

const MIN_FPS: u32 = 5;
const MAX_FPS: u32 = 60;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
/// Shell 空闲超时：30 分钟无活动自动关闭 PTY
const SHELL_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 自适应比特率评估间隔（秒）
const ADAPTIVE_EVAL_INTERVAL: Duration = Duration::from_secs(10);

/// 网络质量指标（自适应比特率）
struct NetworkMetrics {
    rtt_samples: VecDeque<u64>,
    bytes_sent_window: u64,
    current_quality: u32,
    current_fps: u32,
    last_eval_time: Instant,
}

impl NetworkMetrics {
    fn new(initial_quality: u32, initial_fps: u32) -> Self {
        Self {
            rtt_samples: VecDeque::with_capacity(64),
            bytes_sent_window: 0,
            current_quality: initial_quality,
            current_fps: initial_fps,
            last_eval_time: Instant::now(),
        }
    }

    fn record_rtt(&mut self, rtt_ms: u64) {
        self.rtt_samples.push_back(rtt_ms);
        if self.rtt_samples.len() > 60 {
            self.rtt_samples.pop_front();
        }
    }

    fn avg_rtt(&self) -> u64 {
        if self.rtt_samples.is_empty() {
            return 0;
        }
        let sum: u64 = self.rtt_samples.iter().sum();
        sum / self.rtt_samples.len() as u64
    }

    /// 评估网络质量并调整参数，返回 (new_quality, new_fps) 如果有变化
    fn evaluate(&mut self, bandwidth_limit: u64) -> Option<(u32, u32)> {
        if self.last_eval_time.elapsed() < ADAPTIVE_EVAL_INTERVAL {
            return None;
        }
        self.last_eval_time = Instant::now();

        let avg_rtt = self.avg_rtt();
        let utilization = if bandwidth_limit > 0 {
            (self.bytes_sent_window as f64
                / (bandwidth_limit as f64 * ADAPTIVE_EVAL_INTERVAL.as_secs() as f64)
                * 100.0) as u64
        } else {
            50 // 无限制时视为中等利用率
        };

        let mut changed = false;

        if avg_rtt > 200 || utilization > 90 {
            // 网络拥塞：快速降级
            let new_quality = self.current_quality.saturating_sub(10).max(30);
            let new_fps = self.current_fps.saturating_sub(10).max(MIN_FPS);
            if new_quality != self.current_quality || new_fps != self.current_fps {
                self.current_quality = new_quality;
                self.current_fps = new_fps;
                changed = true;
            }
        } else if avg_rtt < 50 && utilization < 50 {
            // 网络良好：渐进升级
            let new_quality = (self.current_quality + 5).min(90);
            let new_fps = (self.current_fps + 5).min(MAX_FPS);
            if new_quality != self.current_quality || new_fps != self.current_fps {
                self.current_quality = new_quality;
                self.current_fps = new_fps;
                changed = true;
            }
        }

        self.bytes_sent_window = 0;

        if changed {
            Some((self.current_quality, self.current_fps))
        } else {
            None
        }
    }
}

/// 单个控制会话
pub struct Session<S: AsyncRead + AsyncWrite + Unpin> {
    framed: Framed<S, LanDeskCodec>,
    addr: SocketAddr,
    /// 远程控制端主机名（Hello 握手中获取）
    pub(crate) remote_hostname: String,
    injector: Box<dyn InputInjector>,
    frame_source: FrameSource,
    pub(crate) capture_cmd_tx: Option<tokio_mpsc::Sender<CaptureCommand>>,
    last_recv_time: Instant,
    bytes_sent: u64,
    pub(crate) granted_role: SessionRole,
    pub(crate) file_transfers: std::collections::HashMap<u32, (std::path::PathBuf, u64)>,
    bandwidth_limit: u64,
    bytes_this_second: u64,
    second_start: Instant,
    /// PTY 写入端（Shell stdin）
    pub(crate) pty_writer: Option<Box<dyn std::io::Write + Send>>,
    /// PTY 子进程
    pub(crate) pty_child: Option<Box<dyn portable_pty::Child + Send>>,
    /// PTY master（用于 resize）
    pub(crate) pty_master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    /// PTY 输出 channel（Shell stdout -> 网络）
    pub(crate) pty_output_rx: Option<tokio_mpsc::Receiver<Vec<u8>>>,
    /// 是否允许远程 Shell 访问
    pub(crate) shell_enabled: bool,
    /// PTY 最后活动时间（用于空闲超时检测）
    pty_last_activity: Option<Instant>,
    /// 最后输入时间（用于会话空闲超时检测）
    last_input_time: Instant,
    /// 会话空闲超时时长（0 表示不启用）
    idle_timeout: Duration,
    /// 断开时是否自动锁屏
    lock_on_disconnect: bool,
    /// 屏幕遮蔽器
    screen_blanker: Option<screen_blank::ScreenBlanker>,
    /// 网络质量指标（自适应比特率）
    network_metrics: NetworkMetrics,
    /// 聊天消息转发到本地前端
    pub(crate) chat_tx: Option<tokio_mpsc::Sender<(String, String, u64)>>,
    /// 被控端主机发来的聊天消息（广播接收端）
    pub(crate) host_chat_rx: Option<tokio::sync::broadcast::Receiver<(String, String, u64)>>,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Session<S> {
    pub async fn new(
        stream: S,
        addr: SocketAddr,
        control_pin: &str,
        view_pin: &str,
        auth_callback: Option<&AuthCallback>,
    ) -> anyhow::Result<Self> {
        let mut framed = Framed::new(stream, LanDeskCodec::new());

        let msg = framed
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("连接在握手前关闭"))??;

        match msg {
            Message::Hello {
                version,
                hostname,
                pin,
                pin_salt,
                requested_role,
                ..
            } => {
                info!(
                    "收到来自 {} ({}) 的 Hello, 版本 {}, 请求角色 {:?}",
                    hostname, addr, version, requested_role
                );

                let granted_role = Self::verify_and_auth(
                    &mut framed,
                    addr,
                    &hostname,
                    &pin,
                    &pin_salt,
                    requested_role,
                    control_pin,
                    view_pin,
                    auth_callback,
                    None,
                )
                .await?;

                // 初始化捕获和注入
                let capturer = create_capturer()?;
                let (w, h) = capturer.screen_size();

                info!(
                    "握手完成，来自 {} ({})，屏幕 {}x{}，角色 {:?}",
                    hostname, addr, w, h, granted_role
                );

                let injector = create_injector()?;

                // 设置活跃显示器边界：必须与 create_capturer() 使用的 display index 0 一致
                if let Ok(monitors) = lan_desk_capture::list_monitors() {
                    if let Some(m) = monitors.iter().find(|m| m.index == 0).or(monitors.first()) {
                        injector.set_active_monitor(m.left, m.top, m.width, m.height);
                        info!(
                            "输入坐标映射到显示器 {} ({}x{} at {},{})",
                            m.index, m.width, m.height, m.left, m.top
                        );
                    }
                }

                let (frame_tx, frame_rx) = tokio_mpsc::channel::<EncodedFrame>(4);
                let (cmd_tx, cmd_rx) = tokio_mpsc::channel::<CaptureCommand>(4);

                std::thread::Builder::new()
                    .name("screen-capture".to_string())
                    .spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_time()
                            .build()
                            .unwrap();
                        rt.block_on(async {
                            Self::capture_loop(capturer, frame_tx, cmd_rx).await;
                        });
                    })?;

                Ok(Self {
                    framed,
                    addr,
                    remote_hostname: hostname.clone(),
                    injector,
                    frame_source: FrameSource::Dedicated(frame_rx),
                    capture_cmd_tx: Some(cmd_tx),
                    last_recv_time: Instant::now(),
                    bytes_sent: 0,
                    granted_role,
                    file_transfers: std::collections::HashMap::new(),
                    bandwidth_limit: 0,
                    bytes_this_second: 0,
                    second_start: Instant::now(),
                    pty_writer: None,
                    pty_child: None,
                    pty_master: None,
                    pty_output_rx: None,
                    shell_enabled: false,
                    pty_last_activity: None,
                    last_input_time: Instant::now(),
                    idle_timeout: Duration::ZERO,
                    lock_on_disconnect: false,
                    screen_blanker: None,
                    network_metrics: NetworkMetrics::new(75, 30),
                    chat_tx: None,
                    host_chat_rx: None,
                })
            }
            _ => {
                anyhow::bail!("期望 Hello 消息，收到了其他类型");
            }
        }
    }

    /// 使用共享帧广播创建会话（不启动独立捕获线程）
    // PIN 验证逻辑已通过 verify_and_auth 方法提取，消除了重复代码
    #[allow(clippy::too_many_arguments)]
    pub async fn new_with_broadcast(
        stream: S,
        addr: SocketAddr,
        control_pin: &str,
        view_pin: &str,
        auth_callback: Option<&AuthCallback>,
        frame_rx: tokio::sync::broadcast::Receiver<EncodedFrame>,
        rate_limiter: Arc<Mutex<RateLimiter>>,
        shell_enabled: bool,
        idle_timeout: Duration,
        lock_on_disconnect: bool,
    ) -> anyhow::Result<Self> {
        let mut framed = Framed::new(stream, LanDeskCodec::new());

        let msg = framed
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("连接在握手前关闭"))??;

        match msg {
            Message::Hello {
                version,
                hostname,
                pin,
                pin_salt,
                requested_role,
                ..
            } => {
                info!(
                    "收到来自 {} ({}) 的 Hello, 版本 {}, 请求角色 {:?}",
                    hostname, addr, version, requested_role
                );

                let granted_role = Self::verify_and_auth(
                    &mut framed,
                    addr,
                    &hostname,
                    &pin,
                    &pin_salt,
                    requested_role,
                    control_pin,
                    view_pin,
                    auth_callback,
                    Some(&rate_limiter),
                )
                .await?;

                let injector = create_injector()?;

                // 设置活跃显示器边界
                if let Ok(monitors) = lan_desk_capture::list_monitors() {
                    if let Some(primary) =
                        monitors.iter().find(|m| m.is_primary).or(monitors.first())
                    {
                        injector.set_active_monitor(
                            primary.left,
                            primary.top,
                            primary.width,
                            primary.height,
                        );
                    }
                }

                info!(
                    "握手完成（共享捕获模式），来自 {} ({})，角色 {:?}",
                    hostname, addr, granted_role
                );

                Ok(Self {
                    framed,
                    addr,
                    remote_hostname: hostname.clone(),
                    injector,
                    frame_source: FrameSource::Broadcast(frame_rx),
                    capture_cmd_tx: None,
                    last_recv_time: Instant::now(),
                    bytes_sent: 0,
                    granted_role,
                    file_transfers: std::collections::HashMap::new(),
                    bandwidth_limit: 0,
                    bytes_this_second: 0,
                    second_start: Instant::now(),
                    pty_writer: None,
                    pty_child: None,
                    pty_master: None,
                    pty_output_rx: None,
                    shell_enabled,
                    pty_last_activity: None,
                    last_input_time: Instant::now(),
                    idle_timeout,
                    lock_on_disconnect,
                    screen_blanker: None,
                    network_metrics: NetworkMetrics::new(75, 30),
                    chat_tx: None,
                    host_chat_rx: None,
                })
            }
            _ => anyhow::bail!("期望 Hello 消息"),
        }
    }

    /// 设置聊天消息转发通道
    pub fn set_chat_tx(&mut self, tx: tokio_mpsc::Sender<(String, String, u64)>) {
        self.chat_tx = Some(tx);
    }

    /// 设置被控端主机聊天接收通道
    pub fn set_host_chat_rx(
        &mut self,
        rx: tokio::sync::broadcast::Receiver<(String, String, u64)>,
    ) {
        self.host_chat_rx = Some(rx);
    }

    /// 是否为仅查看角色
    pub fn is_viewer(&self) -> bool {
        self.granted_role == SessionRole::Viewer
    }

    /// 获取角色字符串
    pub fn role_str(&self) -> &str {
        if self.granted_role == SessionRole::Viewer {
            "Viewer"
        } else {
            "Controller"
        }
    }

    /// 运行会话主循环
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut heartbeat_interval = tokio::time::interval(HEARTBEAT_INTERVAL);

        // 剪贴板
        let (mut clipboard_mgr, mut clipboard_local_rx, clipboard_remote_tx) =
            ClipboardManager::new();
        tokio::spawn(async move {
            clipboard_mgr.run().await;
        });

        // 音频捕获
        let mut _audio_handle: Option<lan_desk_audio::AudioHandle> = None;
        let mut audio_encoding = lan_desk_protocol::message::AudioEncoding::Pcm16;
        let mut opus_encoder: Option<std::sync::Mutex<lan_desk_audio::opus_codec::OpusEncoder>> =
            None;
        let audio_rx = match lan_desk_audio::start_audio_capture() {
            Ok((config, rx, handle)) => {
                _audio_handle = Some(handle);
                // 尝试创建 Opus 编码器
                match lan_desk_audio::opus_codec::OpusEncoder::new(
                    config.sample_rate,
                    config.channels,
                    128_000,
                ) {
                    Ok(enc) => {
                        info!(
                            "Opus 编码器已初始化: {}Hz {}ch 128kbps",
                            config.sample_rate, config.channels
                        );
                        audio_encoding = lan_desk_protocol::message::AudioEncoding::Opus;
                        opus_encoder = Some(std::sync::Mutex::new(enc));
                    }
                    Err(e) => {
                        warn!("Opus 编码器不可用，回退 PCM: {}", e);
                    }
                };
                // 发送音频格式（包含编码类型）
                if let Err(e) = self
                    .framed
                    .send(Message::AudioFormat {
                        sample_rate: config.sample_rate,
                        channels: config.channels,
                        bits_per_sample: config.bits_per_sample,
                        encoding: audio_encoding,
                    })
                    .await
                {
                    warn!("发送音频格式失败: {}", e);
                }
                info!(
                    "音频捕获已启动: {}Hz {}ch, 编码: {:?}",
                    config.sample_rate, config.channels, audio_encoding
                );
                Some(rx)
            }
            Err(e) => {
                warn!("音频捕获启动失败（将不转发音频）: {}", e);
                None
            }
        };

        // 音频消息封装：PCM 或 Opus 编码
        let _audio_encoding_for_bridge = audio_encoding;
        let opus_enc_for_bridge = opus_encoder;
        let (audio_tokio_tx, mut audio_tokio_rx) =
            tokio_mpsc::channel::<(Vec<u8>, lan_desk_protocol::message::AudioEncoding)>(32);
        if let Some(audio_rx) = audio_rx {
            std::thread::Builder::new()
                .name("audio-bridge".to_string())
                .spawn(move || {
                    while let Ok(data) = audio_rx.recv() {
                        if let Some(ref enc) = opus_enc_for_bridge {
                            // Opus 编码：将 PCM 编码为 Opus 帧
                            if let Ok(mut encoder) = enc.lock() {
                                let opus_frames = encoder.encode_pcm(&data);
                                for frame in opus_frames {
                                    if audio_tokio_tx
                                        .blocking_send((
                                            frame,
                                            lan_desk_protocol::message::AudioEncoding::Opus,
                                        ))
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                        } else {
                            // PCM 直接传输
                            if audio_tokio_tx
                                .blocking_send((
                                    data,
                                    lan_desk_protocol::message::AudioEncoding::Pcm16,
                                ))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                })
                .ok();
        }

        // 系统信息定期发送
        let mut sysinfo_interval = tokio::time::interval(Duration::from_secs(3));
        let mut sys = sysinfo::System::new();

        info!("会话 {} 主循环开始", self.addr);

        // 发送远程显示器列表给控制端
        if let Ok(monitors) = lan_desk_capture::list_monitors() {
            if let Err(e) = self.framed.send(Message::MonitorList { monitors }).await {
                warn!("发送显示器列表失败: {}", e);
            }
        }

        // 统一帧接收：转为 tokio_mpsc（broadcast 的接口不同）
        let (unified_tx, mut unified_rx) = tokio_mpsc::channel::<EncodedFrame>(8);
        match std::mem::replace(
            &mut self.frame_source,
            FrameSource::Dedicated(tokio_mpsc::channel(1).1),
        ) {
            FrameSource::Dedicated(mut rx) => {
                let tx = unified_tx.clone();
                tokio::spawn(async move {
                    while let Some(frame) = rx.recv().await {
                        if tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                });
            }
            FrameSource::Broadcast(mut rx) => {
                let tx = unified_tx.clone();
                tokio::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(frame) => {
                                if tx.send(frame).await.is_err() {
                                    break;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                debug!("广播接收滞后 {} 帧", n);
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        }
        drop(unified_tx);

        loop {
            tokio::select! {
                Some(encoded) = unified_rx.recv() => {
                    let data_size: usize = encoded.regions.iter().map(|r| r.data.len()).sum();

                    // 带宽节流：检查当前秒是否超限
                    if self.second_start.elapsed() >= Duration::from_secs(1) {
                        self.bytes_this_second = 0;
                        self.second_start = Instant::now();
                    }
                    if self.bandwidth_limit > 0 && self.bytes_this_second + data_size as u64 > self.bandwidth_limit {
                        // 超限跳帧
                        debug!("带宽超限，跳帧 (本秒已发 {} bytes, 限制 {})", self.bytes_this_second, self.bandwidth_limit);
                        continue;
                    }

                    self.bytes_sent += data_size as u64;
                    self.bytes_this_second += data_size as u64;
                    self.network_metrics.bytes_sent_window += data_size as u64;

                    let (cx, cy) = self.injector.cursor_position();
                    let cs = self.injector.cursor_shape();
                    let msg = Message::FrameData {
                        seq: encoded.seq,
                        timestamp_ms: encoded.timestamp_ms,
                        regions: Arc::unwrap_or_clone(encoded.regions),
                        cursor_x: cx,
                        cursor_y: cy,
                        cursor_shape: cs,
                    };
                    if let Err(e) = self.framed.send(msg).await {
                        warn!("发送帧数据失败: {}", e);
                        break;
                    }
                }

                // 心跳
                _ = heartbeat_interval.tick() => {
                    if self.last_recv_time.elapsed() > HEARTBEAT_TIMEOUT {
                        warn!("控制端 {} 心跳超时", self.addr);
                        break;
                    }

                    // 会话空闲超时检查
                    if !self.idle_timeout.is_zero() && self.last_input_time.elapsed() > self.idle_timeout {
                        warn!(
                            target: "audit",
                            op = "disconnect",
                            addr = %self.addr,
                            role = ?self.granted_role,
                            reason = "idle_timeout",
                            "会话空闲超时，自动断开"
                        );
                        let _ = self.framed.send(Message::Disconnect).await;
                        break;
                    }

                    // Shell 空闲超时检查
                    if let Some(last_activity) = self.pty_last_activity {
                        if last_activity.elapsed() > SHELL_IDLE_TIMEOUT {
                            warn!(
                                target: "audit",
                                op = "close",
                                addr = %self.addr,
                                role = ?self.granted_role,
                                reason = "idle_timeout",
                                "Shell 空闲超时，自动关闭 PTY"
                            );
                            self.close_pty();
                            self.pty_last_activity = None;
                            let _ = self.framed.send(Message::ShellClose).await;
                        }
                    }

                    // 自适应比特率评估
                    if let Some((new_quality, new_fps)) = self.network_metrics.evaluate(self.bandwidth_limit) {
                        info!("自适应比特率调整: quality={}, fps={}", new_quality, new_fps);
                        if let Some(ref tx) = self.capture_cmd_tx {
                            let _ = tx.send(CaptureCommand::UpdateSettings {
                                jpeg_quality: new_quality as u8,
                                max_fps: new_fps,
                            }).await;
                        }
                    }

                    let ping_ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if let Err(e) = self.framed.send(Message::Ping { timestamp_ms: ping_ts }).await {
                        warn!("发送心跳失败: {}", e);
                        break;
                    }
                }

                // 接收控制端消息
                msg = self.framed.next() => {
                    match msg {
                        Some(Ok(message)) => {
                            self.last_recv_time = Instant::now();
                            if !self.handle_message(message, &clipboard_remote_tx).await? {
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            warn!("接收消息错误: {}", e);
                            break;
                        }
                        None => {
                            info!("控制端 {} 断开连接", self.addr);
                            break;
                        }
                    }
                }

                // 剪贴板
                Some(change) = clipboard_local_rx.recv() => {
                    let msg = Message::ClipboardUpdate {
                        content_type: change.content_type,
                        data: change.data,
                    };
                    if let Err(e) = self.framed.send(msg).await {
                        warn!("发送剪贴板更新失败: {}", e);
                    }
                }

                // 系统信息
                _ = sysinfo_interval.tick() => {
                    sys.refresh_cpu_usage();
                    sys.refresh_memory();
                    let cpu = sys.global_cpu_usage();
                    let mem_used = sys.used_memory() as f32;
                    let mem_total = sys.total_memory();
                    let mem_pct = if mem_total > 0 { mem_used / mem_total as f32 * 100.0 } else { 0.0 };
                    if let Err(e) = self.framed.send(Message::SystemInfo {
                        cpu_usage: cpu,
                        memory_usage: mem_pct,
                        memory_total_mb: mem_total / 1024 / 1024,
                    }).await {
                        warn!("发送系统信息失败: {}", e);
                    }
                }

                // 音频数据
                Some((audio_data, enc)) = audio_tokio_rx.recv() => {
                    let msg = Message::AudioData { data: audio_data, encoding: enc };
                    if let Err(e) = self.framed.send(msg).await {
                        warn!("发送音频数据失败: {}", e);
                    }
                }

                // PTY 输出
                Some(pty_data) = async {
                    if let Some(ref mut rx) = self.pty_output_rx { rx.recv().await }
                    else { std::future::pending().await }
                } => {
                    self.pty_last_activity = Some(Instant::now());
                    info!(
                        target: "audit",
                        op = "output",
                        addr = %self.addr,
                        role = ?self.granted_role,
                        data_len = pty_data.len(),
                        "Shell 输出"
                    );
                    let msg = Message::ShellData { data: pty_data };
                    if let Err(e) = self.framed.send(msg).await {
                        warn!("发送 PTY 数据失败: {}", e);
                    }
                }

                // 被控端主机发来的聊天消息 → 转发给远程控制端
                result = async {
                    if let Some(ref mut rx) = self.host_chat_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    if let Ok((text, sender, timestamp_ms)) = result {
                        let msg = Message::ChatMessage { text, sender, timestamp_ms };
                        if let Err(e) = self.framed.send(msg).await {
                            warn!("发送被控端聊天消息失败: {}", e);
                        }
                    }
                }
            }
        }

        // 会话结束时恢复屏幕显示
        if let Some(mut blanker) = self.screen_blanker.take() {
            let _ = blanker.set_blank(false);
        }

        // 会话结束时清理 PTY
        self.close_pty();

        // 断开时自动锁屏（仅针对控制者角色）
        if self.lock_on_disconnect && self.granted_role == SessionRole::Controller {
            info!("控制者断开，执行自动锁屏");
            if let Err(e) = platform::lock_screen() {
                warn!("断开时自动锁屏失败: {}", e);
            }
        }

        info!(
            "会话 {} 结束, 共发送 {:.1} MB",
            self.addr,
            self.bytes_sent as f64 / 1024.0 / 1024.0
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_metrics_new_defaults() {
        let m = NetworkMetrics::new(75, 30);
        assert_eq!(m.current_quality, 75);
        assert_eq!(m.current_fps, 30);
        assert!(m.rtt_samples.is_empty());
        assert_eq!(m.bytes_sent_window, 0);
    }

    #[test]
    fn test_network_metrics_record_rtt() {
        let mut m = NetworkMetrics::new(75, 30);
        m.record_rtt(50);
        m.record_rtt(100);
        assert_eq!(m.rtt_samples.len(), 2);

        // 超过 60 个样本时淘汰旧样本
        for i in 0..60 {
            m.record_rtt(i);
        }
        // 之前已有 2 个，再加 60 个 = 62，但上限是 60，所以应弹出 2 个
        assert_eq!(m.rtt_samples.len(), 60);
    }

    #[test]
    fn test_network_metrics_avg_rtt() {
        let mut m = NetworkMetrics::new(75, 30);
        // 空样本返回 0
        assert_eq!(m.avg_rtt(), 0);

        m.record_rtt(100);
        m.record_rtt(200);
        m.record_rtt(300);
        // 平均值: (100+200+300)/3 = 200
        assert_eq!(m.avg_rtt(), 200);
    }

    #[test]
    fn test_network_metrics_evaluate_downgrade() {
        let mut m = NetworkMetrics::new(75, 30);
        // 填充高 RTT 样本（> 200ms）
        for _ in 0..10 {
            m.record_rtt(300);
        }
        // 强制 evaluate 立即执行：将 last_eval_time 设为足够久之前
        m.last_eval_time = Instant::now() - Duration::from_secs(60);

        let result = m.evaluate(0);
        assert!(result.is_some());
        let (q, f) = result.unwrap();
        // 质量应降低：75 - 10 = 65
        assert_eq!(q, 65);
        // 帧率应降低：30 - 10 = 20
        assert_eq!(f, 20);
        assert_eq!(m.current_quality, 65);
        assert_eq!(m.current_fps, 20);
    }

    #[test]
    fn test_network_metrics_evaluate_upgrade() {
        let mut m = NetworkMetrics::new(60, 15);
        // 填充低 RTT 样本（< 50ms）
        for _ in 0..10 {
            m.record_rtt(20);
        }
        m.last_eval_time = Instant::now() - Duration::from_secs(60);
        // 设带宽限制足够高，利用率 < 50%
        m.bytes_sent_window = 100;

        let result = m.evaluate(1_000_000);
        assert!(result.is_some());
        let (q, f) = result.unwrap();
        // 质量应提升：60 + 5 = 65
        assert_eq!(q, 65);
        // 帧率应提升：15 + 5 = 20
        assert_eq!(f, 20);
        assert_eq!(m.current_quality, 65);
        assert_eq!(m.current_fps, 20);
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Session<S> {
    /// 处理控制端消息
    async fn handle_message(
        &mut self,
        msg: Message,
        clipboard_tx: &tokio::sync::mpsc::Sender<ClipboardChange>,
    ) -> anyhow::Result<bool> {
        // 先尝试文件传输处理
        if self.handle_file_message(&msg).await? {
            return Ok(true);
        }

        match msg {
            Message::MouseMove { x, y } => {
                if self.granted_role == SessionRole::Controller {
                    self.last_input_time = Instant::now();
                    self.injector.move_mouse(x, y)?;
                }
            }
            Message::MouseButton { button, pressed } => {
                if self.granted_role == SessionRole::Controller {
                    self.last_input_time = Instant::now();
                    self.injector.mouse_button(button, pressed)?;
                }
            }
            Message::MouseScroll { dx, dy } => {
                if self.granted_role == SessionRole::Controller {
                    self.last_input_time = Instant::now();
                    self.injector.mouse_scroll(dx, dy)?;
                }
            }
            Message::KeyEvent {
                code,
                pressed,
                modifiers,
            } => {
                if self.granted_role == SessionRole::Controller {
                    self.last_input_time = Instant::now();
                    self.injector.key_event(&code, pressed, modifiers)?;
                }
            }
            Message::ClipboardUpdate { content_type, data } => {
                if self.granted_role == SessionRole::Controller {
                    let _ = clipboard_tx
                        .send(ClipboardChange { content_type, data })
                        .await;
                }
            }
            Message::SwitchMonitor { index } => {
                info!("控制端请求切换到显示器 {}", index);
                if let Some(ref tx) = self.capture_cmd_tx {
                    let _ = tx.send(CaptureCommand::SwitchMonitor(index)).await;
                }
                // 更新输入注入器的活跃显示器边界
                if let Ok(monitors) = lan_desk_capture::list_monitors() {
                    if let Some(m) = monitors.iter().find(|m| m.index == index) {
                        self.injector
                            .set_active_monitor(m.left, m.top, m.width, m.height);
                        info!(
                            "输入坐标映射切换到显示器 {} ({}x{} at {},{})",
                            index, m.width, m.height, m.left, m.top
                        );
                    }
                }
            }
            Message::SetBandwidthLimit { bytes_per_sec } => {
                self.bandwidth_limit = bytes_per_sec;
                info!(
                    "带宽限制设为 {} bytes/sec ({})",
                    bytes_per_sec,
                    if bytes_per_sec == 0 {
                        "不限制".to_string()
                    } else {
                        format!("{:.1} Mbps", bytes_per_sec as f64 * 8.0 / 1_000_000.0)
                    }
                );
            }
            Message::CaptureSettings {
                jpeg_quality,
                max_fps,
            } => {
                info!("收到捕获设置更新: 画质={}, 帧率={}", jpeg_quality, max_fps);
                if let Some(ref tx) = self.capture_cmd_tx {
                    let _ = tx
                        .send(CaptureCommand::UpdateSettings {
                            jpeg_quality,
                            max_fps,
                        })
                        .await;
                }
            }
            // === 远程终端 PTY ===
            Message::ShellStart { cols, rows } => {
                if !self.shell_enabled {
                    if let Err(e) = self
                        .framed
                        .send(Message::ShellStartAck {
                            success: false,
                            error: "远程终端已被服务端禁用".to_string(),
                        })
                        .await
                    {
                        warn!("发送 ShellStartAck 失败: {}", e);
                    }
                } else if self.granted_role != SessionRole::Controller {
                    if let Err(e) = self
                        .framed
                        .send(Message::ShellStartAck {
                            success: false,
                            error: "仅查看模式无终端权限".to_string(),
                        })
                        .await
                    {
                        warn!("发送 ShellStartAck 失败: {}", e);
                    }
                } else if self.pty_child.is_some() {
                    if let Err(e) = self
                        .framed
                        .send(Message::ShellStartAck {
                            success: false,
                            error: "终端已在运行".to_string(),
                        })
                        .await
                    {
                        warn!("发送 ShellStartAck 失败: {}", e);
                    }
                } else {
                    match self.start_pty(cols, rows) {
                        Ok(()) => {
                            self.pty_last_activity = Some(Instant::now());
                            info!(
                                target: "audit",
                                op = "start",
                                addr = %self.addr,
                                role = ?self.granted_role,
                                cols = cols,
                                rows = rows,
                                "Shell 启动"
                            );
                            if let Err(e) = self
                                .framed
                                .send(Message::ShellStartAck {
                                    success: true,
                                    error: String::new(),
                                })
                                .await
                            {
                                warn!("发送 ShellStartAck 失败: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("PTY 启动失败: {}", e);
                            if let Err(e2) = self
                                .framed
                                .send(Message::ShellStartAck {
                                    success: false,
                                    error: format!("启动终端失败: {}", e),
                                })
                                .await
                            {
                                warn!("发送 ShellStartAck 失败: {}", e2);
                            }
                        }
                    }
                }
            }
            Message::ShellData { data } => {
                self.last_input_time = Instant::now();
                self.pty_last_activity = Some(Instant::now());
                info!(
                    target: "audit",
                    op = "input",
                    addr = %self.addr,
                    role = ?self.granted_role,
                    data_len = data.len(),
                    "Shell 输入"
                );
                // PTY 写入通过 spawn_blocking 避免阻塞 tokio 运行时
                if let Some(writer) = self.pty_writer.take() {
                    let result = tokio::task::spawn_blocking(move || {
                        use std::io::Write;
                        let mut w = writer;
                        if let Err(e) = w.write_all(&data) {
                            tracing::warn!("PTY 写入失败: {}", e);
                        }
                        if let Err(e) = w.flush() {
                            tracing::warn!("PTY flush 失败: {}", e);
                        }
                        w
                    })
                    .await;
                    if let Ok(w) = result {
                        self.pty_writer = Some(w);
                    }
                }
            }
            Message::ShellResize { cols, rows } => {
                if let Some(ref master) = self.pty_master {
                    let size = portable_pty::PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    };
                    if let Err(e) = master.resize(size) {
                        warn!("PTY resize 失败: {}", e);
                    }
                    debug!("PTY resize: {}x{}", cols, rows);
                }
            }
            Message::ShellClose => {
                self.close_pty();
                self.pty_last_activity = None;
                info!(
                    target: "audit",
                    op = "close",
                    addr = %self.addr,
                    role = ?self.granted_role,
                    reason = "client_request",
                    "Shell 关闭"
                );
            }
            Message::Annotation { .. } | Message::AnnotationClear => {
                // 标注在控制端本地渲染，被控端忽略
                debug!("收到标注消息");
            }
            Message::Ping { timestamp_ms } => {
                if let Err(e) = self.framed.send(Message::Pong { timestamp_ms }).await {
                    warn!("发送 Pong 失败: {}", e);
                }
            }
            Message::Pong { timestamp_ms } => {
                // 对方回应了我们的 Ping，记录 RTT
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let rtt = now.saturating_sub(timestamp_ms);
                self.network_metrics.record_rtt(rtt);
            }
            Message::ChatMessage {
                ref text,
                ref sender,
                timestamp_ms,
            } => {
                info!(
                    target: "audit",
                    op = "chat",
                    addr = %self.addr,
                    role = ?self.granted_role,
                    sender = %sender,
                    text_len = text.len(),
                    "收到聊天消息"
                );
                // 转发聊天消息到本地前端
                if let Some(ref tx) = self.chat_tx {
                    let _ = tx.try_send((text.clone(), sender.clone(), timestamp_ms));
                }
            }
            Message::SpecialKey { key } => {
                if self.granted_role == SessionRole::Controller {
                    self.last_input_time = Instant::now();
                    info!(
                        target: "audit",
                        op = "special_key",
                        addr = %self.addr,
                        role = ?self.granted_role,
                        key = ?key,
                        "特殊按键请求"
                    );
                    if let Err(e) = self.injector.send_special_key(key) {
                        warn!("发送特殊按键失败: {}", e);
                    }
                }
            }
            Message::ScreenBlank { enable } => {
                if self.granted_role == SessionRole::Controller {
                    info!(
                        target: "audit",
                        op = "screen_blank",
                        addr = %self.addr,
                        role = ?self.granted_role,
                        enable = enable,
                        "屏幕遮蔽请求"
                    );
                    if enable {
                        let mut blanker = screen_blank::ScreenBlanker::new();
                        if let Err(e) = blanker.set_blank(true) {
                            warn!("屏幕遮蔽失败: {}", e);
                        } else {
                            self.screen_blanker = Some(blanker);
                        }
                    } else if let Some(mut blanker) = self.screen_blanker.take() {
                        if let Err(e) = blanker.set_blank(false) {
                            warn!("取消屏幕遮蔽失败: {}", e);
                        }
                    }
                }
            }
            Message::RemoteReboot => {
                if self.granted_role == SessionRole::Controller {
                    info!(
                        target: "audit",
                        op = "remote_reboot",
                        addr = %self.addr,
                        role = ?self.granted_role,
                        "远程重启请求"
                    );
                    // 先发送重启通知
                    let _ = self
                        .framed
                        .send(Message::RebootPending {
                            estimated_seconds: 30,
                        })
                        .await;
                    // 执行重启
                    if let Err(e) = reboot::reboot_system() {
                        warn!("远程重启失败: {}", e);
                    }
                }
            }
            Message::LockScreen => {
                if self.granted_role == SessionRole::Controller {
                    info!(
                        target: "audit",
                        op = "lock_screen",
                        addr = %self.addr,
                        role = ?self.granted_role,
                        "远程锁屏请求"
                    );
                    if let Err(e) = platform::lock_screen() {
                        warn!("远程锁屏失败: {}", e);
                    }
                }
            }
            Message::Disconnect => {
                info!("控制端请求断开");
                return Ok(false);
            }
            _ => {
                debug!("忽略未处理的消息类型");
            }
        }
        Ok(true)
    }
}
