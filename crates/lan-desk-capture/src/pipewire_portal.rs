//! PipeWire/Portal 屏幕捕获实现
//!
//! 通过 XDG Desktop Portal（ashpd）获取用户授权和 PipeWire fd，
//! 再通过 pipewire-rs 消费视频帧流。这是现代 Wayland 桌面的标准
//! 屏幕捕获方式，无需外部工具，支持 GNOME/KDE/Sway/Hyprland 等。
//!
//! 线程模型:
//! - Portal 协商在临时线程 + 临时 tokio runtime 中完成（async ashpd）
//! - PipeWire 帧接收在 `pipewire-capture` 后台线程中运行（MainLoop 事件循环）
//! - capture_frame() 通过 Arc<Mutex> 读取后台线程写入的最新帧

use std::os::fd::{FromRawFd, IntoRawFd};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use tracing::{debug, info, warn};

use crate::frame::{CapturedFrame, PixelFormat};
use crate::ScreenCapture;

// ─── Portal 授权令牌持久化 ──────────────────────────────────────────

/// Portal restore_token 管理，避免每次启动都弹出授权对话框
struct RestoreToken {
    token: Option<String>,
    path: std::path::PathBuf,
}

impl RestoreToken {
    fn token_path() -> std::path::PathBuf {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
                std::path::PathBuf::from(home).join(".local/share")
            });
        data_dir.join("lan-desk").join("portal_restore_token")
    }

    fn load() -> Self {
        let path = Self::token_path();
        let token = std::fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if token.is_some() {
            debug!("已加载 Portal restore_token");
        }
        Self { token, path }
    }

    fn save(&mut self, token: &str) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&self.path, token) {
            warn!("保存 Portal restore_token 失败: {}", e);
        } else {
            debug!("Portal restore_token 已保存");
        }
        self.token = Some(token.to_string());
    }
}

// ─── Portal 协商结果 ────────────────────────────────────────────────

struct PortalResult {
    pipewire_fd: i32,
    node_id: u32,
    width: u32,
    height: u32,
    restore_token: Option<String>,
}

/// 通过 XDG Desktop Portal 协商屏幕捕获会话（async）
async fn negotiate_portal_session(saved_token: Option<String>) -> anyhow::Result<PortalResult> {
    use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
    use ashpd::desktop::PersistMode;

    let proxy = Screencast::new()
        .await
        .context("创建 ScreenCast Portal 代理失败")?;

    let session = proxy
        .create_session()
        .await
        .context("创建 Portal 会话失败")?;

    proxy
        .select_sources(
            &session,
            CursorMode::Embedded,
            SourceType::Monitor.into(),
            false,
            saved_token.as_deref(),
            PersistMode::Application,
        )
        .await
        .context("Portal SelectSources 失败")?;

    // Start 会在首次时弹出用户授权对话框
    let response = proxy
        .start(&session, None)
        .await
        .context("Portal Start 失败（用户可能拒绝了授权）")?
        .response()
        .context("Portal Start response 解析失败")?;

    let stream = response
        .streams()
        .first()
        .ok_or_else(|| anyhow::anyhow!("Portal 未返回任何视频流"))?;

    let node_id = stream.pipe_wire_node_id();
    let (width, height) = stream
        .size()
        .map(|(w, h)| (w as u32, h as u32))
        .unwrap_or((1920, 1080));

    info!(
        "Portal 授权成功: PipeWire node_id={}, 分辨率={}x{}",
        node_id, width, height
    );

    // 获取 PipeWire fd
    let fd = proxy
        .open_pipe_wire_remote(&session)
        .await
        .context("获取 PipeWire fd 失败")?;

    let raw_fd = fd.into_raw_fd();

    let new_restore_token = response.restore_token().map(|s: &str| s.to_string());

    Ok(PortalResult {
        pipewire_fd: raw_fd,
        node_id,
        width,
        height,
        restore_token: new_restore_token,
    })
}

// ─── 共享状态 ───────────────────────────────────────────────────────

struct SharedState {
    /// 最新帧（PipeWire 线程覆盖写入，capture_frame 读取克隆）
    latest_frame: Mutex<Option<CapturedFrame>>,
    /// 当前屏幕宽度（原子，可被 PipeWire 线程更新）
    width: AtomicU32,
    /// 当前屏幕高度
    height: AtomicU32,
    /// PipeWire 后台线程是否在运行
    running: AtomicBool,
    /// 后台线程错误信息
    error: Mutex<Option<String>>,
    /// 协商后的像素格式（存储为 u32 便于原子操作）
    /// 0=BGRx, 1=RGBx, 2=BGRA, 3=RGBA, 4=xRGB, 5=ARGB, 255=未知
    pixel_format: AtomicU32,
}

// ─── 像素格式转换 ──────────────────────────────────────────────────

/// SPA 视频格式标识（简化版，仅覆盖常见格式）
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u32)]
enum SpaFormat {
    BGRx = 0,
    RGBx = 1,
    BGRA = 2,
    RGBA = 3,
    XRGB = 4,
    ARGB = 5,
    Unknown = 255,
}

impl From<u32> for SpaFormat {
    fn from(v: u32) -> Self {
        match v {
            0 => Self::BGRx,
            1 => Self::RGBx,
            2 => Self::BGRA,
            3 => Self::RGBA,
            4 => Self::XRGB,
            5 => Self::ARGB,
            _ => Self::Unknown,
        }
    }
}

/// 将 PipeWire 帧数据转换为 BGRA8 格式
fn convert_to_bgra(
    raw: &[u8],
    width: u32,
    height: u32,
    src_stride: u32,
    format: SpaFormat,
) -> Vec<u8> {
    let dst_stride = (width * 4) as usize;
    let mut bgra = vec![0u8; dst_stride * height as usize];

    for y in 0..height as usize {
        let src_start = y * src_stride as usize;
        let src_end = src_start + dst_stride.min(raw.len() - src_start);
        if src_start >= raw.len() {
            break;
        }
        let src_row = &raw[src_start..src_end];
        let dst_row = &mut bgra[y * dst_stride..(y + 1) * dst_stride];

        match format {
            SpaFormat::BGRx | SpaFormat::BGRA => {
                // B G R x/A → B G R 255（直接拷贝 + 填充 alpha）
                let copy_len = dst_stride.min(src_row.len());
                dst_row[..copy_len].copy_from_slice(&src_row[..copy_len]);
                for pixel in dst_row.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
            }
            SpaFormat::RGBx | SpaFormat::RGBA => {
                // R G B x/A → B G R 255（交换 R/B）
                for (src_px, dst_px) in src_row
                    .chunks_exact(4)
                    .zip(dst_row.chunks_exact_mut(4))
                    .take(width as usize)
                {
                    dst_px[0] = src_px[2]; // B
                    dst_px[1] = src_px[1]; // G
                    dst_px[2] = src_px[0]; // R
                    dst_px[3] = 255;
                }
            }
            SpaFormat::XRGB | SpaFormat::ARGB => {
                // x/A R G B → B G R 255（通道重排）
                for (src_px, dst_px) in src_row
                    .chunks_exact(4)
                    .zip(dst_row.chunks_exact_mut(4))
                    .take(width as usize)
                {
                    dst_px[0] = src_px[3]; // B
                    dst_px[1] = src_px[2]; // G
                    dst_px[2] = src_px[1]; // R
                    dst_px[3] = 255;
                }
            }
            SpaFormat::Unknown => {
                // 未知格式，当作 BGRx 处理
                let copy_len = dst_stride.min(src_row.len());
                dst_row[..copy_len].copy_from_slice(&src_row[..copy_len]);
                for pixel in dst_row.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
            }
        }
    }
    bgra
}

// ─── PipeWire 后台线程 ─────────────────────────────────────────────

/// PipeWire 后台线程主函数
///
/// 通过 Portal 返回的 fd 和 node_id 连接 PipeWire daemon，
/// 创建 Stream 接收视频帧，将每帧转换为 BGRA 写入共享状态。
fn pipewire_thread_main(
    shared: Arc<SharedState>,
    pipewire_fd: i32,
    node_id: u32,
    shutdown_rx: std::sync::mpsc::Receiver<()>,
) {
    use pipewire as pw;

    pw::init();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pipewire_event_loop(shared.clone(), pipewire_fd, node_id, shutdown_rx)
    }));

    shared.running.store(false, Ordering::Release);

    let msg = match result {
        Ok(Ok(())) => return, // 正常退出
        Ok(Err(e)) => format!("PipeWire 初始化失败: {}", e),
        Err(e) => {
            if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "PipeWire 线程 panic".to_string()
            }
        }
    };
    warn!("PipeWire 后台线程异常: {}", msg);
    if let Ok(mut err) = shared.error.lock() {
        *err = Some(msg);
    }
}

fn pipewire_event_loop(
    shared: Arc<SharedState>,
    pipewire_fd: i32,
    node_id: u32,
    shutdown_rx: std::sync::mpsc::Receiver<()>,
) -> anyhow::Result<()> {
    use pipewire as pw;

    let mainloop = pw::main_loop::MainLoop::new(None)
        .map_err(|e| anyhow::anyhow!("无法创建 PipeWire MainLoop: {}", e))?;
    let context = pw::context::Context::new(&mainloop)
        .map_err(|e| anyhow::anyhow!("无法创建 PipeWire Context: {}", e))?;

    // 通过 Portal fd 连接 PipeWire daemon
    let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(pipewire_fd) };
    let core = context
        .connect_fd(fd, None)
        .map_err(|e| anyhow::anyhow!("无法通过 Portal fd 连接 PipeWire: {}", e))?;

    // 创建视频流
    let stream = pw::stream::Stream::new(
        &core,
        "lan-desk-capture",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| anyhow::anyhow!("无法创建 PipeWire Stream: {}", e))?;

    // 构建 Stream 参数：请求 BGRx 格式，让 PipeWire 协商最佳格式
    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaType,
            Id,
            pw::spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            pw::spa::param::video::VideoFormat::BGRx,
            pw::spa::param::video::VideoFormat::RGBx,
            pw::spa::param::video::VideoFormat::BGRA,
            pw::spa::param::video::VideoFormat::RGBA,
            pw::spa::param::video::VideoFormat::xRGB,
            pw::spa::param::video::VideoFormat::ARGB
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle {
                width: shared.width.load(Ordering::Relaxed),
                height: shared.height.load(Ordering::Relaxed),
            },
            pw::spa::utils::Rectangle {
                width: 1,
                height: 1,
            },
            pw::spa::utils::Rectangle {
                width: 7680,
                height: 4320,
            }
        ),
    );

    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [pw::spa::pod::Pod::from_bytes(&values).unwrap()];

    // 注册回调
    let shared_process = shared.clone();
    let shared_param = shared.clone();

    let _listener =
        stream
            .add_local_listener_with_user_data(())
            .param_changed(move |_, id, _user_data, param| {
                // 格式协商完成时更新分辨率和像素格式
                if id != pw::spa::param::ParamType::Format.as_raw() {
                    return;
                }
                if let Some(param) = param {
                    if let Ok((_, obj)) =
                        pw::spa::pod::deserialize::PodDeserializer::deserialize_from::<
                            pw::spa::pod::Value,
                        >(param.as_bytes())
                    {
                        parse_format_from_pod(&obj, &shared_param);
                    }
                }
            })
            .process(move |stream, _user_data| {
                process_frame(stream, &shared_process);
            })
            .register()
            .map_err(|e| anyhow::anyhow!("注册 PipeWire Stream 回调失败: {}", e))?;

    // 连接到目标 node
    stream
        .connect(
            pw::spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|e| anyhow::anyhow!("PipeWire Stream 连接失败: {}", e))?;

    shared.running.store(true, Ordering::Release);
    info!("PipeWire 捕获线程启动: node_id={}", node_id);

    // 运行事件循环，定期检查停止信号
    loop {
        // 处理 PipeWire 事件（非阻塞，最多等 50ms）
        mainloop.iterate(Duration::from_millis(50));

        // 检查停止信号
        if shutdown_rx.try_recv().is_ok() {
            info!("PipeWire 捕获线程收到停止信号");
            break;
        }
    }
    Ok(())
}

/// 从 PipeWire SPA Pod 中解析视频格式参数
fn parse_format_from_pod(value: &pipewire::spa::pod::Value, shared: &Arc<SharedState>) {
    if let pipewire::spa::pod::Value::Object(obj) = value {
        for prop in &obj.properties {
            match prop.key {
                k if k == pipewire::spa::param::format::FormatProperties::VideoFormat.as_raw() => {
                    if let pipewire::spa::pod::Value::Id(id) = &prop.value {
                        let fmt = spa_video_format_to_local(id.0);
                        shared.pixel_format.store(fmt as u32, Ordering::Release);
                        debug!("PipeWire 协商像素格式: {:?}", fmt);
                    }
                }
                k if k == pipewire::spa::param::format::FormatProperties::VideoSize.as_raw() => {
                    if let pipewire::spa::pod::Value::Rectangle(pipewire::spa::utils::Rectangle {
                        width,
                        height,
                    }) = &prop.value
                    {
                        shared.width.store(*width, Ordering::Release);
                        shared.height.store(*height, Ordering::Release);
                        info!("PipeWire 协商分辨率: {}x{}", width, height);
                    }
                }
                _ => {}
            }
        }
    }
}

/// 将 SPA VideoFormat ID 映射到本地 SpaFormat 枚举
fn spa_video_format_to_local(id: u32) -> SpaFormat {
    use pipewire::spa::param::video::VideoFormat;
    match VideoFormat::from_raw(id) {
        VideoFormat::BGRx => SpaFormat::BGRx,
        VideoFormat::RGBx => SpaFormat::RGBx,
        VideoFormat::BGRA => SpaFormat::BGRA,
        VideoFormat::RGBA => SpaFormat::RGBA,
        VideoFormat::xRGB => SpaFormat::XRGB,
        VideoFormat::ARGB => SpaFormat::ARGB,
        _ => SpaFormat::Unknown,
    }
}

/// PipeWire Stream process 回调：从 SPA buffer 读取帧数据
fn process_frame(stream: &pipewire::stream::Stream, shared: &Arc<SharedState>) {
    let mut buffer = match stream.dequeue_buffer() {
        Some(buf) => buf,
        None => return,
    };

    let datas = buffer.datas_mut();
    if datas.is_empty() {
        return;
    }

    let data = &mut datas[0];

    // 先从 chunk 读取 size 和 stride（不可变借用），再做可变借用获取像素数据
    let (size, stride) = {
        let chunk = data.chunk();
        (chunk.size() as usize, chunk.stride() as u32)
    };

    if size == 0 {
        return;
    }

    // 获取像素数据指针（MemFd/MemPtr 映射）— 需要可变借用
    let slice = data.data();
    if slice.is_none() {
        // DMA-BUF 传输，当前版本不支持（需要 EGL 导入）
        debug!("PipeWire 帧为 DMA-BUF 格式，跳过（不支持）");
        return;
    }

    let raw_data = match slice {
        Some(s) => &s[..size.min(s.len())],
        None => return,
    };

    let width = shared.width.load(Ordering::Acquire);
    let height = shared.height.load(Ordering::Acquire);
    let format = SpaFormat::from(shared.pixel_format.load(Ordering::Acquire));

    // 使用实际 stride（如果有效），否则根据宽度计算
    let effective_stride = if stride > 0 { stride } else { width * 4 };

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let bgra_data = convert_to_bgra(raw_data, width, height, effective_stride, format);

    let frame = CapturedFrame {
        width,
        height,
        stride: width * 4,
        pixel_format: PixelFormat::Bgra8,
        data: bgra_data,
        timestamp_ms,
    };

    // 覆盖写入最新帧（try_lock 避免阻塞 PipeWire 线程）
    if let Ok(mut guard) = shared.latest_frame.try_lock() {
        *guard = Some(frame);
    }
}

// ─── PipeWireCapture 公开接口 ──────────────────────────────────────

/// 基于 PipeWire/Portal 的 Wayland 屏幕捕获器
///
/// 使用 XDG Desktop Portal 获取用户授权，通过 PipeWire 流接收帧数据。
/// 首次启动时会弹出系统授权对话框，后续通过 restore_token 跳过。
pub struct PipeWireCapture {
    shared: Arc<SharedState>,
    /// 停止信号发送端
    shutdown_tx: std::sync::mpsc::Sender<()>,
    /// 后台线程句柄
    _thread: Option<std::thread::JoinHandle<()>>,
    /// Portal 返回的 PipeWire fd，需要在 Drop 时关闭
    pipewire_fd: Option<i32>,
}

impl PipeWireCapture {
    /// 创建 PipeWire 捕获器
    ///
    /// 在内部通过 XDG Desktop Portal 协商会话，获取 PipeWire fd，
    /// 然后启动后台线程接收帧数据。首次调用可能弹出授权对话框。
    pub fn new(_display_index: usize) -> anyhow::Result<Self> {
        // 加载保存的 restore_token
        let mut token_mgr = RestoreToken::load();
        let saved_token = token_mgr.token.clone();

        // 在临时线程中执行 async Portal 协商
        // （避免与调用方已有的 tokio runtime 冲突）
        let portal_result = std::thread::scope(|_| {
            std::thread::Builder::new()
                .name("portal-negotiate".into())
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .context("创建 Portal 协商 runtime 失败")?;
                    rt.block_on(negotiate_portal_session(saved_token))
                })
                .context("启动 Portal 协商线程失败")?
                .join()
                .map_err(|_| anyhow::anyhow!("Portal 协商线程 panic"))?
        })?;

        // 保存 restore_token
        if let Some(ref new_token) = portal_result.restore_token {
            token_mgr.save(new_token);
        }

        // 构建共享状态
        let shared = Arc::new(SharedState {
            latest_frame: Mutex::new(None),
            width: AtomicU32::new(portal_result.width),
            height: AtomicU32::new(portal_result.height),
            running: AtomicBool::new(false),
            error: Mutex::new(None),
            pixel_format: AtomicU32::new(SpaFormat::BGRx as u32),
        });

        // 启动 PipeWire 后台线程
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
        let shared_clone = shared.clone();
        let pw_fd = portal_result.pipewire_fd;
        let pw_node = portal_result.node_id;

        let thread = std::thread::Builder::new()
            .name("pipewire-capture".into())
            .spawn(move || {
                pipewire_thread_main(shared_clone, pw_fd, pw_node, shutdown_rx);
            })
            .context("启动 PipeWire 捕获线程失败")?;

        // 等待后台线程开始运行（最多 5 秒）
        let start = std::time::Instant::now();
        while !shared.running.load(Ordering::Acquire) {
            if start.elapsed() > Duration::from_secs(5) {
                warn!("PipeWire 捕获线程启动超时");
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        info!(
            "PipeWire 捕获器已初始化: {}x{}, node_id={}",
            portal_result.width, portal_result.height, portal_result.node_id
        );

        Ok(Self {
            shared,
            shutdown_tx,
            _thread: Some(thread),
            pipewire_fd: Some(pw_fd),
        })
    }

    /// 检测 PipeWire 是否在系统上可用（运行时检查）
    pub fn is_available() -> bool {
        // 检查 libpipewire 是否可加载
        std::panic::catch_unwind(|| {
            pipewire::init();
        })
        .is_ok()
    }
}

impl ScreenCapture for PipeWireCapture {
    fn capture_frame(&mut self) -> anyhow::Result<CapturedFrame> {
        // 检查后台线程是否存活
        if !self.shared.running.load(Ordering::Acquire) {
            let err = self
                .shared
                .error
                .lock()
                .ok()
                .and_then(|mut e| e.take())
                .unwrap_or_else(|| "PipeWire 后台线程已退出".into());
            anyhow::bail!("PipeWire 捕获失败: {}", err);
        }

        // 读取最新帧
        let frame = self
            .shared
            .latest_frame
            .lock()
            .map_err(|_| anyhow::anyhow!("PipeWire 共享状态锁中毒"))?
            .clone();

        frame.ok_or_else(|| anyhow::anyhow!("PipeWire 流尚未产生帧数据"))
    }

    fn screen_size(&self) -> (u32, u32) {
        (
            self.shared.width.load(Ordering::Relaxed),
            self.shared.height.load(Ordering::Relaxed),
        )
    }
}

impl Drop for PipeWireCapture {
    fn drop(&mut self) {
        // 发送停止信号
        let _ = self.shutdown_tx.send(());
        // 等待线程结束（最多 2 秒，使用辅助线程实现超时）
        if let Some(thread) = self._thread.take() {
            let (tx, rx) = std::sync::mpsc::channel();
            let join_thread = std::thread::spawn(move || {
                let _ = thread.join();
                let _ = tx.send(());
            });
            if rx.recv_timeout(Duration::from_secs(2)).is_err() {
                warn!("PipeWire 线程关闭超时");
            }
            // 无论超时与否都不要阻塞，辅助线程最终会退出
            drop(join_thread);
        }
        // 关闭 Portal 返回的 PipeWire fd（BorrowedFd 不转移所有权，需手动关闭）
        if let Some(fd) = self.pipewire_fd.take() {
            unsafe {
                // 通过 OwnedFd 的 Drop 关闭 fd
                let _ = std::os::fd::OwnedFd::from_raw_fd(fd);
            }
        }
        info!("PipeWire 捕获器已释放");
    }
}

// Send + Sync: SharedState 通过 Arc<Mutex/Atomic> 保证线程安全
unsafe impl Send for PipeWireCapture {}
unsafe impl Sync for PipeWireCapture {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_to_bgra_bgrx() {
        // BGRx 格式只需填充 alpha
        let raw = vec![10, 20, 30, 0, 40, 50, 60, 0]; // 2 个像素
        let result = convert_to_bgra(&raw, 2, 1, 8, SpaFormat::BGRx);
        assert_eq!(result, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn test_convert_to_bgra_rgbx() {
        // RGBx 需要交换 R/B
        let raw = vec![30, 20, 10, 0]; // R=30, G=20, B=10
        let result = convert_to_bgra(&raw, 1, 1, 4, SpaFormat::RGBx);
        assert_eq!(result, vec![10, 20, 30, 255]); // B=10, G=20, R=30, A=255
    }

    #[test]
    fn test_convert_to_bgra_xrgb() {
        // xRGB: x=0, R=30, G=20, B=10 → B=10, G=20, R=30, A=255
        let raw = vec![0, 30, 20, 10];
        let result = convert_to_bgra(&raw, 1, 1, 4, SpaFormat::XRGB);
        assert_eq!(result, vec![10, 20, 30, 255]);
    }

    #[test]
    fn test_spa_format_roundtrip() {
        assert_eq!(SpaFormat::from(0), SpaFormat::BGRx);
        assert_eq!(SpaFormat::from(3), SpaFormat::RGBA);
        assert_eq!(SpaFormat::from(99), SpaFormat::Unknown);
    }

    #[test]
    fn test_restore_token_path() {
        let path = RestoreToken::token_path();
        assert!(path.to_string_lossy().contains("lan-desk"));
        assert!(path.to_string_lossy().contains("portal_restore_token"));
    }
}
