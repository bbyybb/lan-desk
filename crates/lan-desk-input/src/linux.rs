/// Linux 输入注入实现（X11 XTest 扩展）
///
/// 使用 x11rb 的 XTest 扩展注入鼠标和键盘事件。
/// Wayland 环境下通过 XWayland 兼容层工作。
/// 原生 Wayland uinput 注入计划在后续版本实现。
use std::sync::Mutex;

use crate::InputInjector;
use lan_desk_protocol::message::{CursorShape, MouseBtn};
use tracing::{info, warn};

/// 活跃显示器边界
struct ActiveMonitorBounds {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

pub struct LinuxInputInjector {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
    screen_width: f64,
    screen_height: f64,
    /// 缓存的键盘映射 (keysym -> keycode)
    keymap_cache: std::collections::HashMap<u32, u8>,
    /// XFixes 扩展是否可用（用于光标形状检测）
    xfixes_available: bool,
    /// 当前捕获的显示器边界
    active_monitor: Mutex<ActiveMonitorBounds>,
}

// SAFETY: LinuxInputInjector 在独立 session 线程中使用
unsafe impl Send for LinuxInputInjector {}

impl LinuxInputInjector {
    pub fn new() -> anyhow::Result<Self> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xtest;

        let (conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)
            .map_err(|e| anyhow::anyhow!("无法连接 X11: {}", e))?;

        // 检查 XTest 扩展
        xtest::get_version(&conn, 2, 2)?.reply()?;

        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let screen_width = screen.width_in_pixels as f64;
        let screen_height = screen.height_in_pixels as f64;

        // 初始化 XFixes 扩展（用于光标形状检测）
        let xfixes_available = {
            use x11rb::protocol::xfixes;
            match xfixes::query_version(&conn, 4, 0) {
                Ok(cookie) => cookie.reply().is_ok(),
                Err(_) => false,
            }
        };
        if xfixes_available {
            info!("XFixes 扩展已初始化，光标形状检测可用");
        } else {
            warn!("XFixes 扩展不可用，光标形状将固定为 Arrow");
        }

        info!(
            "Linux XTest 输入注入已初始化: {}x{}",
            screen_width, screen_height
        );

        // 预加载键盘映射缓存
        let mut keymap_cache = std::collections::HashMap::new();
        {
            use x11rb::connection::Connection;
            use x11rb::protocol::xproto;
            let setup = conn.setup();
            let min_kc = setup.min_keycode;
            let max_kc = setup.max_keycode;
            if let Ok(reply) =
                xproto::get_keyboard_mapping(&conn, min_kc, max_kc - min_kc + 1)?.reply()
            {
                let per_kc = reply.keysyms_per_keycode as usize;
                for i in 0..=(max_kc - min_kc) as usize {
                    for j in 0..per_kc {
                        let sym = reply.keysyms[i * per_kc + j];
                        if sym != 0 {
                            keymap_cache.entry(sym).or_insert(min_kc + i as u8);
                        }
                    }
                }
            }
        }

        Ok(Self {
            conn,
            root,
            screen_width,
            screen_height,
            keymap_cache,
            xfixes_available,
            active_monitor: Mutex::new(ActiveMonitorBounds {
                left: 0.0,
                top: 0.0,
                width: screen_width,
                height: screen_height,
            }),
        })
    }
}

impl InputInjector for LinuxInputInjector {
    fn set_active_monitor(&self, left: i32, top: i32, width: u32, height: u32) {
        let mut bounds = self
            .active_monitor
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        bounds.left = left as f64;
        bounds.top = top as f64;
        bounds.width = width as f64;
        bounds.height = height as f64;
    }

    fn move_mouse(&self, x: f64, y: f64) -> anyhow::Result<()> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto;
        use x11rb::protocol::xtest;

        let bounds = self
            .active_monitor
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let abs_x = (bounds.left + x * bounds.width) as i16;
        let abs_y = (bounds.top + y * bounds.height) as i16;
        drop(bounds);

        xtest::fake_input(
            &self.conn,
            xproto::MOTION_NOTIFY_EVENT,
            0, // detail
            0, // time
            self.root,
            abs_x,
            abs_y,
            0, // device_id
        )?;
        self.conn.flush()?;
        Ok(())
    }

    fn mouse_button(&self, button: MouseBtn, pressed: bool) -> anyhow::Result<()> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto;
        use x11rb::protocol::xtest;

        let x11_button: u8 = match button {
            MouseBtn::Left => 1,
            MouseBtn::Middle => 2,
            MouseBtn::Right => 3,
        };

        let event_type = if pressed {
            xproto::BUTTON_PRESS_EVENT
        } else {
            xproto::BUTTON_RELEASE_EVENT
        };

        xtest::fake_input(&self.conn, event_type, x11_button, 0, self.root, 0, 0, 0)?;
        self.conn.flush()?;
        Ok(())
    }

    fn mouse_scroll(&self, _dx: f64, dy: f64) -> anyhow::Result<()> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto;
        use x11rb::protocol::xtest;

        // X11 滚轮：按钮 4 = 向上, 按钮 5 = 向下
        let button = if dy > 0.0 { 4u8 } else { 5u8 };
        let clicks = (dy.abs() as u32).max(1);

        for _ in 0..clicks {
            xtest::fake_input(
                &self.conn,
                xproto::BUTTON_PRESS_EVENT,
                button,
                0,
                self.root,
                0,
                0,
                0,
            )?;
            xtest::fake_input(
                &self.conn,
                xproto::BUTTON_RELEASE_EVENT,
                button,
                0,
                self.root,
                0,
                0,
                0,
            )?;
        }
        self.conn.flush()?;
        Ok(())
    }

    fn key_event(&self, code: &str, pressed: bool, _modifiers: u8) -> anyhow::Result<()> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto;
        use x11rb::protocol::xtest;

        let keysym = code_to_keysym(code);
        if keysym == 0 {
            return Ok(());
        }

        // 使用缓存的键盘映射，避免每次按键都查询 X server
        if let Some(&kc) = self.keymap_cache.get(&keysym) {
            let event_type = if pressed {
                xproto::KEY_PRESS_EVENT
            } else {
                xproto::KEY_RELEASE_EVENT
            };
            xtest::fake_input(&self.conn, event_type, kc, 0, self.root, 0, 0, 0)?;
            self.conn.flush()?;
        }

        Ok(())
    }

    fn cursor_position(&self) -> (f64, f64) {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto;

        if let Ok(cookie) = xproto::query_pointer(&self.conn, self.root) {
            if let Ok(reply) = cookie.reply() {
                let bounds = self
                    .active_monitor
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let x = if bounds.width > 0.0 {
                    (reply.root_x as f64 - bounds.left) / bounds.width
                } else {
                    0.0
                };
                let y = if bounds.height > 0.0 {
                    (reply.root_y as f64 - bounds.top) / bounds.height
                } else {
                    0.0
                };
                return (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
            }
        }
        (0.0, 0.0)
    }

    fn cursor_shape(&self) -> CursorShape {
        if !self.xfixes_available {
            return CursorShape::Arrow;
        }

        use x11rb::connection::Connection;
        use x11rb::protocol::xfixes;
        use x11rb::protocol::xproto;

        // 通过 XFixes 获取当前光标名称
        let cursor_name = match xfixes::get_cursor_name(&self.conn, 0u32) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => {
                    // reply.atom 包含光标名称对应的 atom，需解析为字符串
                    match xproto::get_atom_name(&self.conn, reply.atom) {
                        Ok(name_cookie) => match name_cookie.reply() {
                            Ok(name_reply) => String::from_utf8_lossy(&name_reply.name).to_string(),
                            Err(_) => return CursorShape::Arrow,
                        },
                        Err(_) => return CursorShape::Arrow,
                    }
                }
                Err(_) => return CursorShape::Arrow,
            },
            Err(_) => return CursorShape::Arrow,
        };

        // 将 freedesktop cursor-spec 光标名称映射到 CursorShape
        // 参考: https://www.freedesktop.org/wiki/Specifications/cursor-spec/
        match cursor_name.as_str() {
            "default" | "left_ptr" | "arrow" => CursorShape::Arrow,
            "text" | "xterm" | "ibeam" => CursorShape::IBeam,
            "pointer" | "hand2" | "hand1" | "pointing_hand" => CursorShape::Hand,
            "crosshair" | "cross" => CursorShape::Crosshair,
            "sb_v_double_arrow" | "ns-resize" | "row-resize" | "n-resize" | "s-resize" => {
                CursorShape::ResizeNS
            }
            "sb_h_double_arrow" | "ew-resize" | "col-resize" | "e-resize" | "w-resize" => {
                CursorShape::ResizeEW
            }
            "nesw-resize" | "size_bdiag" | "fd_double_arrow" => CursorShape::ResizeNESW,
            "nwse-resize" | "size_fdiag" | "bd_double_arrow" => CursorShape::ResizeNWSE,
            "fleur" | "all-scroll" | "move" | "grab" | "grabbing" => CursorShape::Move,
            "wait" | "watch" => CursorShape::Wait,
            "help" | "question_arrow" => CursorShape::Help,
            "not-allowed" | "crossed_circle" | "no-drop" | "X_cursor" => CursorShape::NotAllowed,
            _ => CursorShape::Arrow,
        }
    }

    fn send_special_key(
        &self,
        key: lan_desk_protocol::message::SpecialKeyType,
    ) -> anyhow::Result<()> {
        use lan_desk_protocol::message::SpecialKeyType;
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto;
        use x11rb::protocol::xtest;

        // 辅助闭包：发送按键组合
        let send_combo = |keysyms: &[u32]| -> anyhow::Result<()> {
            let mut keycodes: Vec<u8> = Vec::new();
            for &sym in keysyms {
                if let Some(&kc) = self.keymap_cache.get(&sym) {
                    keycodes.push(kc);
                }
            }
            // key down
            for &kc in &keycodes {
                xtest::fake_input(
                    &self.conn,
                    xproto::KEY_PRESS_EVENT,
                    kc,
                    0,
                    self.root,
                    0,
                    0,
                    0,
                )?;
            }
            // key up (reverse)
            for &kc in keycodes.iter().rev() {
                xtest::fake_input(
                    &self.conn,
                    xproto::KEY_RELEASE_EVENT,
                    kc,
                    0,
                    self.root,
                    0,
                    0,
                    0,
                )?;
            }
            self.conn.flush()?;
            Ok(())
        };

        match key {
            SpecialKeyType::CtrlAltDel => {
                // Ctrl+Alt+Del
                send_combo(&[0xffe3, 0xffe9, 0xffff]) // Control_L, Alt_L, Delete
            }
            SpecialKeyType::AltTab => {
                send_combo(&[0xffe9, 0xff09]) // Alt_L, Tab
            }
            SpecialKeyType::AltF4 => {
                send_combo(&[0xffe9, 0xffc1]) // Alt_L, F4
            }
            SpecialKeyType::PrintScreen => {
                send_combo(&[0xff61]) // Print
            }
            SpecialKeyType::WinKey => {
                send_combo(&[0xffeb]) // Super_L
            }
            SpecialKeyType::WinL => {
                // 锁屏：调用 loginctl lock-session
                std::process::Command::new("loginctl")
                    .arg("lock-session")
                    .spawn()?;
                Ok(())
            }
            SpecialKeyType::CtrlEsc => {
                send_combo(&[0xffe3, 0xff1b]) // Control_L, Escape
            }
        }
    }
}

/// 将 web KeyboardEvent.code 映射到 X11 KeySym
fn code_to_keysym(code: &str) -> u32 {
    match code {
        // 字母键
        "KeyA" => 0x0061,
        "KeyB" => 0x0062,
        "KeyC" => 0x0063,
        "KeyD" => 0x0064,
        "KeyE" => 0x0065,
        "KeyF" => 0x0066,
        "KeyG" => 0x0067,
        "KeyH" => 0x0068,
        "KeyI" => 0x0069,
        "KeyJ" => 0x006a,
        "KeyK" => 0x006b,
        "KeyL" => 0x006c,
        "KeyM" => 0x006d,
        "KeyN" => 0x006e,
        "KeyO" => 0x006f,
        "KeyP" => 0x0070,
        "KeyQ" => 0x0071,
        "KeyR" => 0x0072,
        "KeyS" => 0x0073,
        "KeyT" => 0x0074,
        "KeyU" => 0x0075,
        "KeyV" => 0x0076,
        "KeyW" => 0x0077,
        "KeyX" => 0x0078,
        "KeyY" => 0x0079,
        "KeyZ" => 0x007a,

        // 数字键
        "Digit0" => 0x0030,
        "Digit1" => 0x0031,
        "Digit2" => 0x0032,
        "Digit3" => 0x0033,
        "Digit4" => 0x0034,
        "Digit5" => 0x0035,
        "Digit6" => 0x0036,
        "Digit7" => 0x0037,
        "Digit8" => 0x0038,
        "Digit9" => 0x0039,

        // 功能键
        "F1" => 0xffbe,
        "F2" => 0xffbf,
        "F3" => 0xffc0,
        "F4" => 0xffc1,
        "F5" => 0xffc2,
        "F6" => 0xffc3,
        "F7" => 0xffc4,
        "F8" => 0xffc5,
        "F9" => 0xffc6,
        "F10" => 0xffc7,
        "F11" => 0xffc8,
        "F12" => 0xffc9,

        // 修饰键
        "ShiftLeft" => 0xffe1,
        "ShiftRight" => 0xffe2,
        "ControlLeft" => 0xffe3,
        "ControlRight" => 0xffe4,
        "AltLeft" => 0xffe9,
        "AltRight" => 0xffea,
        "MetaLeft" => 0xffeb,
        "MetaRight" => 0xffec,
        "CapsLock" => 0xffe5,
        "NumLock" => 0xff7f,
        "ScrollLock" => 0xff14,

        // 控制键
        "Enter" => 0xff0d,
        "Tab" => 0xff09,
        "Space" => 0x0020,
        "Backspace" => 0xff08,
        "Escape" => 0xff1b,
        "Delete" => 0xffff,
        "Insert" => 0xff63,
        "Home" => 0xff50,
        "End" => 0xff57,
        "PageUp" => 0xff55,
        "PageDown" => 0xff56,
        "PrintScreen" => 0xff61,
        "Pause" => 0xff13,

        // 方向键
        "ArrowUp" => 0xff52,
        "ArrowDown" => 0xff54,
        "ArrowLeft" => 0xff51,
        "ArrowRight" => 0xff53,

        // 符号键
        "Minus" => 0x002d,
        "Equal" => 0x003d,
        "BracketLeft" => 0x005b,
        "BracketRight" => 0x005d,
        "Backslash" => 0x005c,
        "Semicolon" => 0x003b,
        "Quote" => 0x0027,
        "Backquote" => 0x0060,
        "Comma" => 0x002c,
        "Period" => 0x002e,
        "Slash" => 0x002f,

        // 小键盘
        "Numpad0" => 0xffb0,
        "Numpad1" => 0xffb1,
        "Numpad2" => 0xffb2,
        "Numpad3" => 0xffb3,
        "Numpad4" => 0xffb4,
        "Numpad5" => 0xffb5,
        "Numpad6" => 0xffb6,
        "Numpad7" => 0xffb7,
        "Numpad8" => 0xffb8,
        "Numpad9" => 0xffb9,
        "NumpadAdd" => 0xffab,
        "NumpadSubtract" => 0xffad,
        "NumpadMultiply" => 0xffaa,
        "NumpadDivide" => 0xffaf,
        "NumpadDecimal" => 0xffae,
        "NumpadEnter" => 0xff8d,

        _ => {
            if !code.is_empty() {
                tracing::debug!("未映射的键位: {}", code);
            }
            0
        }
    }
}
