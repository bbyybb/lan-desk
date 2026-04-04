use std::sync::Mutex;

use core_graphics::display::CGDisplay;
use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use tracing::{debug, warn};

use lan_desk_protocol::message::MouseBtn;

use crate::InputInjector;

// CoreGraphics FFI（框架链接由 build.rs 处理）
extern "C" {
    fn CFRelease(cf: *const std::ffi::c_void);
    fn CGEventSetType(event: *mut std::ffi::c_void, event_type: u32);
    fn CGEventSetIntegerValueField(event: *mut std::ffi::c_void, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: *const std::ffi::c_void);
    fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
}

// macOS 辅助功能权限检测 API
extern "C" {
    /// 检查当前进程是否已获得辅助功能（Accessibility）权限
    fn AXIsProcessTrusted() -> bool;
}

/// 检查 macOS 辅助功能权限。
/// 输入注入（CGEvent）需要辅助功能权限才能正常工作。
fn ensure_accessibility_permission() -> anyhow::Result<()> {
    unsafe {
        if AXIsProcessTrusted() {
            debug!("macOS 辅助功能权限已授予");
            return Ok(());
        }

        warn!("macOS 辅助功能权限未授予");
        anyhow::bail!(
            "macOS 辅助功能权限未授予。\
             请前往「系统偏好设置 → 隐私与安全性 → 辅助功能」中授权本应用，然后重新启动。"
        );
    }
}

/// 活跃显示器边界 (left, top, width, height)
struct ActiveDisplayBounds {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

/// macOS 输入注入器（CGEvent API）
pub struct MacInputInjector {
    screen_width: f64,
    screen_height: f64,
    /// 最近一次鼠标移动的归一化坐标，用于鼠标按键事件定位
    last_mouse_pos: Mutex<(f64, f64)>,
    /// 当前捕获的显示器边界
    active_display: Mutex<ActiveDisplayBounds>,
}

impl MacInputInjector {
    pub fn new() -> anyhow::Result<Self> {
        // 在创建输入注入器前检查辅助功能权限
        ensure_accessibility_permission()?;

        let display = CGDisplay::main();
        let width = display.pixels_wide() as f64;
        let height = display.pixels_high() as f64;

        Ok(Self {
            screen_width: width,
            screen_height: height,
            last_mouse_pos: Mutex::new((0.0, 0.0)),
            active_display: Mutex::new(ActiveDisplayBounds {
                left: 0.0,
                top: 0.0,
                width,
                height,
            }),
        })
    }

    fn create_source(&self) -> Option<CGEventSource> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()
    }
}

impl InputInjector for MacInputInjector {
    fn set_active_monitor(&self, left: i32, top: i32, width: u32, height: u32) {
        // macOS CGEvent 坐标系使用逻辑点（非物理像素）
        // 传入的 width/height 是物理像素，left/top 是逻辑点坐标
        // 需要通过 CGDisplay 获取当前显示器的逻辑尺寸
        let displays = core_graphics::display::CGDisplay::active_displays().unwrap_or_default();
        let mut logical_w = width as f64;
        let mut logical_h = height as f64;
        for &did in &displays {
            let d = core_graphics::display::CGDisplay::new(did);
            if d.pixels_wide() as u32 == width && d.pixels_high() as u32 == height {
                let b = d.bounds();
                logical_w = b.size.width;
                logical_h = b.size.height;
                break;
            }
        }
        let mut bounds = self
            .active_display
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        bounds.left = left as f64;
        bounds.top = top as f64;
        bounds.width = logical_w;
        bounds.height = logical_h;
    }

    fn move_mouse(&self, x: f64, y: f64) -> anyhow::Result<()> {
        let bounds = self
            .active_display
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 从显示器归一化坐标 → 全局像素坐标
        let px = bounds.left + x * bounds.width;
        let py = bounds.top + y * bounds.height;
        drop(bounds);
        let point = CGPoint::new(px, py);

        let source = self
            .create_source()
            .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
        let event =
            CGEvent::new_mouse_event(source, CGEventType::MouseMoved, point, CGMouseButton::Left)
                .map_err(|_| anyhow::anyhow!("创建鼠标移动事件失败"))?;

        event.post(CGEventTapLocation::HID);
        *self
            .last_mouse_pos
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = (x, y);
        Ok(())
    }

    fn mouse_button(&self, button: MouseBtn, pressed: bool) -> anyhow::Result<()> {
        let (cg_button, down_type, up_type) = match button {
            MouseBtn::Left => (
                CGMouseButton::Left,
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
            ),
            MouseBtn::Right => (
                CGMouseButton::Right,
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
            ),
            MouseBtn::Middle => (
                CGMouseButton::Center,
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
            ),
        };

        let event_type = if pressed { down_type } else { up_type };
        let pos = *self
            .last_mouse_pos
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let bounds = self
            .active_display
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let point = CGPoint::new(
            bounds.left + pos.0 * bounds.width,
            bounds.top + pos.1 * bounds.height,
        );
        drop(bounds);

        let source = self
            .create_source()
            .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
        let event = CGEvent::new_mouse_event(source, event_type, point, cg_button)
            .map_err(|_| anyhow::anyhow!("创建鼠标按键事件失败"))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn mouse_scroll(&self, _dx: f64, dy: f64) -> anyhow::Result<()> {
        // 使用非变参 FFI 函数创建滚轮事件
        // CGEventCreateScrollEvent 是变参函数，在 macOS 新版 SDK + 旧部署目标下链接失败
        // 改用 CGEventCreate + CGEventSetType + CGEventSetIntegerValueField 组合实现
        const CG_EVENT_SCROLL_WHEEL: u32 = 22;
        const CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1: u32 = 11;
        const HID_EVENT_TAP: u32 = 0;

        let event_ref = unsafe { CGEventCreate(std::ptr::null()) };
        if event_ref.is_null() {
            anyhow::bail!("创建滚轮事件失败");
        }
        unsafe {
            CGEventSetType(event_ref, CG_EVENT_SCROLL_WHEEL);
            CGEventSetIntegerValueField(event_ref, CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1, dy as i64);
            CGEventPost(HID_EVENT_TAP, event_ref);
            CFRelease(event_ref);
        }
        Ok(())
    }

    fn key_event(&self, code: &str, pressed: bool, _modifiers: u8) -> anyhow::Result<()> {
        let keycode = code_to_macos_keycode(code);
        if keycode == 0xFFFF {
            return Ok(()); // 未知键位
        }

        let source = self
            .create_source()
            .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
        let event = CGEvent::new_keyboard_event(source, keycode, pressed)
            .map_err(|_| anyhow::anyhow!("创建键盘事件失败"))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn cursor_position(&self) -> (f64, f64) {
        if let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
            if let Ok(event) = CGEvent::new(source) {
                let point = event.location();
                let bounds = self
                    .active_display
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                // 归一化到当前捕获的显示器坐标系
                let x = if bounds.width > 0.0 {
                    (point.x - bounds.left) / bounds.width
                } else {
                    0.0
                };
                let y = if bounds.height > 0.0 {
                    (point.y - bounds.top) / bounds.height
                } else {
                    0.0
                };
                return (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
            }
        }
        (0.0, 0.0)
    }

    fn cursor_shape(&self) -> lan_desk_protocol::message::CursorShape {
        use lan_desk_protocol::message::CursorShape;
        use objc2::runtime::{AnyClass, AnyObject};
        use objc2::{class, msg_send};

        unsafe {
            let cls: &AnyClass = class!(NSCursor);

            // 获取当前系统光标（macOS 10.6+）
            let current: *mut AnyObject = msg_send![cls, currentSystemCursor];
            if current.is_null() {
                return CursorShape::Arrow;
            }

            // 获取各种系统光标并与当前光标做指针比较
            let arrow: *mut AnyObject = msg_send![cls, arrowCursor];
            if current == arrow {
                return CursorShape::Arrow;
            }

            let ibeam: *mut AnyObject = msg_send![cls, IBeamCursor];
            if current == ibeam {
                return CursorShape::IBeam;
            }

            let hand: *mut AnyObject = msg_send![cls, pointingHandCursor];
            if current == hand {
                return CursorShape::Hand;
            }

            let crosshair: *mut AnyObject = msg_send![cls, crosshairCursor];
            if current == crosshair {
                return CursorShape::Crosshair;
            }

            let resize_ns: *mut AnyObject = msg_send![cls, resizeUpDownCursor];
            if current == resize_ns {
                return CursorShape::ResizeNS;
            }

            let resize_ew: *mut AnyObject = msg_send![cls, resizeLeftRightCursor];
            if current == resize_ew {
                return CursorShape::ResizeEW;
            }

            let move_cursor: *mut AnyObject = msg_send![cls, openHandCursor];
            if current == move_cursor {
                return CursorShape::Move;
            }

            let not_allowed: *mut AnyObject = msg_send![cls, operationNotAllowedCursor];
            if current == not_allowed {
                return CursorShape::NotAllowed;
            }

            // 对角线 resize 光标 (NESW/NWSE) 无直接 NSCursor API，回退到 Arrow
            CursorShape::Arrow
        }
    }

    fn send_special_key(
        &self,
        key: lan_desk_protocol::message::SpecialKeyType,
    ) -> anyhow::Result<()> {
        use lan_desk_protocol::message::SpecialKeyType;
        match key {
            SpecialKeyType::CtrlAltDel => {
                anyhow::bail!("macOS 不支持此操作")
            }
            SpecialKeyType::AltTab => {
                // Cmd+Tab on macOS
                let source = self
                    .create_source()
                    .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
                if let Ok(event) = CGEvent::new_keyboard_event(source.clone(), 0x30, true) {
                    event.set_flags(core_graphics::event::CGEventFlags::CGEventFlagCommand);
                    event.post(CGEventTapLocation::HID);
                }
                let source2 = self
                    .create_source()
                    .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
                if let Ok(event) = CGEvent::new_keyboard_event(source2, 0x30, false) {
                    event.set_flags(core_graphics::event::CGEventFlags::CGEventFlagCommand);
                    event.post(CGEventTapLocation::HID);
                }
                Ok(())
            }
            SpecialKeyType::AltF4 => {
                // Cmd+Q on macOS
                let source = self
                    .create_source()
                    .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
                if let Ok(event) = CGEvent::new_keyboard_event(source.clone(), 0x0C, true) {
                    event.set_flags(core_graphics::event::CGEventFlags::CGEventFlagCommand);
                    event.post(CGEventTapLocation::HID);
                }
                let source2 = self
                    .create_source()
                    .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
                if let Ok(event) = CGEvent::new_keyboard_event(source2, 0x0C, false) {
                    event.set_flags(core_graphics::event::CGEventFlags::CGEventFlagCommand);
                    event.post(CGEventTapLocation::HID);
                }
                Ok(())
            }
            SpecialKeyType::PrintScreen => {
                // Cmd+Shift+3 on macOS
                let source = self
                    .create_source()
                    .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
                if let Ok(event) = CGEvent::new_keyboard_event(source.clone(), 0x14, true) {
                    event.set_flags(
                        core_graphics::event::CGEventFlags::CGEventFlagCommand
                            | core_graphics::event::CGEventFlags::CGEventFlagShift,
                    );
                    event.post(CGEventTapLocation::HID);
                }
                let source2 = self
                    .create_source()
                    .ok_or_else(|| anyhow::anyhow!("创建 CGEventSource 失败"))?;
                if let Ok(event) = CGEvent::new_keyboard_event(source2, 0x14, false) {
                    event.post(CGEventTapLocation::HID);
                }
                Ok(())
            }
            SpecialKeyType::WinKey => {
                // macOS 没有等价的 Win 键功能
                Ok(())
            }
            SpecialKeyType::WinL => {
                // Ctrl+Cmd+Q on macOS (锁屏)
                std::process::Command::new("osascript")
                    .args(["-e", r#"tell application "System Events" to keystroke "q" using {control down, command down}"#])
                    .spawn()?;
                Ok(())
            }
            SpecialKeyType::CtrlEsc => {
                // macOS 没有等价功能
                Ok(())
            }
        }
    }
}

/// KeyboardEvent.code → macOS keycode
fn code_to_macos_keycode(code: &str) -> u16 {
    match code {
        // 字母键
        "KeyA" => 0x00,
        "KeyS" => 0x01,
        "KeyD" => 0x02,
        "KeyF" => 0x03,
        "KeyH" => 0x04,
        "KeyG" => 0x05,
        "KeyZ" => 0x06,
        "KeyX" => 0x07,
        "KeyC" => 0x08,
        "KeyV" => 0x09,
        "KeyB" => 0x0B,
        "KeyQ" => 0x0C,
        "KeyW" => 0x0D,
        "KeyE" => 0x0E,
        "KeyR" => 0x0F,
        "KeyY" => 0x10,
        "KeyT" => 0x11,
        "KeyI" => 0x22,
        "KeyO" => 0x1F,
        "KeyP" => 0x23,
        "KeyL" => 0x25,
        "KeyJ" => 0x26,
        "KeyK" => 0x28,
        "KeyN" => 0x2D,
        "KeyM" => 0x2E,
        "KeyU" => 0x20,
        // 数字键
        "Digit1" => 0x12,
        "Digit2" => 0x13,
        "Digit3" => 0x14,
        "Digit4" => 0x15,
        "Digit5" => 0x17,
        "Digit6" => 0x16,
        "Digit7" => 0x1A,
        "Digit8" => 0x1C,
        "Digit9" => 0x19,
        "Digit0" => 0x1D,
        // 功能键
        "F1" => 0x7A,
        "F2" => 0x78,
        "F3" => 0x63,
        "F4" => 0x76,
        "F5" => 0x60,
        "F6" => 0x61,
        "F7" => 0x62,
        "F8" => 0x64,
        "F9" => 0x65,
        "F10" => 0x6D,
        "F11" => 0x67,
        "F12" => 0x6F,
        // 控制键
        "Enter" => 0x24,
        "Escape" => 0x35,
        "Backspace" => 0x33,
        "Tab" => 0x30,
        "Space" => 0x31,
        "Delete" => 0x75,
        "Home" => 0x73,
        "End" => 0x77,
        "PageUp" => 0x74,
        "PageDown" => 0x79,
        // 方向键
        "ArrowUp" => 0x7E,
        "ArrowDown" => 0x7D,
        "ArrowLeft" => 0x7B,
        "ArrowRight" => 0x7C,
        // 修饰键
        "ShiftLeft" => 0x38,
        "ShiftRight" => 0x3C,
        "ControlLeft" => 0x3B,
        "ControlRight" => 0x3E,
        "AltLeft" => 0x3A,
        "AltRight" => 0x3D,
        "MetaLeft" => 0x37,
        "MetaRight" => 0x36,
        "CapsLock" => 0x39,
        // 符号键
        "Minus" => 0x1B,
        "Equal" => 0x18,
        "BracketLeft" => 0x21,
        "BracketRight" => 0x1E,
        "Backslash" => 0x2A,
        "Semicolon" => 0x29,
        "Quote" => 0x27,
        "Backquote" => 0x32,
        "Comma" => 0x2B,
        "Period" => 0x2F,
        "Slash" => 0x2C,
        _ => 0xFFFF,
    }
}
