use std::time::{SystemTime, UNIX_EPOCH};

use core_graphics::display::CGDisplay;
use foreign_types::ForeignType;
use tracing::{debug, warn};

use lan_desk_protocol::message::MonitorInfo;

use crate::frame::{CapturedFrame, PixelFormat};
use crate::ScreenCapture;

// CoreGraphics FFI（模块顶层声明，确保链接器正确链接框架）
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// 检查当前进程是否已获得屏幕录制权限（不会弹出系统提示）
    fn CGPreflightScreenCaptureAccess() -> bool;
    /// 请求屏幕录制权限（首次调用会弹出系统授权对话框）
    fn CGRequestScreenCaptureAccess() -> bool;
    /// 获取 CGImage 的位图信息（像素格式标志）
    fn CGImageGetBitmapInfo(image: *const std::ffi::c_void) -> u32;
}

/// 检查并请求 macOS 屏幕录制权限。
/// 如果权限未授予，会尝试请求；请求被拒绝则返回错误。
fn ensure_screen_capture_permission() -> anyhow::Result<()> {
    unsafe {
        if CGPreflightScreenCaptureAccess() {
            debug!("macOS 屏幕录制权限已授予");
            return Ok(());
        }

        warn!("macOS 屏幕录制权限未授予，正在请求...");
        if CGRequestScreenCaptureAccess() {
            debug!("macOS 屏幕录制权限请求已通过");
            return Ok(());
        }

        anyhow::bail!(
            "macOS 屏幕录制权限被拒绝。\
             请前往「系统偏好设置 → 隐私与安全性 → 屏幕录制」中授权本应用，然后重新启动。"
        );
    }
}

/// 枚举 macOS 显示器
pub fn list_monitors_mac() -> anyhow::Result<Vec<MonitorInfo>> {
    let displays =
        CGDisplay::active_displays().map_err(|e| anyhow::anyhow!("获取显示器列表失败: {:?}", e))?;

    let mut monitors = Vec::new();
    for (i, &display_id) in displays.iter().enumerate() {
        let display = CGDisplay::new(display_id);
        let bounds = display.bounds();
        // 使用物理像素（与捕获图像一致），但 left/top 使用逻辑点坐标（与 CGEvent 坐标系一致）
        monitors.push(MonitorInfo {
            index: i as u32,
            name: format!("显示器 {}", i + 1),
            width: display.pixels_wide() as u32,
            height: display.pixels_high() as u32,
            is_primary: display.is_main(),
            left: bounds.origin.x as i32,
            top: bounds.origin.y as i32,
        });
    }
    Ok(monitors)
}

/// macOS 屏幕捕获器（基于 CGDisplayCreateImage）
pub struct MacCapture {
    display_id: u32,
    width: u32,
    height: u32,
}

impl MacCapture {
    pub fn new(display_index: usize) -> anyhow::Result<Self> {
        // 在创建捕获器前检查屏幕录制权限
        ensure_screen_capture_permission()?;

        let displays = CGDisplay::active_displays()
            .map_err(|e| anyhow::anyhow!("获取显示器列表失败: {:?}", e))?;

        if display_index >= displays.len() {
            anyhow::bail!(
                "显示器索引 {} 超出范围（共 {} 个显示器）",
                display_index,
                displays.len()
            );
        }

        let display_id = displays[display_index];
        let display = CGDisplay::new(display_id);
        let width = display.pixels_wide() as u32;
        let height = display.pixels_high() as u32;

        debug!(
            "macOS 屏幕捕获初始化: 显示器 {}, {}x{}",
            display_id, width, height
        );

        Ok(Self {
            display_id,
            width,
            height,
        })
    }
}

impl ScreenCapture for MacCapture {
    fn capture_frame(&mut self) -> anyhow::Result<CapturedFrame> {
        let display = CGDisplay::new(self.display_id);
        let image = display.image().ok_or_else(|| {
            anyhow::anyhow!("CGDisplayCreateImage 返回 null（可能缺少屏幕录制权限）")
        })?;

        let width = image.width() as u32;
        let height = image.height() as u32;
        let bytes_per_row = image.bytes_per_row() as u32;
        let data = image.data();
        let pixel_data = data.bytes().to_vec();

        // 检测实际像素格式：macOS 不同硬件可能返回不同格式
        let bitmap_info = unsafe { CGImageGetBitmapInfo(image.as_ptr() as *const _) };
        let alpha_info = bitmap_info & 0x1F; // kCGBitmapAlphaInfoMask
        let pixel_format = if alpha_info == 2 || alpha_info == 6 {
            // PremultipliedLast (2) 或 NoneSkipLast (6) → RGBA 字节序
            PixelFormat::Rgba8
        } else {
            // PremultipliedFirst (1) 或 NoneSkipFirst (5) → BGRA 字节序（最常见）
            PixelFormat::Bgra8
        };

        Ok(CapturedFrame {
            width,
            height,
            stride: bytes_per_row,
            pixel_format,
            data: pixel_data,
            timestamp_ms: now_ms(),
        })
    }

    fn screen_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
