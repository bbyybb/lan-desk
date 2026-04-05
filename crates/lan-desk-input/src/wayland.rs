/// Wayland 输入注入
///
/// 支持三种模式（运行时自动选择）：
/// 1. ydotool 模式：通过 ydotool + ydotoold 守护进程（使用 uinput 内核接口）
///    - 适用于所有 Wayland 合成器
///    - 需要 ydotoold 守护进程运行且具有 /dev/uinput 访问权限
/// 2. dotool 模式：通过 dotool 工具（不需要 root 权限）
///    - 适用于所有 Wayland 合成器
///    - 支持鼠标和键盘，通过 stdin 管道接收命令
///    - 使用长驻子进程避免频繁 fork
/// 3. wtype 模式：通过 wtype 工具（使用 wlr-virtual-keyboard 协议）
///    - 仅适用于 wlroots 合成器（Sway、Hyprland 等）
///    - wtype 仅支持键盘输入，鼠标操作仍需 ydotool
///
/// 工具检测回退链：ydotool > dotool > wtype
///
/// Wayland 不允许应用直接注入输入到其他窗口，
/// 必须通过特权工具或 compositor 扩展协议。
use crate::InputInjector;
use lan_desk_protocol::message::{CursorShape, MouseBtn};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tracing::{debug, info, warn};

/// 输入注入方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMethod {
    /// ydotool（需要 ydotoold 守护进程运行）
    Ydotool,
    /// dotool（不需要 root 权限，通过 stdin 管道接收命令）
    Dotool,
    /// wtype 用于键盘 + ydotool 用于鼠标（混合模式）
    WtypeWithYdotool,
    /// 仅 wtype（只能注入键盘事件，鼠标功能受限）
    WtypeOnly,
}

/// 活跃显示器边界
struct ActiveMonitorBounds {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

/// Wayland 输入注入器
pub struct WaylandInputInjector {
    /// 选定的输入注入方式
    method: InputMethod,
    /// 屏幕宽度（像素）
    _screen_width: u32,
    /// 屏幕高度（像素）
    _screen_height: u32,
    /// 缓存的鼠标位置（归一化 0.0~1.0），因为 Wayland 无法直接查询
    cached_cursor_pos: Mutex<(f64, f64)>,
    /// dotool 长驻子进程（通过 stdin 管道发送命令，避免频繁 fork）
    dotool_process: Mutex<Option<Child>>,
    /// 当前捕获的显示器边界
    active_monitor: Mutex<ActiveMonitorBounds>,
}

// SAFETY: 注入器在 session 线程中使用，Mutex 保护共享状态
unsafe impl Send for WaylandInputInjector {}
unsafe impl Sync for WaylandInputInjector {}

impl WaylandInputInjector {
    /// 创建 Wayland 输入注入器
    ///
    /// 按优先级检测可用工具：ydotool > wtype+ydotool > wtype
    pub fn new() -> anyhow::Result<Self> {
        // 检测是否在 Wayland 环境
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            anyhow::bail!("未检测到 Wayland 显示服务器（WAYLAND_DISPLAY 未设置）");
        }

        let method = detect_input_method()?;
        info!("Wayland 输入注入方式: {:?}", method);

        // 获取屏幕分辨率
        let (screen_width, screen_height) = get_screen_resolution().unwrap_or_else(|e| {
            warn!("无法获取屏幕分辨率，使用默认值 1920x1080: {}", e);
            (1920, 1080)
        });

        // 如果选用 dotool，启动长驻子进程
        let dotool_process = if method == InputMethod::Dotool {
            match spawn_dotool_process() {
                Ok(child) => {
                    info!("dotool 长驻子进程已启动 (pid: {})", child.id());
                    Mutex::new(Some(child))
                }
                Err(e) => {
                    warn!("启动 dotool 子进程失败: {}，将在每次调用时单独启动", e);
                    Mutex::new(None)
                }
            }
        } else {
            Mutex::new(None)
        };

        info!(
            "Wayland 输入注入已初始化: {}x{}, 方式={:?}",
            screen_width, screen_height, method
        );

        Ok(Self {
            method,
            _screen_width: screen_width,
            _screen_height: screen_height,
            cached_cursor_pos: Mutex::new((0.5, 0.5)),
            dotool_process,
            active_monitor: Mutex::new(ActiveMonitorBounds {
                left: 0,
                top: 0,
                width: screen_width,
                height: screen_height,
            }),
        })
    }
}

impl InputInjector for WaylandInputInjector {
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
        let bounds = self
            .active_monitor
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let abs_x = bounds.left + (x * bounds.width as f64) as i32;
        let abs_y = bounds.top + (y * bounds.height as f64) as i32;
        drop(bounds);

        match self.method {
            InputMethod::Ydotool | InputMethod::WtypeWithYdotool => {
                // ydotool mousemove --absolute -x X -y Y
                run_tool(
                    "ydotool",
                    &[
                        "mousemove",
                        "--absolute",
                        "-x",
                        &abs_x.to_string(),
                        "-y",
                        &abs_y.to_string(),
                    ],
                )?;
            }
            InputMethod::Dotool => {
                let cmd = format!("mousemove {} {}", abs_x, abs_y);
                self.send_dotool_command(&cmd)?;
            }
            InputMethod::WtypeOnly => {
                // wtype 不支持鼠标移动
                debug!("wtype 模式不支持鼠标移动，忽略 move_mouse({}, {})", x, y);
                // 仅更新缓存位置
            }
        }

        // 更新缓存的鼠标位置
        if let Ok(mut pos) = self.cached_cursor_pos.lock() {
            *pos = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        }

        Ok(())
    }

    fn mouse_button(&self, button: MouseBtn, pressed: bool) -> anyhow::Result<()> {
        match self.method {
            InputMethod::Ydotool | InputMethod::WtypeWithYdotool => {
                // ydotool 的按钮编码：
                // 左键=0x110 (BTN_LEFT), 右键=0x111 (BTN_RIGHT), 中键=0x112 (BTN_MIDDLE)
                let btn_code = match button {
                    MouseBtn::Left => "0x110",
                    MouseBtn::Right => "0x111",
                    MouseBtn::Middle => "0x112",
                };

                // ydotool click --button BTN --next-delay 0
                // 对于按下/释放分开的情况，使用 mousedown/mouseup
                if pressed {
                    run_tool("ydotool", &["mousedown", btn_code])?;
                } else {
                    run_tool("ydotool", &["mouseup", btn_code])?;
                }
            }
            InputMethod::Dotool => {
                // dotool 按钮编号：1=左键, 2=中键, 3=右键
                let btn_num = match button {
                    MouseBtn::Left => 1,
                    MouseBtn::Right => 3,
                    MouseBtn::Middle => 2,
                };
                let action = if pressed { "buttondown" } else { "buttonup" };
                let cmd = format!("{} {}", action, btn_num);
                self.send_dotool_command(&cmd)?;
            }
            InputMethod::WtypeOnly => {
                debug!("wtype 模式不支持鼠标按键，忽略");
            }
        }
        Ok(())
    }

    fn mouse_scroll(&self, _dx: f64, dy: f64) -> anyhow::Result<()> {
        match self.method {
            InputMethod::Ydotool | InputMethod::WtypeWithYdotool => {
                // ydotool mousemove --wheel: 正值向上，负值向下
                // dy > 0 表示向上滚动
                let steps = (dy * 3.0) as i32; // 放大滚动步数
                if steps == 0 {
                    return Ok(());
                }

                // ydotool 新版本使用 mousemove --wheel 来滚动
                run_tool("ydotool", &["mousemove", "--wheel", &(-steps).to_string()])?;
            }
            InputMethod::Dotool => {
                let steps = (dy * 3.0) as i32;
                if steps == 0 {
                    return Ok(());
                }
                // dotool: wheel 正值向上，负值向下
                let cmd = format!("wheel {}", steps);
                self.send_dotool_command(&cmd)?;
            }
            InputMethod::WtypeOnly => {
                debug!("wtype 模式不支持鼠标滚轮，忽略");
            }
        }
        Ok(())
    }

    fn key_event(&self, code: &str, pressed: bool, _modifiers: u8) -> anyhow::Result<()> {
        let evdev_code = web_code_to_evdev(code);
        if evdev_code == 0 {
            debug!("未映射的键位: {}", code);
            return Ok(());
        }

        match self.method {
            InputMethod::Ydotool | InputMethod::WtypeWithYdotool => {
                // ydotool key <keycode>:<state>
                // state: 1 = 按下, 0 = 释放
                let state = if pressed { 1 } else { 0 };
                let key_spec = format!("{}:{}", evdev_code, state);
                run_tool("ydotool", &["key", &key_spec])?;
            }
            InputMethod::Dotool => {
                // dotool 使用 X11 键名（如 "a", "Return", "shift" 等）
                if let Some(key_name) = evdev_to_dotool_keyname(evdev_code) {
                    let action = if pressed { "keydown" } else { "keyup" };
                    let cmd = format!("{} {}", action, key_name);
                    self.send_dotool_command(&cmd)?;
                } else {
                    debug!(
                        "dotool: 未映射的 evdev 键码: {} (web code: {})",
                        evdev_code, code
                    );
                }
            }
            InputMethod::WtypeOnly => {
                // wtype 只支持输入字符串，不支持单独的按下/释放事件
                // 对于修饰键等无法处理
                if pressed {
                    if let Some(ch) = evdev_to_char(evdev_code) {
                        run_tool("wtype", &[&ch.to_string()])?;
                    }
                }
            }
        }

        Ok(())
    }

    fn cursor_position(&self) -> (f64, f64) {
        // Wayland 不提供直接查询其他窗口光标位置的接口
        // 返回缓存的最后已知位置
        self.cached_cursor_pos
            .lock()
            .map(|pos| *pos)
            .unwrap_or((0.0, 0.0))
    }

    fn cursor_shape(&self) -> CursorShape {
        // Wayland 不允许应用读取其他窗口的光标形状
        CursorShape::Arrow
    }
}

// =============================================================================
// 内部实现函数
// =============================================================================

/// 检测可用的输入注入方式
fn detect_input_method() -> anyhow::Result<InputMethod> {
    let has_ydotool = is_tool_available("ydotool");
    let has_dotool = is_tool_available("dotool");
    let has_wtype = is_tool_available("wtype");

    // 优先 ydotool（最完善，支持鼠标和键盘）
    if has_ydotool {
        // 检测 ydotoold 是否在运行
        if is_ydotoold_running() {
            return Ok(InputMethod::Ydotool);
        }
        warn!(
            "ydotool 已安装但 ydotoold 守护进程未运行。\n\
             请运行: sudo ydotoold &\n\
             或通过 systemd: systemctl --user enable --now ydotool"
        );
    }

    // 次选 dotool（不需要 root 权限，支持鼠标和键盘）
    if has_dotool {
        info!("使用 dotool 进行输入注入（不需要 root 权限）");
        return Ok(InputMethod::Dotool);
    }

    if has_wtype && has_ydotool {
        // ydotool 可用但守护进程未运行，wtype 可处理键盘
        warn!("使用 wtype+ydotool 混合模式（鼠标功能可能受限）");
        return Ok(InputMethod::WtypeWithYdotool);
    }

    if has_wtype {
        warn!(
            "仅有 wtype 可用，鼠标功能将不可用。\n\
             建议安装 ydotool 或 dotool 以获得完整功能。"
        );
        return Ok(InputMethod::WtypeOnly);
    }

    anyhow::bail!(
        "未找到可用的 Wayland 输入注入工具。请安装以下工具之一：\n\
         - ydotool + ydotoold（推荐，支持鼠标和键盘）\n\
         - dotool（不需要 root 权限，支持鼠标和键盘）\n\
         - wtype（仅键盘，适用于 wlroots 合成器）\n\n\
         在 Arch Linux 上: pacman -S ydotool 或 pacman -S dotool\n\
         在 Ubuntu/Debian 上: apt install ydotool\n\
         在 Fedora 上: dnf install ydotool\n\n\
         ydotool 安装后启动守护进程: sudo ydotoold &\n\
         或 systemctl --user enable --now ydotool"
    )
}

/// 检测命令行工具是否可用
fn is_tool_available(tool: &str) -> bool {
    Command::new("which")
        .arg(tool)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 检测 ydotoold 守护进程是否在运行
fn is_ydotoold_running() -> bool {
    // 方法 1: 检查 ydotools socket 文件
    let socket_path = format!(
        "/run/user/{}/ydotool_socket",
        std::env::var("UID")
            .or_else(|_| std::env::var("EUID"))
            .unwrap_or_default()
    );
    if std::path::Path::new(&socket_path).exists() {
        return true;
    }

    // 方法 2: 检查默认 socket 路径
    if std::path::Path::new("/tmp/.ydotool_socket").exists() {
        return true;
    }

    // 方法 3: 使用 pgrep 检查进程
    Command::new("pgrep")
        .arg("ydotoold")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 获取屏幕分辨率
fn get_screen_resolution() -> anyhow::Result<(u32, u32)> {
    // 尝试 wlr-randr
    if let Ok(output) = Command::new("wlr-randr").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // 查找第一个 "current" 模式行
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.contains("current") && trimmed.contains("px") {
                    if let Some((w, h)) = parse_resolution(trimmed) {
                        return Ok((w, h));
                    }
                }
            }
        }
    }

    // 尝试 swaymsg
    if let Ok(output) = Command::new("swaymsg")
        .args(["-t", "get_outputs", "--raw"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // 简单查找 "width" 和 "height"
            if let (Some(w), Some(h)) = (
                extract_first_number(&stdout, "width"),
                extract_first_number(&stdout, "height"),
            ) {
                return Ok((w, h));
            }
        }
    }

    anyhow::bail!("无法获取 Wayland 屏幕分辨率")
}

/// 从文本中解析分辨率 "WxH"
fn parse_resolution(text: &str) -> Option<(u32, u32)> {
    for part in text.split_whitespace() {
        if let Some((w_str, h_str)) = part.split_once('x') {
            if let (Ok(w), Ok(h)) = (w_str.parse::<u32>(), h_str.parse::<u32>()) {
                if w > 0 && h > 0 {
                    return Some((w, h));
                }
            }
        }
    }
    None
}

/// 从文本中提取指定键后的第一个数字
fn extract_first_number(text: &str, key: &str) -> Option<u32> {
    let pattern = format!("\"{}\"", key);
    let key_pos = text.find(&pattern)?;
    let after = &text[key_pos + pattern.len()..];
    let colon_pos = after.find(':')?;
    let after_colon = after[colon_pos + 1..].trim_start();
    let num_str: String = after_colon
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse::<u32>().ok()
}

/// 执行工具命令
fn run_tool(tool: &str, args: &[&str]) -> anyhow::Result<()> {
    debug!("执行: {} {}", tool, args.join(" "));

    let output = Command::new(tool)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| anyhow::anyhow!("执行 {} 失败: {}", tool, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            warn!("{} 返回错误: {}", tool, stderr.trim());
        }
    }

    Ok(())
}

/// 启动 dotool 长驻子进程
///
/// dotool 通过 stdin 管道接收命令，每行一条命令。
/// 保持一个长期运行的子进程可以避免频繁 fork 的开销。
fn spawn_dotool_process() -> anyhow::Result<Child> {
    let child = Command::new("dotool")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动 dotool 失败: {}", e))?;
    Ok(child)
}

impl Drop for WaylandInputInjector {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.dotool_process.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

impl WaylandInputInjector {
    /// 向 dotool 子进程发送命令
    ///
    /// 如果长驻子进程可用，通过 stdin 管道发送；
    /// 否则回退为每次调用单独启动 dotool 进程。
    fn send_dotool_command(&self, command: &str) -> anyhow::Result<()> {
        debug!("dotool 命令: {}", command);

        let mut guard = self
            .dotool_process
            .lock()
            .map_err(|e| anyhow::anyhow!("获取 dotool 进程锁失败: {}", e))?;

        // 尝试通过长驻子进程发送
        let need_respawn;
        if let Some(ref mut child) = *guard {
            if let Some(ref mut stdin) = child.stdin {
                match writeln!(stdin, "{}", command) {
                    Ok(()) => {
                        if stdin.flush().is_ok() {
                            return Ok(());
                        }
                        need_respawn = true;
                    }
                    Err(_) => {
                        need_respawn = true;
                    }
                }
            } else {
                need_respawn = true;
            }
        } else {
            need_respawn = true;
        }

        // 长驻进程不可用，尝试重新启动
        if need_respawn {
            match spawn_dotool_process() {
                Ok(mut new_child) => {
                    if let Some(ref mut stdin) = new_child.stdin {
                        writeln!(stdin, "{}", command)
                            .map_err(|e| anyhow::anyhow!("向 dotool 写入命令失败: {}", e))?;
                        stdin
                            .flush()
                            .map_err(|e| anyhow::anyhow!("刷新 dotool stdin 失败: {}", e))?;
                    }
                    *guard = Some(new_child);
                    return Ok(());
                }
                Err(e) => {
                    *guard = None;
                    // 最终回退：每次调用单独启动进程
                    return run_dotool_oneshot(command).map_err(|e2| {
                        anyhow::anyhow!("dotool 执行失败 (重启失败: {}): {}", e, e2)
                    });
                }
            }
        }

        Ok(())
    }
}

/// 单次调用 dotool（回退方案，每次 fork 新进程）
fn run_dotool_oneshot(command: &str) -> anyhow::Result<()> {
    let mut child = Command::new("dotool")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动 dotool 失败: {}", e))?;

    if let Some(ref mut stdin) = child.stdin {
        writeln!(stdin, "{}", command)
            .map_err(|e| anyhow::anyhow!("向 dotool 写入命令失败: {}", e))?;
    }
    // 关闭 stdin 以让 dotool 处理并退出
    drop(child.stdin.take());
    let _ = child.wait();
    Ok(())
}

/// 将 evdev 键码映射到 dotool 使用的 XKB 键名
///
/// dotool 使用 XKB 键名（类似 xdotool），如 "a", "Return", "shift" 等。
fn evdev_to_dotool_keyname(code: u32) -> Option<&'static str> {
    match code {
        // 字母键
        30 => Some("a"),
        48 => Some("b"),
        46 => Some("c"),
        32 => Some("d"),
        18 => Some("e"),
        33 => Some("f"),
        34 => Some("g"),
        35 => Some("h"),
        23 => Some("i"),
        36 => Some("j"),
        37 => Some("k"),
        38 => Some("l"),
        50 => Some("m"),
        49 => Some("n"),
        24 => Some("o"),
        25 => Some("p"),
        16 => Some("q"),
        19 => Some("r"),
        31 => Some("s"),
        20 => Some("t"),
        22 => Some("u"),
        47 => Some("v"),
        17 => Some("w"),
        45 => Some("x"),
        21 => Some("y"),
        44 => Some("z"),

        // 数字键
        2 => Some("1"),
        3 => Some("2"),
        4 => Some("3"),
        5 => Some("4"),
        6 => Some("5"),
        7 => Some("6"),
        8 => Some("7"),
        9 => Some("8"),
        10 => Some("9"),
        11 => Some("0"),

        // 功能键
        59 => Some("F1"),
        60 => Some("F2"),
        61 => Some("F3"),
        62 => Some("F4"),
        63 => Some("F5"),
        64 => Some("F6"),
        65 => Some("F7"),
        66 => Some("F8"),
        67 => Some("F9"),
        68 => Some("F10"),
        87 => Some("F11"),
        88 => Some("F12"),

        // 修饰键
        42 => Some("shift"),
        54 => Some("shift"),
        29 => Some("ctrl"),
        97 => Some("ctrl"),
        56 => Some("alt"),
        100 => Some("alt"),
        125 => Some("super"),
        126 => Some("super"),
        58 => Some("Caps_Lock"),
        69 => Some("Num_Lock"),
        70 => Some("Scroll_Lock"),

        // 控制键
        28 => Some("Return"),
        15 => Some("Tab"),
        57 => Some("space"),
        14 => Some("BackSpace"),
        1 => Some("Escape"),
        111 => Some("Delete"),
        110 => Some("Insert"),
        102 => Some("Home"),
        107 => Some("End"),
        104 => Some("Page_Up"),
        109 => Some("Page_Down"),
        99 => Some("Print"),
        119 => Some("Pause"),

        // 方向键
        103 => Some("Up"),
        108 => Some("Down"),
        105 => Some("Left"),
        106 => Some("Right"),

        // 符号键
        12 => Some("minus"),
        13 => Some("equal"),
        26 => Some("bracketleft"),
        27 => Some("bracketright"),
        43 => Some("backslash"),
        39 => Some("semicolon"),
        40 => Some("apostrophe"),
        41 => Some("grave"),
        51 => Some("comma"),
        52 => Some("period"),
        53 => Some("slash"),

        // 小键盘
        82 => Some("KP_0"),
        79 => Some("KP_1"),
        80 => Some("KP_2"),
        81 => Some("KP_3"),
        75 => Some("KP_4"),
        76 => Some("KP_5"),
        77 => Some("KP_6"),
        71 => Some("KP_7"),
        72 => Some("KP_8"),
        73 => Some("KP_9"),
        78 => Some("KP_Add"),
        74 => Some("KP_Subtract"),
        55 => Some("KP_Multiply"),
        98 => Some("KP_Divide"),
        83 => Some("KP_Decimal"),
        96 => Some("KP_Enter"),

        _ => None,
    }
}

/// 将 web KeyboardEvent.code 映射到 Linux evdev 键码
///
/// 参考: linux/input-event-codes.h
pub fn web_code_to_evdev(code: &str) -> u32 {
    match code {
        // 字母键 (KEY_A=30 .. KEY_Z)
        "KeyA" => 30,
        "KeyB" => 48,
        "KeyC" => 46,
        "KeyD" => 32,
        "KeyE" => 18,
        "KeyF" => 33,
        "KeyG" => 34,
        "KeyH" => 35,
        "KeyI" => 23,
        "KeyJ" => 36,
        "KeyK" => 37,
        "KeyL" => 38,
        "KeyM" => 50,
        "KeyN" => 49,
        "KeyO" => 24,
        "KeyP" => 25,
        "KeyQ" => 16,
        "KeyR" => 19,
        "KeyS" => 31,
        "KeyT" => 20,
        "KeyU" => 22,
        "KeyV" => 47,
        "KeyW" => 17,
        "KeyX" => 45,
        "KeyY" => 21,
        "KeyZ" => 44,

        // 数字键
        "Digit1" => 2,
        "Digit2" => 3,
        "Digit3" => 4,
        "Digit4" => 5,
        "Digit5" => 6,
        "Digit6" => 7,
        "Digit7" => 8,
        "Digit8" => 9,
        "Digit9" => 10,
        "Digit0" => 11,

        // 功能键
        "F1" => 59,
        "F2" => 60,
        "F3" => 61,
        "F4" => 62,
        "F5" => 63,
        "F6" => 64,
        "F7" => 65,
        "F8" => 66,
        "F9" => 67,
        "F10" => 68,
        "F11" => 87,
        "F12" => 88,

        // 修饰键
        "ShiftLeft" => 42,    // KEY_LEFTSHIFT
        "ShiftRight" => 54,   // KEY_RIGHTSHIFT
        "ControlLeft" => 29,  // KEY_LEFTCTRL
        "ControlRight" => 97, // KEY_RIGHTCTRL
        "AltLeft" => 56,      // KEY_LEFTALT
        "AltRight" => 100,    // KEY_RIGHTALT
        "MetaLeft" => 125,    // KEY_LEFTMETA
        "MetaRight" => 126,   // KEY_RIGHTMETA
        "CapsLock" => 58,     // KEY_CAPSLOCK
        "NumLock" => 69,      // KEY_NUMLOCK
        "ScrollLock" => 70,   // KEY_SCROLLLOCK

        // 控制键
        "Enter" => 28,       // KEY_ENTER
        "Tab" => 15,         // KEY_TAB
        "Space" => 57,       // KEY_SPACE
        "Backspace" => 14,   // KEY_BACKSPACE
        "Escape" => 1,       // KEY_ESC
        "Delete" => 111,     // KEY_DELETE
        "Insert" => 110,     // KEY_INSERT
        "Home" => 102,       // KEY_HOME
        "End" => 107,        // KEY_END
        "PageUp" => 104,     // KEY_PAGEUP
        "PageDown" => 109,   // KEY_PAGEDOWN
        "PrintScreen" => 99, // KEY_SYSRQ
        "Pause" => 119,      // KEY_PAUSE

        // 方向键
        "ArrowUp" => 103,    // KEY_UP
        "ArrowDown" => 108,  // KEY_DOWN
        "ArrowLeft" => 105,  // KEY_LEFT
        "ArrowRight" => 106, // KEY_RIGHT

        // 符号键
        "Minus" => 12,        // KEY_MINUS
        "Equal" => 13,        // KEY_EQUAL
        "BracketLeft" => 26,  // KEY_LEFTBRACE
        "BracketRight" => 27, // KEY_RIGHTBRACE
        "Backslash" => 43,    // KEY_BACKSLASH
        "Semicolon" => 39,    // KEY_SEMICOLON
        "Quote" => 40,        // KEY_APOSTROPHE
        "Backquote" => 41,    // KEY_GRAVE
        "Comma" => 51,        // KEY_COMMA
        "Period" => 52,       // KEY_DOT
        "Slash" => 53,        // KEY_SLASH

        // 小键盘
        "Numpad0" => 82,        // KEY_KP0
        "Numpad1" => 79,        // KEY_KP1
        "Numpad2" => 80,        // KEY_KP2
        "Numpad3" => 81,        // KEY_KP3
        "Numpad4" => 75,        // KEY_KP4
        "Numpad5" => 76,        // KEY_KP5
        "Numpad6" => 77,        // KEY_KP6
        "Numpad7" => 71,        // KEY_KP7
        "Numpad8" => 72,        // KEY_KP8
        "Numpad9" => 73,        // KEY_KP9
        "NumpadAdd" => 78,      // KEY_KPPLUS
        "NumpadSubtract" => 74, // KEY_KPMINUS
        "NumpadMultiply" => 55, // KEY_KPASTERISK
        "NumpadDivide" => 98,   // KEY_KPSLASH
        "NumpadDecimal" => 83,  // KEY_KPDOT
        "NumpadEnter" => 96,    // KEY_KPENTER

        _ => 0,
    }
}

/// evdev 键码转可打印字符（仅用于 wtype 后备模式）
fn evdev_to_char(code: u32) -> Option<char> {
    match code {
        // 字母键
        30 => Some('a'),
        48 => Some('b'),
        46 => Some('c'),
        32 => Some('d'),
        18 => Some('e'),
        33 => Some('f'),
        34 => Some('g'),
        35 => Some('h'),
        23 => Some('i'),
        36 => Some('j'),
        37 => Some('k'),
        38 => Some('l'),
        50 => Some('m'),
        49 => Some('n'),
        24 => Some('o'),
        25 => Some('p'),
        16 => Some('q'),
        19 => Some('r'),
        31 => Some('s'),
        20 => Some('t'),
        22 => Some('u'),
        47 => Some('v'),
        17 => Some('w'),
        45 => Some('x'),
        21 => Some('y'),
        44 => Some('z'),
        // 数字键
        2 => Some('1'),
        3 => Some('2'),
        4 => Some('3'),
        5 => Some('4'),
        6 => Some('5'),
        7 => Some('6'),
        8 => Some('7'),
        9 => Some('8'),
        10 => Some('9'),
        11 => Some('0'),
        // 常用符号
        57 => Some(' '),  // Space
        12 => Some('-'),  // Minus
        13 => Some('='),  // Equal
        26 => Some('['),  // BracketLeft
        27 => Some(']'),  // BracketRight
        43 => Some('\\'), // Backslash
        39 => Some(';'),  // Semicolon
        40 => Some('\''), // Quote
        41 => Some('`'),  // Backquote
        51 => Some(','),  // Comma
        52 => Some('.'),  // Period
        53 => Some('/'),  // Slash
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_code_to_evdev_letters() {
        assert_eq!(web_code_to_evdev("KeyA"), 30);
        assert_eq!(web_code_to_evdev("KeyZ"), 44);
        assert_eq!(web_code_to_evdev("KeyM"), 50);
    }

    #[test]
    fn test_web_code_to_evdev_digits() {
        assert_eq!(web_code_to_evdev("Digit0"), 11);
        assert_eq!(web_code_to_evdev("Digit1"), 2);
        assert_eq!(web_code_to_evdev("Digit9"), 10);
    }

    #[test]
    fn test_web_code_to_evdev_function_keys() {
        assert_eq!(web_code_to_evdev("F1"), 59);
        assert_eq!(web_code_to_evdev("F12"), 88);
    }

    #[test]
    fn test_web_code_to_evdev_modifiers() {
        assert_eq!(web_code_to_evdev("ShiftLeft"), 42);
        assert_eq!(web_code_to_evdev("ControlLeft"), 29);
        assert_eq!(web_code_to_evdev("AltLeft"), 56);
        assert_eq!(web_code_to_evdev("MetaLeft"), 125);
    }

    #[test]
    fn test_web_code_to_evdev_control_keys() {
        assert_eq!(web_code_to_evdev("Enter"), 28);
        assert_eq!(web_code_to_evdev("Space"), 57);
        assert_eq!(web_code_to_evdev("Escape"), 1);
        assert_eq!(web_code_to_evdev("Backspace"), 14);
        assert_eq!(web_code_to_evdev("Tab"), 15);
    }

    #[test]
    fn test_web_code_to_evdev_arrows() {
        assert_eq!(web_code_to_evdev("ArrowUp"), 103);
        assert_eq!(web_code_to_evdev("ArrowDown"), 108);
        assert_eq!(web_code_to_evdev("ArrowLeft"), 105);
        assert_eq!(web_code_to_evdev("ArrowRight"), 106);
    }

    #[test]
    fn test_web_code_to_evdev_unknown() {
        assert_eq!(web_code_to_evdev("UnknownKey"), 0);
        assert_eq!(web_code_to_evdev(""), 0);
    }

    #[test]
    fn test_evdev_to_char_letters() {
        assert_eq!(evdev_to_char(30), Some('a'));
        assert_eq!(evdev_to_char(44), Some('z'));
    }

    #[test]
    fn test_evdev_to_char_digits() {
        assert_eq!(evdev_to_char(2), Some('1'));
        assert_eq!(evdev_to_char(11), Some('0'));
    }

    #[test]
    fn test_evdev_to_char_symbols() {
        assert_eq!(evdev_to_char(57), Some(' '));
        assert_eq!(evdev_to_char(12), Some('-'));
        assert_eq!(evdev_to_char(52), Some('.'));
    }

    #[test]
    fn test_evdev_to_char_unknown() {
        assert_eq!(evdev_to_char(0), None);
        assert_eq!(evdev_to_char(999), None);
    }

    #[test]
    fn test_parse_resolution() {
        assert_eq!(
            parse_resolution("1920x1080 px, 60.001 Hz (current)"),
            Some((1920, 1080))
        );
        assert_eq!(
            parse_resolution("3840x2160 px, 59.997 Hz"),
            Some((3840, 2160))
        );
        assert_eq!(parse_resolution("no resolution here"), None);
    }

    #[test]
    fn test_extract_first_number() {
        let json = r#"{"width": 1920, "height": 1080}"#;
        assert_eq!(extract_first_number(json, "width"), Some(1920));
        assert_eq!(extract_first_number(json, "height"), Some(1080));
        assert_eq!(extract_first_number(json, "missing"), None);
    }

    #[test]
    fn test_evdev_to_dotool_keyname_letters() {
        assert_eq!(evdev_to_dotool_keyname(30), Some("a"));
        assert_eq!(evdev_to_dotool_keyname(44), Some("z"));
        assert_eq!(evdev_to_dotool_keyname(50), Some("m"));
    }

    #[test]
    fn test_evdev_to_dotool_keyname_modifiers() {
        assert_eq!(evdev_to_dotool_keyname(42), Some("shift"));
        assert_eq!(evdev_to_dotool_keyname(29), Some("ctrl"));
        assert_eq!(evdev_to_dotool_keyname(56), Some("alt"));
        assert_eq!(evdev_to_dotool_keyname(125), Some("super"));
    }

    #[test]
    fn test_evdev_to_dotool_keyname_control_keys() {
        assert_eq!(evdev_to_dotool_keyname(28), Some("Return"));
        assert_eq!(evdev_to_dotool_keyname(57), Some("space"));
        assert_eq!(evdev_to_dotool_keyname(1), Some("Escape"));
        assert_eq!(evdev_to_dotool_keyname(14), Some("BackSpace"));
        assert_eq!(evdev_to_dotool_keyname(15), Some("Tab"));
    }

    #[test]
    fn test_evdev_to_dotool_keyname_arrows() {
        assert_eq!(evdev_to_dotool_keyname(103), Some("Up"));
        assert_eq!(evdev_to_dotool_keyname(108), Some("Down"));
        assert_eq!(evdev_to_dotool_keyname(105), Some("Left"));
        assert_eq!(evdev_to_dotool_keyname(106), Some("Right"));
    }

    #[test]
    fn test_evdev_to_dotool_keyname_unknown() {
        assert_eq!(evdev_to_dotool_keyname(0), None);
        assert_eq!(evdev_to_dotool_keyname(999), None);
    }

    #[test]
    fn test_wayland_injector_fails_without_wayland() {
        // 在无 Wayland 环境下创建注入器应失败
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            let result = WaylandInputInjector::new();
            assert!(result.is_err());
        }
    }
}
