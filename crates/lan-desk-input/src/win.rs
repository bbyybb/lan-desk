use std::sync::Mutex;

use lan_desk_protocol::message::CursorShape;
use lan_desk_protocol::message::{MouseBtn, SpecialKeyType};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::InputInjector;

/// 缓存的系统光标句柄，避免每帧调用 LoadCursorW
struct CachedCursors {
    arrow: HCURSOR,
    ibeam: HCURSOR,
    hand: HCURSOR,
    cross: HCURSOR,
    size_ns: HCURSOR,
    size_we: HCURSOR,
    size_nesw: HCURSOR,
    size_nwse: HCURSOR,
    size_all: HCURSOR,
    wait: HCURSOR,
    help: HCURSOR,
    no: HCURSOR,
}

// 安全性说明：系统光标句柄由 LoadCursorW 返回，是进程级全局资源，
// 生命周期覆盖整个进程，跨线程只读使用是安全的。
unsafe impl Send for CachedCursors {}
unsafe impl Sync for CachedCursors {}

impl CachedCursors {
    fn load() -> Self {
        unsafe {
            Self {
                arrow: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ibeam: LoadCursorW(None, IDC_IBEAM).unwrap_or_default(),
                hand: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
                cross: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
                size_ns: LoadCursorW(None, IDC_SIZENS).unwrap_or_default(),
                size_we: LoadCursorW(None, IDC_SIZEWE).unwrap_or_default(),
                size_nesw: LoadCursorW(None, IDC_SIZENESW).unwrap_or_default(),
                size_nwse: LoadCursorW(None, IDC_SIZENWSE).unwrap_or_default(),
                size_all: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(),
                wait: LoadCursorW(None, IDC_WAIT).unwrap_or_default(),
                help: LoadCursorW(None, IDC_HELP).unwrap_or_default(),
                no: LoadCursorW(None, IDC_NO).unwrap_or_default(),
            }
        }
    }
}

/// 活跃显示器边界 (left, top, width, height) 像素
/// 默认为 (0, 0, 0, 0) 表示使用虚拟桌面全局坐标
struct ActiveMonitorBounds {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

pub struct WindowsInputInjector {
    cursors: CachedCursors,
    /// 当前捕获的显示器边界，用于坐标映射
    active_monitor: Mutex<ActiveMonitorBounds>,
}

impl Default for WindowsInputInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsInputInjector {
    pub fn new() -> Self {
        Self {
            cursors: CachedCursors::load(),
            active_monitor: Mutex::new(ActiveMonitorBounds {
                left: 0,
                top: 0,
                width: 0,
                height: 0,
            }),
        }
    }

    fn send_input(&self, input: INPUT) -> anyhow::Result<()> {
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            anyhow::bail!("SendInput 失败");
        }
        Ok(())
    }
}

impl InputInjector for WindowsInputInjector {
    fn set_active_monitor(&self, left: i32, top: i32, width: u32, height: u32) {
        let mut bounds = self
            .active_monitor
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        bounds.left = left;
        bounds.top = top;
        bounds.width = width;
        bounds.height = height;
    }

    fn move_mouse(&self, x: f64, y: f64) -> anyhow::Result<()> {
        // x,y 是相对于当前捕获显示器的归一化坐标 (0.0~1.0)
        // 需要先转换为虚拟桌面像素坐标，再转为 0~65535 绝对坐标
        let bounds = self
            .active_monitor
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe {
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN) as f64;
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN) as f64;
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN) as f64;
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN) as f64;

            let (px, py) = if bounds.width > 0 && bounds.height > 0 {
                // 从显示器归一化坐标 → 虚拟桌面像素坐标
                let pixel_x = bounds.left as f64 + x * bounds.width as f64;
                let pixel_y = bounds.top as f64 + y * bounds.height as f64;
                (pixel_x, pixel_y)
            } else {
                // 未设置显示器边界，回退到虚拟桌面全局归一化
                (vx + x * vw, vy + y * vh)
            };

            // 虚拟桌面像素 → 65535 绝对坐标
            let abs_x = if vw > 0.0 {
                ((px - vx) / vw * 65535.0) as i32
            } else {
                0
            };
            let abs_y = if vh > 0.0 {
                ((py - vy) / vh * 65535.0) as i32
            } else {
                0
            };

            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: abs_x,
                        dy: abs_y,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            drop(bounds);
            self.send_input(input)
        }
    }

    fn mouse_button(&self, button: MouseBtn, pressed: bool) -> anyhow::Result<()> {
        let flags = match (button, pressed) {
            (MouseBtn::Left, true) => MOUSEEVENTF_LEFTDOWN,
            (MouseBtn::Left, false) => MOUSEEVENTF_LEFTUP,
            (MouseBtn::Right, true) => MOUSEEVENTF_RIGHTDOWN,
            (MouseBtn::Right, false) => MOUSEEVENTF_RIGHTUP,
            (MouseBtn::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
            (MouseBtn::Middle, false) => MOUSEEVENTF_MIDDLEUP,
        };

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        self.send_input(input)
    }

    fn mouse_scroll(&self, _dx: f64, dy: f64) -> anyhow::Result<()> {
        let wheel_delta = (dy * 120.0) as i32;
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    // bit-reinterpret: i32 负值需要以 u32 位模式传递给 Windows API
                    mouseData: u32::from_ne_bytes(wheel_delta.to_ne_bytes()),
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        self.send_input(input)
    }

    fn key_event(&self, code: &str, pressed: bool, _modifiers: u8) -> anyhow::Result<()> {
        let (vk, code_is_extended) = code_to_vk(code);
        if vk == 0 {
            return Ok(()); // 未知键位，忽略
        }

        // 使用 MapVirtualKeyW 获取 scan code
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAP_VIRTUAL_KEY_TYPE(0)) as u16 };

        let mut flags = if pressed {
            KEYBD_EVENT_FLAGS(0)
        } else {
            KEYEVENTF_KEYUP
        };

        // 扩展键标记：通用扩展键列表 OR code_to_vk 返回的特定扩展标志
        let is_extended = code_is_extended
            || matches!(vk,
                0x21..=0x28 | 0x2C..=0x2E | 0x5B | 0x5C | 0x5D | // Page/Arrow/PrintScreen/Delete/Insert/Win/Menu
                0xA3 | 0xA5 | 0x6F // RightCtrl/RightAlt/NumpadDivide
            );
        if is_extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }

        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        self.send_input(input)
    }

    fn cursor_position(&self) -> (f64, f64) {
        let bounds = self
            .active_monitor
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe {
            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);

            if bounds.width > 0 && bounds.height > 0 {
                // 归一化到当前捕获的显示器坐标系
                let x = (point.x as f64 - bounds.left as f64) / bounds.width as f64;
                let y = (point.y as f64 - bounds.top as f64) / bounds.height as f64;
                (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
            } else {
                // 未设置显示器边界，回退到虚拟桌面全局归一化
                let vx = GetSystemMetrics(SM_XVIRTUALSCREEN) as f64;
                let vy = GetSystemMetrics(SM_YVIRTUALSCREEN) as f64;
                let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN) as f64;
                let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN) as f64;
                let x = if vw > 0.0 {
                    (point.x as f64 - vx) / vw
                } else {
                    0.0
                };
                let y = if vh > 0.0 {
                    (point.y as f64 - vy) / vh
                } else {
                    0.0
                };
                (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
            }
        }
    }

    fn cursor_shape(&self) -> CursorShape {
        unsafe {
            let mut info = CURSORINFO {
                cbSize: std::mem::size_of::<CURSORINFO>() as u32,
                ..Default::default()
            };
            if GetCursorInfo(&mut info).is_ok() {
                let cursor = info.hCursor;
                let c = &self.cursors;
                // 使用缓存的系统光标句柄进行对比，避免每帧调用 LoadCursorW
                if cursor == c.arrow {
                    CursorShape::Arrow
                } else if cursor == c.ibeam {
                    CursorShape::IBeam
                } else if cursor == c.hand {
                    CursorShape::Hand
                } else if cursor == c.cross {
                    CursorShape::Crosshair
                } else if cursor == c.size_ns {
                    CursorShape::ResizeNS
                } else if cursor == c.size_we {
                    CursorShape::ResizeEW
                } else if cursor == c.size_nesw {
                    CursorShape::ResizeNESW
                } else if cursor == c.size_nwse {
                    CursorShape::ResizeNWSE
                } else if cursor == c.size_all {
                    CursorShape::Move
                } else if cursor == c.wait {
                    CursorShape::Wait
                } else if cursor == c.help {
                    CursorShape::Help
                } else if cursor == c.no {
                    CursorShape::NotAllowed
                } else {
                    CursorShape::Arrow
                }
            } else {
                CursorShape::Arrow
            }
        }
    }

    fn send_special_key(&self, key: SpecialKeyType) -> anyhow::Result<()> {
        match key {
            SpecialKeyType::CtrlAltDel => {
                // 尝试通过 sas.dll 的 SendSAS 发送安全注意序列
                // 使用 raw FFI 避免额外 windows crate feature 依赖
                #[link(name = "kernel32")]
                extern "system" {
                    fn LoadLibraryW(name: *const u16) -> *mut std::ffi::c_void;
                    fn GetProcAddress(
                        module: *mut std::ffi::c_void,
                        name: *const u8,
                    ) -> *mut std::ffi::c_void;
                    fn FreeLibrary(module: *mut std::ffi::c_void) -> i32;
                }
                unsafe {
                    let dll_name: Vec<u16> = "sas.dll\0".encode_utf16().collect();
                    let lib = LoadLibraryW(dll_name.as_ptr());
                    if !lib.is_null() {
                        let proc = GetProcAddress(lib, c"SendSAS".as_ptr().cast());
                        if !proc.is_null() {
                            type SendSASFn = unsafe extern "system" fn(i32);
                            let send_sas: SendSASFn = std::mem::transmute(proc);
                            send_sas(0); // FALSE = 非交互式
                            FreeLibrary(lib);
                            return Ok(());
                        }
                        FreeLibrary(lib);
                    }
                }
                anyhow::bail!(
                    "SendSAS 不可用，需要以 SYSTEM 权限运行或配置注册表 SoftwareSASGeneration"
                )
            }
            SpecialKeyType::AltTab => {
                self.send_combo_keys(&[VIRTUAL_KEY(0xA4), VIRTUAL_KEY(0x09)]) // Alt + Tab
            }
            SpecialKeyType::AltF4 => {
                self.send_combo_keys(&[VIRTUAL_KEY(0xA4), VIRTUAL_KEY(0x73)]) // Alt + F4
            }
            SpecialKeyType::PrintScreen => {
                self.send_combo_keys(&[VIRTUAL_KEY(0x2C)]) // PrintScreen
            }
            SpecialKeyType::WinKey => {
                self.send_combo_keys(&[VIRTUAL_KEY(0x5B)]) // Left Win
            }
            SpecialKeyType::WinL => {
                // Win+L 是系统保护热键，SendInput 无法模拟
                // 直接调用 LockWorkStation API 锁屏
                #[link(name = "user32")]
                extern "system" {
                    fn LockWorkStation() -> i32;
                }
                let result = unsafe { LockWorkStation() };
                if result == 0 {
                    anyhow::bail!("LockWorkStation 失败")
                }
                Ok(())
            }
            SpecialKeyType::CtrlEsc => {
                self.send_combo_keys(&[VIRTUAL_KEY(0xA2), VIRTUAL_KEY(0x1B)]) // Ctrl + Esc
            }
        }
    }
}

impl WindowsInputInjector {
    /// 发送组合键：按顺序 key_down 所有键，再逆序 key_up
    fn send_combo_keys(&self, keys: &[VIRTUAL_KEY]) -> anyhow::Result<()> {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(keys.len() * 2);

        // key down
        for &vk in keys {
            let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAP_VIRTUAL_KEY_TYPE(0)) as u16 };
            let mut flags = KEYBD_EVENT_FLAGS(0);
            if is_extended_key(vk.0) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        // key up (reverse order)
        for &vk in keys.iter().rev() {
            let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAP_VIRTUAL_KEY_TYPE(0)) as u16 };
            let mut flags = KEYEVENTF_KEYUP;
            if is_extended_key(vk.0) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        let result = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            anyhow::bail!("SendInput 组合键失败");
        }
        Ok(())
    }
}

/// 判断虚拟键是否为扩展键
fn is_extended_key(vk: u16) -> bool {
    matches!(vk,
        0x21..=0x28 | 0x2C..=0x2E | 0x5B | 0x5C | 0x5D |
        0xA3 | 0xA5 | 0x6F
    )
}

/// KeyboardEvent.code → (Windows Virtual Key Code, is_extended)
/// 返回 (vk_code, is_extended) 元组，is_extended 用于区分如 NumpadEnter 和 Enter
fn code_to_vk(code: &str) -> (u16, bool) {
    match code {
        // 字母键
        "KeyA" => (0x41, false),
        "KeyB" => (0x42, false),
        "KeyC" => (0x43, false),
        "KeyD" => (0x44, false),
        "KeyE" => (0x45, false),
        "KeyF" => (0x46, false),
        "KeyG" => (0x47, false),
        "KeyH" => (0x48, false),
        "KeyI" => (0x49, false),
        "KeyJ" => (0x4A, false),
        "KeyK" => (0x4B, false),
        "KeyL" => (0x4C, false),
        "KeyM" => (0x4D, false),
        "KeyN" => (0x4E, false),
        "KeyO" => (0x4F, false),
        "KeyP" => (0x50, false),
        "KeyQ" => (0x51, false),
        "KeyR" => (0x52, false),
        "KeyS" => (0x53, false),
        "KeyT" => (0x54, false),
        "KeyU" => (0x55, false),
        "KeyV" => (0x56, false),
        "KeyW" => (0x57, false),
        "KeyX" => (0x58, false),
        "KeyY" => (0x59, false),
        "KeyZ" => (0x5A, false),
        // 数字键
        "Digit0" => (0x30, false),
        "Digit1" => (0x31, false),
        "Digit2" => (0x32, false),
        "Digit3" => (0x33, false),
        "Digit4" => (0x34, false),
        "Digit5" => (0x35, false),
        "Digit6" => (0x36, false),
        "Digit7" => (0x37, false),
        "Digit8" => (0x38, false),
        "Digit9" => (0x39, false),
        // 功能键
        "F1" => (0x70, false),
        "F2" => (0x71, false),
        "F3" => (0x72, false),
        "F4" => (0x73, false),
        "F5" => (0x74, false),
        "F6" => (0x75, false),
        "F7" => (0x76, false),
        "F8" => (0x77, false),
        "F9" => (0x78, false),
        "F10" => (0x79, false),
        "F11" => (0x7A, false),
        "F12" => (0x7B, false),
        // 控制键
        "Enter" => (0x0D, false),
        "Escape" => (0x1B, false),
        "Backspace" => (0x08, false),
        "Tab" => (0x09, false),
        "Space" => (0x20, false),
        "Delete" => (0x2E, false),
        "Insert" => (0x2D, false),
        "Home" => (0x24, false),
        "End" => (0x23, false),
        "PageUp" => (0x21, false),
        "PageDown" => (0x22, false),
        // 方向键
        "ArrowUp" => (0x26, false),
        "ArrowDown" => (0x28, false),
        "ArrowLeft" => (0x25, false),
        "ArrowRight" => (0x27, false),
        // 修饰键
        "ShiftLeft" => (0xA0, false),
        "ShiftRight" => (0xA1, false),
        "ControlLeft" => (0xA2, false),
        "ControlRight" => (0xA3, false),
        "AltLeft" => (0xA4, false),
        "AltRight" => (0xA5, false),
        "MetaLeft" => (0x5B, false),
        "MetaRight" => (0x5C, false),
        "CapsLock" => (0x14, false),
        "NumLock" => (0x90, false),
        "ScrollLock" => (0x91, false),
        // 符号键
        "Minus" => (0xBD, false),
        "Equal" => (0xBB, false),
        "BracketLeft" => (0xDB, false),
        "BracketRight" => (0xDD, false),
        "Backslash" => (0xDC, false),
        "Semicolon" => (0xBA, false),
        "Quote" => (0xDE, false),
        "Backquote" => (0xC0, false),
        "Comma" => (0xBC, false),
        "Period" => (0xBE, false),
        "Slash" => (0xBF, false),
        // 小键盘
        "Numpad0" => (0x60, false),
        "Numpad1" => (0x61, false),
        "Numpad2" => (0x62, false),
        "Numpad3" => (0x63, false),
        "Numpad4" => (0x64, false),
        "Numpad5" => (0x65, false),
        "Numpad6" => (0x66, false),
        "Numpad7" => (0x67, false),
        "Numpad8" => (0x68, false),
        "Numpad9" => (0x69, false),
        "NumpadAdd" => (0x6B, false),
        "NumpadSubtract" => (0x6D, false),
        "NumpadMultiply" => (0x6A, false),
        "NumpadDivide" => (0x6F, false),
        "NumpadDecimal" => (0x6E, false),
        "NumpadEnter" => (0x0D, true), // NumpadEnter 是扩展键，与主键盘 Enter 区分
        // 其他
        "PrintScreen" => (0x2C, false),
        "Pause" => (0x13, false),
        "ContextMenu" => (0x5D, false),
        _ => (0, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_letter_keys() {
        assert_eq!(code_to_vk("KeyA").0, 0x41);
        assert_eq!(code_to_vk("KeyZ").0, 0x5A);
        assert_eq!(code_to_vk("KeyM").0, 0x4D);
    }

    #[test]
    fn test_digit_keys() {
        assert_eq!(code_to_vk("Digit0").0, 0x30);
        assert_eq!(code_to_vk("Digit9").0, 0x39);
    }

    #[test]
    fn test_function_keys() {
        assert_eq!(code_to_vk("F1").0, 0x70);
        assert_eq!(code_to_vk("F12").0, 0x7B);
    }

    #[test]
    fn test_control_keys() {
        assert_eq!(code_to_vk("Enter"), (0x0D, false));
        assert_eq!(code_to_vk("Escape").0, 0x1B);
        assert_eq!(code_to_vk("Space").0, 0x20);
        assert_eq!(code_to_vk("Tab").0, 0x09);
        assert_eq!(code_to_vk("Backspace").0, 0x08);
    }

    #[test]
    fn test_arrow_keys() {
        assert_eq!(code_to_vk("ArrowUp").0, 0x26);
        assert_eq!(code_to_vk("ArrowDown").0, 0x28);
        assert_eq!(code_to_vk("ArrowLeft").0, 0x25);
        assert_eq!(code_to_vk("ArrowRight").0, 0x27);
    }

    #[test]
    fn test_modifier_keys() {
        assert_eq!(code_to_vk("ShiftLeft").0, 0xA0);
        assert_eq!(code_to_vk("ControlLeft").0, 0xA2);
        assert_eq!(code_to_vk("AltLeft").0, 0xA4);
        assert_eq!(code_to_vk("MetaLeft").0, 0x5B);
    }

    #[test]
    fn test_unknown_key() {
        assert_eq!(code_to_vk("NonExistentKey").0, 0);
        assert_eq!(code_to_vk("").0, 0);
    }

    #[test]
    fn test_numpad_keys() {
        assert_eq!(code_to_vk("Numpad0").0, 0x60);
        assert_eq!(code_to_vk("Numpad9").0, 0x69);
        assert_eq!(code_to_vk("NumpadAdd").0, 0x6B);
    }

    #[test]
    fn test_numpad_enter_is_extended() {
        let (vk, extended) = code_to_vk("NumpadEnter");
        assert_eq!(vk, 0x0D);
        assert!(extended, "NumpadEnter 应该被标记为扩展键");
        let (vk2, extended2) = code_to_vk("Enter");
        assert_eq!(vk2, 0x0D);
        assert!(!extended2, "Enter 不应该被标记为扩展键");
    }

    #[test]
    fn test_set_active_monitor() {
        use crate::InputInjector;
        let injector = WindowsInputInjector::new();
        // 默认 bounds 宽高为 0
        {
            let b = injector.active_monitor.lock().unwrap();
            assert_eq!(b.width, 0);
            assert_eq!(b.height, 0);
        }
        // 设置活跃显示器
        injector.set_active_monitor(1920, 0, 2560, 1440);
        {
            let b = injector.active_monitor.lock().unwrap();
            assert_eq!(b.left, 1920);
            assert_eq!(b.top, 0);
            assert_eq!(b.width, 2560);
            assert_eq!(b.height, 1440);
        }
    }

    #[test]
    fn test_set_active_monitor_negative_offset() {
        use crate::InputInjector;
        let injector = WindowsInputInjector::new();
        injector.set_active_monitor(-1920, -200, 1920, 1080);
        let b = injector.active_monitor.lock().unwrap();
        assert_eq!(b.left, -1920);
        assert_eq!(b.top, -200);
    }
}
