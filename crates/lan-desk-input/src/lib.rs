pub mod types;

#[cfg(target_os = "windows")]
pub mod win;

#[cfg(target_os = "macos")]
pub mod mac;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub mod wayland;

use lan_desk_protocol::message::MouseBtn;

/// 输入注入的跨平台 trait
pub trait InputInjector: Send + Sync + 'static {
    /// 移动鼠标到归一化坐标 (0.0~1.0)
    fn move_mouse(&self, x: f64, y: f64) -> anyhow::Result<()>;

    /// 鼠标按键
    fn mouse_button(&self, button: MouseBtn, pressed: bool) -> anyhow::Result<()>;

    /// 鼠标滚轮
    fn mouse_scroll(&self, dx: f64, dy: f64) -> anyhow::Result<()>;

    /// 键盘事件（code 为 KeyboardEvent.code 标识，如 "KeyA", "Enter"）
    fn key_event(&self, code: &str, pressed: bool, modifiers: u8) -> anyhow::Result<()>;

    /// 获取当前鼠标位置（归一化 0.0~1.0）
    fn cursor_position(&self) -> (f64, f64);

    /// 获取当前光标形状
    fn cursor_shape(&self) -> lan_desk_protocol::message::CursorShape;

    /// 设置当前活跃（正在捕获）的显示器边界，用于坐标映射
    /// (left, top, width, height) 为像素值
    fn set_active_monitor(&self, _left: i32, _top: i32, _width: u32, _height: u32) {
        // 默认空实现
    }

    /// 发送特殊组合键（如 Ctrl+Alt+Del, Alt+Tab 等）
    fn send_special_key(
        &self,
        _key: lan_desk_protocol::message::SpecialKeyType,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// 根据当前平台创建输入注入器
pub fn create_injector() -> anyhow::Result<Box<dyn InputInjector>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(win::WindowsInputInjector::new()))
    }

    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(mac::MacInputInjector::new()?))
    }

    #[cfg(target_os = "linux")]
    {
        // 检测纯 Wayland 环境（有 WAYLAND_DISPLAY 但无 DISPLAY/XWayland）
        let is_native_wayland =
            std::env::var("WAYLAND_DISPLAY").is_ok() && std::env::var("DISPLAY").is_err();

        if is_native_wayland {
            match wayland::WaylandInputInjector::new() {
                Ok(injector) => return Ok(Box::new(injector)),
                Err(e) => {
                    tracing::info!("Wayland 原生输入注入不可用 ({}), 回退 XWayland", e);
                }
            }
        }
        Ok(Box::new(linux::LinuxInputInjector::new()?))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("当前平台暂不支持输入注入")
    }
}
