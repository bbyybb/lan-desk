/// Linux 屏幕捕获实现（X11 XShm + GetImage 回退）
///
/// 支持两种模式（运行时自动选择）：
/// - SHM 共享内存：高效零拷贝，需要 X server 支持 MIT-SHM 扩展
/// - GetImage 回退：通用兼容，性能较低
///
/// Wayland 用户：当前版本通过 XWayland 兼容层工作。
/// 原生 Wayland 捕获（XDG Desktop Portal + PipeWire）计划在后续版本实现。
use crate::frame::{CapturedFrame, PixelFormat};
use crate::ScreenCapture;
use lan_desk_protocol::message::MonitorInfo;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;

pub struct LinuxCapture {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
    width: u32,
    height: u32,
    use_shm: bool,
    shm_seg: Option<u32>,
    shm_id: Option<i32>,
    shm_ptr: Option<*mut u8>,
    shm_size: usize,
}

// SAFETY: LinuxCapture 在独立捕获线程中使用，X11 连接和 SHM 指针不跨线程共享
unsafe impl Send for LinuxCapture {}

impl LinuxCapture {
    pub fn new(display_index: usize) -> anyhow::Result<Self> {
        use x11rb::connection::Connection;

        let (conn, screen_num) =
            x11rb::rust_connection::RustConnection::connect(None).map_err(|e| {
                anyhow::anyhow!(
                    "无法连接 X11 显示服务器: {}。如果使用 Wayland，请确保 XWayland 已启用。",
                    e
                )
            })?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        // 获取显示器尺寸
        let (width, height) = if display_index == 0 {
            (
                screen.width_in_pixels as u32,
                screen.height_in_pixels as u32,
            )
        } else {
            get_monitor_geometry(&conn, root, display_index).unwrap_or((
                screen.width_in_pixels as u32,
                screen.height_in_pixels as u32,
            ))
        };

        // 尝试初始化 SHM
        let (use_shm, shm_seg, shm_id, shm_ptr, shm_size) = match init_shm(&conn, width, height) {
            Ok((seg, id, ptr, size)) => {
                info!("X11 SHM 屏幕捕获已初始化: {}x{}", width, height);
                (true, Some(seg), Some(id), Some(ptr), size)
            }
            Err(e) => {
                warn!("X11 SHM 不可用（{}），回退 GetImage 模式", e);
                (false, None, None, None, 0)
            }
        };

        info!(
            "Linux 屏幕捕获已就绪: {}x{}, SHM={}",
            width, height, use_shm
        );

        Ok(Self {
            conn,
            root,
            width,
            height,
            use_shm,
            shm_seg,
            shm_id,
            shm_ptr,
            shm_size,
        })
    }
}

impl ScreenCapture for LinuxCapture {
    fn capture_frame(&mut self) -> anyhow::Result<CapturedFrame> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::*;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if self.use_shm {
            if let (Some(seg), Some(ptr)) = (self.shm_seg, self.shm_ptr) {
                use x11rb::protocol::shm;
                shm::get_image(
                    &self.conn,
                    self.root as Drawable,
                    0,
                    0,
                    self.width as u16,
                    self.height as u16,
                    0xFFFFFFFF,
                    ImageFormat::Z_PIXMAP.into(),
                    seg,
                    0,
                )?;
                self.conn.flush()?;
                self.conn.sync()?;

                let data = unsafe { std::slice::from_raw_parts(ptr, self.shm_size).to_vec() };

                return Ok(CapturedFrame {
                    width: self.width,
                    height: self.height,
                    stride: self.width * 4,
                    pixel_format: PixelFormat::Bgra8,
                    data,
                    timestamp_ms: timestamp,
                });
            }
        }

        // GetImage 回退
        let reply = get_image(
            &self.conn,
            ImageFormat::Z_PIXMAP,
            self.root as Drawable,
            0,
            0,
            self.width as u16,
            self.height as u16,
            0xFFFFFFFF,
        )?
        .reply()?;

        // 从实际返回数据计算 stride，比硬编码 width*4 更安全
        let actual_stride = if self.height > 0 {
            reply.data.len() as u32 / self.height
        } else {
            self.width * 4
        };

        Ok(CapturedFrame {
            width: self.width,
            height: self.height,
            stride: actual_stride,
            pixel_format: PixelFormat::Bgra8,
            data: reply.data,
            timestamp_ms: timestamp,
        })
    }

    fn screen_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl Drop for LinuxCapture {
    fn drop(&mut self) {
        if let Some(seg) = self.shm_seg {
            use x11rb::protocol::shm;
            let _ = shm::detach(&self.conn, seg);
            let _ = self.conn.flush();
        }
        if let (Some(ptr), size) = (self.shm_ptr, self.shm_size) {
            if size > 0 {
                unsafe {
                    libc::shmdt(ptr as *const libc::c_void);
                }
            }
        }
        if let Some(id) = self.shm_id {
            unsafe {
                libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
            }
        }
    }
}

/// 初始化 X11 SHM 共享内存段
fn init_shm(
    conn: &x11rb::rust_connection::RustConnection,
    width: u32,
    height: u32,
) -> anyhow::Result<(u32, i32, *mut u8, usize)> {
    use x11rb::connection::Connection;
    use x11rb::protocol::shm;

    shm::query_version(conn)?.reply()?;

    let size = (width * height * 4) as usize;
    let shm_id = unsafe { libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o600) };
    if shm_id < 0 {
        anyhow::bail!("shmget 失败: {}", std::io::Error::last_os_error());
    }

    let shm_ptr = unsafe { libc::shmat(shm_id, std::ptr::null(), 0) };
    if shm_ptr == (-1isize) as *mut libc::c_void {
        unsafe {
            libc::shmctl(shm_id, libc::IPC_RMID, std::ptr::null_mut());
        }
        anyhow::bail!("shmat 失败: {}", std::io::Error::last_os_error());
    }

    let seg = conn.generate_id()?;
    shm::attach(conn, seg, shm_id as u32, false)?;
    conn.flush()?;

    Ok((seg, shm_id, shm_ptr as *mut u8, size))
}

/// 使用 RandR 获取指定显示器的尺寸
fn get_monitor_geometry(
    conn: &x11rb::rust_connection::RustConnection,
    root: u32,
    index: usize,
) -> Option<(u32, u32)> {
    use x11rb::protocol::randr;

    let monitors = randr::get_monitors(conn, root, true).ok()?.reply().ok()?;
    monitors
        .monitors
        .get(index)
        .map(|m| (m.width as u32, m.height as u32))
}

/// 枚举所有显示器（通过 RandR）
pub fn list_monitors_linux() -> anyhow::Result<Vec<MonitorInfo>> {
    use x11rb::protocol::randr;

    let result = (|| -> anyhow::Result<Vec<MonitorInfo>> {
        let (conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)?;
        let root = conn.setup().roots[screen_num].root;
        let monitors = randr::get_monitors(&conn, root, true)?.reply()?;

        Ok(monitors
            .monitors
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let name = conn
                    .get_atom_name(m.name)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .map(|reply| String::from_utf8_lossy(&reply.name).to_string())
                    .unwrap_or_else(|| format!("Monitor {}", i));

                MonitorInfo {
                    index: i as u32,
                    name,
                    width: m.width as u32,
                    height: m.height as u32,
                    is_primary: m.primary,
                    left: m.x as i32,
                    top: m.y as i32,
                }
            })
            .collect())
    })();

    match result {
        Ok(monitors) if !monitors.is_empty() => Ok(monitors),
        Ok(_) => anyhow::bail!("未检测到任何显示器"),
        Err(e) => Err(e),
    }
}
