pub mod color_convert;
pub mod encoder;
pub mod frame;
pub mod gpu_encoder;
pub mod h264;

#[cfg(target_os = "windows")]
pub mod nvenc;

#[cfg(target_os = "windows")]
pub mod amf;

// QSV 跨平台（Windows + Linux）
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod qsv;

#[cfg(target_os = "macos")]
pub mod videotoolbox;

#[cfg(target_os = "linux")]
pub mod vaapi;

#[cfg(target_os = "linux")]
pub mod wayland;

#[cfg(all(target_os = "linux", feature = "pipewire-capture"))]
pub mod pipewire_portal;

#[cfg(target_os = "windows")]
pub mod win;

#[cfg(target_os = "macos")]
pub mod mac;

#[cfg(target_os = "linux")]
pub mod linux;

/// 屏幕捕获的跨平台 trait
pub trait ScreenCapture: Send + Sync + 'static {
    /// 捕获一帧完整屏幕，返回原始像素数据
    fn capture_frame(&mut self) -> anyhow::Result<frame::CapturedFrame>;

    /// 获取屏幕分辨率
    fn screen_size(&self) -> (u32, u32);
}

/// 获取当前系统 DPI 缩放比例（100 = 无缩放）
pub fn get_dpi_scale() -> u32 {
    #[cfg(target_os = "windows")]
    {
        unsafe {
            // 优先使用 GetDpiForSystem()（Win 10 1607+ / User32.dll），
            // 它返回系统级 DPI，比 GetDeviceCaps 更准确且不依赖 HDC。
            // 注意：该值仍然是系统级 DPI，不是 Per-Monitor DPI。
            // 若需要每个显示器独立 DPI，应使用 GetDpiForMonitor 配合 Per-Monitor DPI Aware 清单。
            use windows::Win32::UI::HiDpi::GetDpiForSystem;
            let dpi = GetDpiForSystem();
            if dpi > 0 {
                return (dpi * 100) / 96;
            }

            // 回退方案：使用 GetDeviceCaps(LOGPIXELSX)，仅获取主显示器 DPI
            use windows::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, LOGPIXELSX};
            let hdc = GetDC(None);
            if !hdc.is_invalid() {
                let dpi = GetDeviceCaps(hdc, LOGPIXELSX);
                ReleaseDC(None, hdc);
                (dpi as u32 * 100) / 96
            } else {
                100
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // 优先读取 GDK_SCALE（GTK 整数倍缩放）
        if let Ok(scale) = std::env::var("GDK_SCALE") {
            if let Ok(s) = scale.parse::<u32>() {
                return s * 100;
            }
        }
        // 其次检查 QT_SCALE_FACTOR（Qt 应用缩放因子，支持小数）
        if let Ok(scale) = std::env::var("QT_SCALE_FACTOR") {
            if let Ok(s) = scale.parse::<f64>() {
                return (s * 100.0) as u32;
            }
        }
        // 再检查 GDK_DPI_SCALE（GTK 小数缩放因子）
        if let Ok(scale) = std::env::var("GDK_DPI_SCALE") {
            if let Ok(s) = scale.parse::<f64>() {
                return (s * 100.0) as u32;
            }
        }
        100
    }

    #[cfg(target_os = "macos")]
    {
        // 通过 NSScreen.mainScreen.backingScaleFactor 获取实际 DPI 缩放
        // 使用 raw objc2 消息发送，避免 objc2-app-kit 版本间 feature gate 差异
        fn get_macos_scale() -> u32 {
            use objc2::runtime::AnyObject;
            use objc2::{class, msg_send};
            unsafe {
                let cls = class!(NSScreen);
                let screen: *mut AnyObject = msg_send![cls, mainScreen];
                if !screen.is_null() {
                    let scale: f64 = msg_send![&*screen, backingScaleFactor];
                    if scale > 0.0 {
                        return (scale * 100.0) as u32;
                    }
                }
            }
            200 // 获取失败时回退到 Retina 默认值
        }
        get_macos_scale()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        100
    }
}

/// 枚举可用显示器
pub fn list_monitors() -> anyhow::Result<Vec<lan_desk_protocol::message::MonitorInfo>> {
    #[cfg(target_os = "windows")]
    {
        win::list_monitors_win()
    }

    #[cfg(target_os = "macos")]
    {
        mac::list_monitors_mac()
    }

    #[cfg(target_os = "linux")]
    {
        linux::list_monitors_linux()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Ok(vec![])
    }
}

/// 根据当前平台创建屏幕捕获器（主显示器）
pub fn create_capturer() -> anyhow::Result<Box<dyn ScreenCapture>> {
    create_capturer_for_display(0)
}

/// 根据当前平台创建指定显示器的捕获器
pub fn create_capturer_for_display(display_index: usize) -> anyhow::Result<Box<dyn ScreenCapture>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(win::DxgiCapture::new_for_display(
            display_index as u32,
        )?))
    }

    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(mac::MacCapture::new(display_index)?))
    }

    #[cfg(target_os = "linux")]
    {
        // 三级 fallback：PipeWire/Portal → Wayland 外部工具 → X11 XShm
        if wayland::is_wayland_session() {
            // 级别 1: PipeWire/Portal（现代 Wayland 标准方案）
            #[cfg(feature = "pipewire-capture")]
            {
                if pipewire_portal::PipeWireCapture::is_available() {
                    match pipewire_portal::PipeWireCapture::new(display_index) {
                        Ok(cap) => {
                            tracing::info!("使用 PipeWire/Portal 屏幕捕获");
                            return Ok(Box::new(cap));
                        }
                        Err(e) => {
                            tracing::info!("PipeWire/Portal 不可用 ({}), 尝试外部工具", e);
                        }
                    }
                } else {
                    tracing::info!("PipeWire 库未安装，尝试 Wayland 外部工具");
                }
            }

            // 级别 2: 外部截图工具（grim/spectacle/gnome-screenshot）
            match wayland::WaylandCapture::new(display_index) {
                Ok(cap) => {
                    tracing::info!("使用 Wayland 外部工具屏幕捕获");
                    return Ok(Box::new(cap));
                }
                Err(e) => {
                    tracing::info!("Wayland 外部工具不可用 ({}), 回退 X11", e);
                }
            }
        }

        // 级别 3: X11 XShm（XWayland 或原生 X11）
        Ok(Box::new(linux::LinuxCapture::new(display_index)?))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = display_index;
        anyhow::bail!("当前平台暂不支持屏幕捕获")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_dpi_scale_returns_reasonable_value() {
        let scale = get_dpi_scale();
        // DPI 缩放应在 50-400% 范围内（0.5x - 4x）
        assert!(
            scale >= 50 && scale <= 400,
            "DPI scale {} 不在合理范围内",
            scale
        );
    }
}
