/// 原生 Wayland 屏幕捕获
///
/// 支持两种模式（运行时自动选择）：
/// 1. grim 模式：通过 grim 命令行工具截图（需安装 grim）
/// 2. gnome-screenshot 模式：通过 gnome-screenshot 作为后备
///
/// Wayland 安全模型不允许应用直接读取其他窗口的像素，
/// 必须通过 portal/compositor 授权的机制进行屏幕共享。
use crate::frame::{CapturedFrame, PixelFormat};
use crate::ScreenCapture;
use lan_desk_protocol::message::MonitorInfo;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

/// 捕获方式枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureMethod {
    /// grim 命令行截图（wlroots 合成器，如 sway、Hyprland）
    Grim,
    /// spectacle 命令行截图（KDE Plasma 桌面）
    Spectacle,
    /// gnome-screenshot（GNOME/Mutter 合成器）
    GnomeScreenshot,
}

/// 原生 Wayland 屏幕捕获器
pub struct WaylandCapture {
    width: u32,
    height: u32,
    /// wlr-randr 或 swaymsg 获取的输出名称（如 "eDP-1"）
    output_name: Option<String>,
    /// 选定的捕获方式
    capture_method: CaptureMethod,
}

impl WaylandCapture {
    /// 创建 Wayland 屏幕捕获器
    ///
    /// 按优先级检测可用的截图工具：grim > spectacle > gnome-screenshot。
    /// 同时获取显示器信息以确定分辨率和输出名称。
    pub fn new(display_index: usize) -> anyhow::Result<Self> {
        // 检测是否在 Wayland 环境中
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            anyhow::bail!("未检测到 Wayland 显示服务器（WAYLAND_DISPLAY 未设置）");
        }

        // 检测可用的捕获方式
        let capture_method = detect_capture_method()?;
        info!("Wayland 屏幕捕获方式: {:?}", capture_method);

        // 获取显示器列表
        let monitors = list_wayland_monitors().unwrap_or_default();

        let (width, height, output_name) = if let Some(monitor) = monitors.get(display_index) {
            (monitor.width, monitor.height, Some(monitor.name.clone()))
        } else if !monitors.is_empty() {
            // 请求的索引不存在，使用第一个显示器
            let m = &monitors[0];
            warn!(
                "显示器索引 {} 不存在，使用第一个显示器: {} ({}x{})",
                display_index, m.name, m.width, m.height
            );
            (m.width, m.height, Some(m.name.clone()))
        } else {
            // 无法获取显示器列表，尝试通过试探性截图获取分辨率
            warn!("无法获取 Wayland 显示器列表，尝试试探性截图获取分辨率");
            let (w, h) = probe_screen_size(&capture_method)?;
            (w, h, None)
        };

        info!(
            "Wayland 屏幕捕获已初始化: {}x{}, 输出={:?}, 方式={:?}",
            width, height, output_name, capture_method
        );

        Ok(Self {
            width,
            height,
            output_name,
            capture_method,
        })
    }
}

impl ScreenCapture for WaylandCapture {
    fn capture_frame(&mut self) -> anyhow::Result<CapturedFrame> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let ppm_data = match self.capture_method {
            CaptureMethod::Grim => capture_with_grim(self.output_name.as_deref())?,
            CaptureMethod::Spectacle => capture_with_spectacle()?,
            CaptureMethod::GnomeScreenshot => capture_with_gnome_screenshot()?,
        };

        let (width, height, rgb_data) = parse_ppm(&ppm_data)?;

        // 如果分辨率发生变化（热插拔/分辨率切换），更新缓存
        if width != self.width || height != self.height {
            info!(
                "Wayland 屏幕分辨率变化: {}x{} -> {}x{}",
                self.width, self.height, width, height
            );
            self.width = width;
            self.height = height;
        }

        // 将 RGB 转为 BGRA
        let bgra_data = rgb_to_bgra(&rgb_data);

        Ok(CapturedFrame {
            width,
            height,
            stride: width * 4,
            pixel_format: PixelFormat::Bgra8,
            data: bgra_data,
            timestamp_ms: timestamp,
        })
    }

    fn screen_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// 检测是否在 Wayland 会话中运行
///
/// 不再要求 DISPLAY 未设置（大多数 Wayland 桌面会同时设置 DISPLAY 做 XWayland 兼容）。
/// 通过以下方式检测：
/// 1. WAYLAND_DISPLAY 环境变量已设置
/// 2. XDG_SESSION_TYPE 为 "wayland"
pub fn is_wayland_session() -> bool {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return true;
    }
    if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
        if session_type == "wayland" {
            return true;
        }
    }
    false
}

/// 旧版检测函数（条件过于严格，大多数 Wayland 桌面不满足）
#[allow(dead_code)]
pub fn is_native_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok() && std::env::var("DISPLAY").is_err()
}

/// 枚举 Wayland 显示器
///
/// 按优先级尝试以下方法：
/// 1. wlr-randr（wlroots 合成器）
/// 2. swaymsg -t get_outputs（Sway）
/// 3. gnome-randr（GNOME）
pub fn list_wayland_monitors() -> anyhow::Result<Vec<MonitorInfo>> {
    // 方法 1: wlr-randr
    if let Ok(monitors) = list_monitors_wlr_randr() {
        if !monitors.is_empty() {
            return Ok(monitors);
        }
    }

    // 方法 2: swaymsg
    if let Ok(monitors) = list_monitors_swaymsg() {
        if !monitors.is_empty() {
            return Ok(monitors);
        }
    }

    // 方法 3: 通过 grim 试探（只能获取一个显示器的分辨率）
    if is_tool_available("grim") {
        if let Ok((w, h)) = probe_screen_size(&CaptureMethod::Grim) {
            return Ok(vec![MonitorInfo {
                index: 0,
                name: "Wayland Output".to_string(),
                width: w,
                height: h,
                is_primary: true,
                left: 0,
                top: 0,
            }]);
        }
    }

    anyhow::bail!("无法获取 Wayland 显示器列表。请安装 wlr-randr 或确保 swaymsg 可用。")
}

// =============================================================================
// 内部实现函数
// =============================================================================

/// 检测可用的截图工具
fn detect_capture_method() -> anyhow::Result<CaptureMethod> {
    if is_tool_available("grim") {
        return Ok(CaptureMethod::Grim);
    }
    if is_tool_available("spectacle") {
        return Ok(CaptureMethod::Spectacle);
    }
    if is_tool_available("gnome-screenshot") {
        return Ok(CaptureMethod::GnomeScreenshot);
    }
    anyhow::bail!(
        "未找到可用的 Wayland 截图工具。请安装以下任一工具：\n\
         - grim（推荐，用于 wlroots 合成器如 sway、Hyprland）\n\
         - spectacle（用于 KDE Plasma 桌面）\n\
         - gnome-screenshot（用于 GNOME 桌面）\n\n\
         在 Arch Linux 上: pacman -S grim 或 pacman -S spectacle\n\
         在 Ubuntu/Debian 上: apt install grim\n\
         在 Fedora 上: dnf install grim"
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

/// 通过 grim 截图，输出 PPM 格式到 stdout
fn capture_with_grim(output_name: Option<&str>) -> anyhow::Result<Vec<u8>> {
    let mut cmd = Command::new("grim");

    // 指定输出（显示器）
    if let Some(name) = output_name {
        cmd.args(["-o", name]);
    }

    // 输出 PPM 格式到 stdout
    cmd.args(["-t", "ppm", "-"]);

    let output = run_command_with_timeout(&mut cmd, Duration::from_secs(5))
        .map_err(|e| anyhow::anyhow!("grim 截图失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "grim 返回错误 (code {:?}): {}",
            output.status.code(),
            stderr
        );
    }

    if output.stdout.is_empty() {
        anyhow::bail!("grim 返回空数据");
    }

    Ok(output.stdout)
}

/// 通过 KDE Spectacle 截图，返回 PPM 数据
///
/// spectacle 支持 `--background --nonotify` 静默截图。
/// 由于 spectacle 不支持直接输出 PPM 到 stdout，
/// 先保存为临时 PNG 文件，再转换为 PPM。
fn capture_with_spectacle() -> anyhow::Result<Vec<u8>> {
    let png_file = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .map_err(|e| anyhow::anyhow!("创建临时文件失败: {}", e))?;
    let png_path = png_file.path().to_owned();

    let mut cmd = Command::new("spectacle");
    cmd.args([
        "--background",
        "--nonotify",
        "--fullscreen",
        "--output",
        &png_path.to_string_lossy(),
    ]);

    let output = run_command_with_timeout(&mut cmd, Duration::from_secs(10))
        .map_err(|e| anyhow::anyhow!("spectacle 截图失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "spectacle 返回错误 (code {:?}): {}",
            output.status.code(),
            stderr
        );
    }

    // 将 PNG 转换为 PPM
    let tmp_file = tempfile::Builder::new()
        .suffix(".ppm")
        .tempfile()
        .map_err(|e| anyhow::anyhow!("创建临时文件失败: {}", e))?;
    let tmp_path = tmp_file.path().to_owned();

    if is_tool_available("convert") {
        let mut convert_cmd = Command::new("convert");
        convert_cmd.args([&*png_path.to_string_lossy(), &*tmp_path.to_string_lossy()]);
        let conv_out = run_command_with_timeout(&mut convert_cmd, Duration::from_secs(5))?;
        if conv_out.status.success() {
            let data = std::fs::read(&tmp_path)?;
            return Ok(data);
        }
    }

    if is_tool_available("ffmpeg") {
        let mut ffmpeg_cmd = Command::new("ffmpeg");
        ffmpeg_cmd.args([
            "-y",
            "-i",
            &*png_path.to_string_lossy(),
            "-f",
            "image2",
            "-pix_fmt",
            "rgb24",
            &*tmp_path.to_string_lossy(),
        ]);
        let ff_out = run_command_with_timeout(&mut ffmpeg_cmd, Duration::from_secs(5))?;
        if ff_out.status.success() {
            let data = std::fs::read(&tmp_path)?;
            return Ok(data);
        }
    }

    anyhow::bail!(
        "spectacle 已截图但无法转换为 PPM 格式。\n\
         请安装 ImageMagick (convert) 或 ffmpeg 进行格式转换。"
    )
}

/// 通过 gnome-screenshot 截图，返回 PPM 数据
///
/// gnome-screenshot 不支持 PPM 输出到 stdout，
/// 因此先保存为临时 PNG 文件，再用 convert 或直接解析。
/// 这里我们保存为临时文件后读取原始 PNG 数据，然后用简单方式转换。
fn capture_with_gnome_screenshot() -> anyhow::Result<Vec<u8>> {
    let tmp_file = tempfile::Builder::new()
        .suffix(".ppm")
        .tempfile()
        .map_err(|e| anyhow::anyhow!("创建临时文件失败: {}", e))?;
    let tmp_path = tmp_file.path().to_owned();

    let png_file = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .map_err(|e| anyhow::anyhow!("创建临时文件失败: {}", e))?;
    let png_path = png_file.path().to_owned();

    // gnome-screenshot 不直接支持 PPM，先截图为 PNG
    let mut cmd = Command::new("gnome-screenshot");
    cmd.args(["--file", &png_path.to_string_lossy()]);

    let output = run_command_with_timeout(&mut cmd, Duration::from_secs(10))
        .map_err(|e| anyhow::anyhow!("gnome-screenshot 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gnome-screenshot 错误: {}", stderr);
    }

    // 使用 convert (ImageMagick) 或 ffmpeg 将 PNG 转为 PPM
    if is_tool_available("convert") {
        let mut convert_cmd = Command::new("convert");
        convert_cmd.args([&*png_path.to_string_lossy(), &*tmp_path.to_string_lossy()]);
        let conv_out = run_command_with_timeout(&mut convert_cmd, Duration::from_secs(5))?;
        if conv_out.status.success() {
            let data = std::fs::read(&tmp_path)?;
            return Ok(data);
        }
    }

    if is_tool_available("ffmpeg") {
        let mut ffmpeg_cmd = Command::new("ffmpeg");
        ffmpeg_cmd.args([
            "-y",
            "-i",
            &*png_path.to_string_lossy(),
            "-f",
            "image2",
            "-pix_fmt",
            "rgb24",
            &*tmp_path.to_string_lossy(),
        ]);
        let ff_out = run_command_with_timeout(&mut ffmpeg_cmd, Duration::from_secs(5))?;
        if ff_out.status.success() {
            let data = std::fs::read(&tmp_path)?;
            return Ok(data);
        }
    }

    anyhow::bail!(
        "gnome-screenshot 已截图但无法转换为 PPM 格式。\n\
         请安装 ImageMagick (convert) 或 ffmpeg 进行格式转换。"
    )
}

/// 执行命令并设置超时
///
/// 使用轮询 `try_wait` 的方式检测子进程完成，超时后主动 kill 子进程并 wait 回收，
/// 避免旧实现中超时路径仅 drop JoinHandle 而不杀死子进程导致的进程泄漏。
fn run_command_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动进程失败: {}", e))?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|e| anyhow::anyhow!("命令执行失败: {}", e));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait(); // 回收子进程，避免僵尸进程
                    anyhow::bail!("命令执行超时（{}秒）", timeout.as_secs());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => anyhow::bail!("检查进程状态失败: {}", e),
        }
    }
}

/// 解析 PPM (P6) 二进制格式图像
///
/// PPM P6 格式：
/// ```text
/// P6
/// <宽度> <高度>
/// <最大颜色值>
/// <二进制 RGB 数据>
/// ```
///
/// 注意：注释行以 '#' 开头，可出现在头部任何位置。
pub fn parse_ppm(data: &[u8]) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let mut pos;

    // 跳过空白字符
    fn skip_whitespace(data: &[u8], pos: &mut usize) {
        while *pos < data.len()
            && (data[*pos] == b' '
                || data[*pos] == b'\n'
                || data[*pos] == b'\r'
                || data[*pos] == b'\t')
        {
            *pos += 1;
        }
    }

    // 跳过注释行
    fn skip_comments(data: &[u8], pos: &mut usize) {
        while *pos < data.len() && data[*pos] == b'#' {
            // 跳到行尾
            while *pos < data.len() && data[*pos] != b'\n' {
                *pos += 1;
            }
            if *pos < data.len() {
                *pos += 1; // 跳过换行符
            }
        }
    }

    // 读取一个数字
    fn read_number(data: &[u8], pos: &mut usize) -> anyhow::Result<u32> {
        skip_whitespace(data, pos);
        skip_comments(data, pos);
        skip_whitespace(data, pos);

        let start = *pos;
        while *pos < data.len() && data[*pos].is_ascii_digit() {
            *pos += 1;
        }
        if start == *pos {
            anyhow::bail!("PPM 解析错误: 预期数字，位置 {}", start);
        }
        let s = std::str::from_utf8(&data[start..*pos])
            .map_err(|_| anyhow::anyhow!("PPM 解析错误: 无效的 UTF-8"))?;
        s.parse::<u32>()
            .map_err(|_| anyhow::anyhow!("PPM 解析错误: 无法解析数字 '{}'", s))
    }

    // 验证魔数
    if data.len() < 3 {
        anyhow::bail!("PPM 数据太短");
    }
    if data[0] != b'P' || data[1] != b'6' {
        anyhow::bail!(
            "不是有效的 PPM P6 格式（魔数: {:?}）",
            &data[..2.min(data.len())]
        );
    }
    pos = 2;

    // 读取宽高和最大颜色值
    let width = read_number(data, &mut pos)?;
    let height = read_number(data, &mut pos)?;
    let max_val = read_number(data, &mut pos)?;

    if width == 0 || height == 0 {
        anyhow::bail!("PPM 尺寸无效: {}x{}", width, height);
    }
    if max_val > 65535 {
        anyhow::bail!("PPM 最大颜色值无效: {}", max_val);
    }

    // 头部后有一个空白字符分隔（通常是换行符）
    if pos < data.len()
        && (data[pos] == b' ' || data[pos] == b'\n' || data[pos] == b'\r' || data[pos] == b'\t')
    {
        pos += 1;
    }

    // 计算预期数据长度
    let bytes_per_sample = if max_val > 255 { 2 } else { 1 };
    let expected_len = (width as usize) * (height as usize) * 3 * bytes_per_sample;
    let remaining = data.len() - pos;

    if remaining < expected_len {
        anyhow::bail!(
            "PPM 数据不完整: 预期 {} 字节 RGB 数据，实际只有 {} 字节（{}x{}, maxval={}）",
            expected_len,
            remaining,
            width,
            height,
            max_val
        );
    }

    let rgb_data = if bytes_per_sample == 1 {
        data[pos..pos + expected_len].to_vec()
    } else {
        // 16-bit 样本转为 8-bit
        let mut result = Vec::with_capacity((width as usize) * (height as usize) * 3);
        let raw = &data[pos..pos + expected_len];
        for i in (0..raw.len()).step_by(2) {
            let val = ((raw[i] as u16) << 8 | raw[i + 1] as u16) as f64;
            result.push((val / max_val as f64 * 255.0) as u8);
        }
        result
    };

    Ok((width, height, rgb_data))
}

/// 将 RGB 数据转为 BGRA 格式
pub fn rgb_to_bgra(rgb: &[u8]) -> Vec<u8> {
    let pixel_count = rgb.len() / 3;
    let mut bgra = Vec::with_capacity(pixel_count * 4);
    for i in 0..pixel_count {
        let offset = i * 3;
        bgra.push(rgb[offset + 2]); // B
        bgra.push(rgb[offset + 1]); // G
        bgra.push(rgb[offset]); // R
        bgra.push(255); // A
    }
    bgra
}

/// 通过试探性截图获取屏幕分辨率
fn probe_screen_size(method: &CaptureMethod) -> anyhow::Result<(u32, u32)> {
    let ppm_data = match method {
        CaptureMethod::Grim => capture_with_grim(None)?,
        CaptureMethod::Spectacle => capture_with_spectacle()?,
        CaptureMethod::GnomeScreenshot => capture_with_gnome_screenshot()?,
    };
    let (w, h, _) = parse_ppm(&ppm_data)?;
    Ok((w, h))
}

/// 通过 wlr-randr 获取显示器列表
fn list_monitors_wlr_randr() -> anyhow::Result<Vec<MonitorInfo>> {
    if !is_tool_available("wlr-randr") {
        anyhow::bail!("wlr-randr 不可用");
    }

    let output = Command::new("wlr-randr")
        .output()
        .map_err(|e| anyhow::anyhow!("执行 wlr-randr 失败: {}", e))?;

    if !output.status.success() {
        anyhow::bail!("wlr-randr 返回错误");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_wlr_randr_output(&stdout)
}

/// 解析 wlr-randr 输出
///
/// 示例输出格式：
/// ```text
/// eDP-1 "AU Optronics 0x573D" (eDP-1)
///   Enabled: yes
///   Modes:
///     1920x1080 px, 60.001000 Hz (preferred, current)
///     1920x1080 px, 48.001000 Hz
/// DP-2 "Dell U2720Q" (DP-2)
///   Enabled: yes
///   Modes:
///     3840x2160 px, 59.997000 Hz (preferred, current)
/// ```
pub fn parse_wlr_randr_output(output: &str) -> anyhow::Result<Vec<MonitorInfo>> {
    let mut monitors = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_enabled = false;
    let mut current_width: u32 = 0;
    let mut current_height: u32 = 0;
    let mut found_current_mode = false;

    for line in output.lines() {
        let trimmed = line.trim();

        // 检测输出名称行（不以空白开头的行）
        if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.is_empty() {
            // 保存前一个显示器
            if let Some(name) = current_name.take() {
                if current_enabled && current_width > 0 {
                    monitors.push(MonitorInfo {
                        index: monitors.len() as u32,
                        name,
                        width: current_width,
                        height: current_height,
                        is_primary: monitors.is_empty(),
                        left: 0,
                        top: 0,
                    });
                }
            }

            // 提取输出名称（行中第一个空格前的部分）
            current_name = Some(
                trimmed
                    .split_whitespace()
                    .next()
                    .unwrap_or(trimmed)
                    .to_string(),
            );
            current_enabled = false;
            current_width = 0;
            current_height = 0;
            found_current_mode = false;
        } else if trimmed.starts_with("Enabled:") {
            current_enabled = trimmed.contains("yes");
        } else if !found_current_mode && trimmed.contains("current") && trimmed.contains("px") {
            // 解析当前分辨率模式行，如 "1920x1080 px, 60.001000 Hz (preferred, current)"
            if let Some((w, h)) = parse_resolution_from_mode_line(trimmed) {
                current_width = w;
                current_height = h;
                found_current_mode = true;
            }
        }
    }

    // 保存最后一个显示器
    if let Some(name) = current_name {
        if current_enabled && current_width > 0 {
            monitors.push(MonitorInfo {
                index: monitors.len() as u32,
                name,
                width: current_width,
                height: current_height,
                is_primary: monitors.is_empty(),
                left: 0,
                top: 0,
            });
        }
    }

    Ok(monitors)
}

/// 从模式行解析分辨率（如 "1920x1080 px, 60.001 Hz (current)"）
fn parse_resolution_from_mode_line(line: &str) -> Option<(u32, u32)> {
    // 查找 "WxH" 模式
    let parts: Vec<&str> = line.split_whitespace().collect();
    for part in &parts {
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

/// 通过 swaymsg 获取显示器列表
fn list_monitors_swaymsg() -> anyhow::Result<Vec<MonitorInfo>> {
    if !is_tool_available("swaymsg") {
        anyhow::bail!("swaymsg 不可用");
    }

    let output = Command::new("swaymsg")
        .args(["-t", "get_outputs", "--raw"])
        .output()
        .map_err(|e| anyhow::anyhow!("执行 swaymsg 失败: {}", e))?;

    if !output.status.success() {
        anyhow::bail!("swaymsg 返回错误");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_swaymsg_output(&stdout)
}

/// 解析 swaymsg -t get_outputs --raw 的 JSON 输出
///
/// 输出格式为 JSON 数组，每个元素包含：
/// ```json
/// [
///   {
///     "name": "eDP-1",
///     "active": true,
///     "current_mode": { "width": 1920, "height": 1080, "refresh": 60001 },
///     "focused": true
///   }
/// ]
/// ```
pub fn parse_swaymsg_output(json_str: &str) -> anyhow::Result<Vec<MonitorInfo>> {
    // 简单的 JSON 解析（不引入 serde_json 依赖）
    // 查找所有输出对象
    let mut monitors = Vec::new();
    let trimmed = json_str.trim();

    if !trimmed.starts_with('[') {
        anyhow::bail!("swaymsg 输出格式不是 JSON 数组");
    }

    // 使用简单的状态机解析每个输出块
    // 查找 "name": "xxx" 和 "current_mode": { "width": N, "height": N }
    let mut i = 0;
    let bytes = trimmed.as_bytes();
    let mut brace_depth = 0;
    let mut in_output_block = false;
    let mut block_start = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if brace_depth == 1 {
                    // 这是数组内的顶层对象
                    in_output_block = true;
                    block_start = i;
                }
                brace_depth += 1;
            }
            b'}' => {
                brace_depth -= 1;
                if brace_depth == 1 && in_output_block {
                    // 解析这个输出块
                    let block = &trimmed[block_start..=i];
                    if let Some(monitor) = parse_single_swaymsg_output(block, monitors.len()) {
                        monitors.push(monitor);
                    }
                    in_output_block = false;
                }
            }
            b'[' if brace_depth == 0 => {
                brace_depth = 1;
            }
            b']' if brace_depth == 1 => {
                break;
            }
            _ => {}
        }
        i += 1;
    }

    Ok(monitors)
}

/// 从单个 swaymsg 输出 JSON 对象中解析显示器信息
fn parse_single_swaymsg_output(block: &str, index: usize) -> Option<MonitorInfo> {
    // 提取 "name": "xxx"
    let name = extract_json_string(block, "name")?;

    // 检查 "active": true
    if !block.contains("\"active\":true")
        && !block.contains("\"active\": true")
        && !block.contains("\"active\" : true")
    {
        return None;
    }

    // 提取 current_mode 中的 width 和 height
    let mode_start = block.find("\"current_mode\"")?;
    let mode_block = &block[mode_start..];
    let width = extract_json_number(mode_block, "width")?;
    let height = extract_json_number(mode_block, "height")?;

    // 检查是否有焦点（作为 primary 的判断依据）
    let is_primary = block.contains("\"focused\":true") || block.contains("\"focused\": true");

    Some(MonitorInfo {
        index: index as u32,
        name,
        width,
        height,
        is_primary,
        left: 0,
        top: 0,
    })
}

/// 从 JSON 字符串中提取指定键的字符串值（简单实现）
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let key_pos = json.find(&pattern)?;
    let after_key = &json[key_pos + pattern.len()..];
    // 跳过 : 和空白
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let value_start = 1; // 跳过开头引号
    let value_end = after_colon[value_start..].find('"')?;
    Some(after_colon[value_start..value_start + value_end].to_string())
}

/// 从 JSON 字符串中提取指定键的数字值（简单实现）
fn extract_json_number(json: &str, key: &str) -> Option<u32> {
    let pattern = format!("\"{}\"", key);
    let key_pos = json.find(&pattern)?;
    let after_key = &json[key_pos + pattern.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    // 读取连续数字
    let num_str: String = after_colon
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_wayland_session_without_env() {
        // 在没有设置 WAYLAND_DISPLAY 的环境中应返回 false
        // (CI 环境和 Windows 通常没有 Wayland)
        if std::env::var("WAYLAND_DISPLAY").is_err()
            && std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("wayland")
        {
            assert!(!is_wayland_session());
        }
    }

    #[test]
    fn test_wayland_capture_fails_without_wayland() {
        // 在无 Wayland 环境下创建捕获器应失败
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            let result = WaylandCapture::new(0);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_parse_ppm_valid_p6() {
        // 构造一个 2x2 的 PPM P6 图像
        let mut data = Vec::new();
        data.extend_from_slice(b"P6\n2 2\n255\n");
        // 4 个像素的 RGB 数据
        data.extend_from_slice(&[
            255, 0, 0, // 红
            0, 255, 0, // 绿
            0, 0, 255, // 蓝
            255, 255, 0, // 黄
        ]);

        let (w, h, rgb) = parse_ppm(&data).unwrap();
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(rgb.len(), 12); // 2*2*3
        assert_eq!(&rgb[0..3], &[255, 0, 0]); // 红色像素
        assert_eq!(&rgb[3..6], &[0, 255, 0]); // 绿色像素
        assert_eq!(&rgb[6..9], &[0, 0, 255]); // 蓝色像素
        assert_eq!(&rgb[9..12], &[255, 255, 0]); // 黄色像素
    }

    #[test]
    fn test_parse_ppm_with_comments() {
        let mut data = Vec::new();
        data.extend_from_slice(b"P6\n# comment line\n1 1\n# another comment\n255\n");
        data.extend_from_slice(&[128, 64, 32]);

        let (w, h, rgb) = parse_ppm(&data).unwrap();
        assert_eq!(w, 1);
        assert_eq!(h, 1);
        assert_eq!(&rgb[..], &[128, 64, 32]);
    }

    #[test]
    fn test_parse_ppm_invalid_magic() {
        let data = b"P5\n1 1\n255\nabc";
        assert!(parse_ppm(data).is_err());
    }

    #[test]
    fn test_parse_ppm_truncated_data() {
        let mut data = Vec::new();
        data.extend_from_slice(b"P6\n10 10\n255\n");
        data.extend_from_slice(&[0; 10]); // 远不够 10*10*3 = 300 字节
        assert!(parse_ppm(&data).is_err());
    }

    #[test]
    fn test_parse_ppm_zero_dimensions() {
        let data = b"P6\n0 100\n255\n";
        assert!(parse_ppm(data).is_err());
    }

    #[test]
    fn test_parse_ppm_16bit() {
        // 构造一个 1x1 的 16-bit PPM
        let mut data = Vec::new();
        data.extend_from_slice(b"P6\n1 1\n65535\n");
        // 16-bit 样本（大端序）
        data.extend_from_slice(&[
            0xFF, 0xFF, // R = 65535 -> 255
            0x80, 0x00, // G = 32768 -> ~128
            0x00, 0x00, // B = 0 -> 0
        ]);

        let (w, h, rgb) = parse_ppm(&data).unwrap();
        assert_eq!(w, 1);
        assert_eq!(h, 1);
        assert_eq!(rgb[0], 255); // R
        assert!(rgb[1] >= 127 && rgb[1] <= 129); // G ~128
        assert_eq!(rgb[2], 0); // B
    }

    #[test]
    fn test_rgb_to_bgra() {
        let rgb = vec![255, 128, 64, 0, 200, 100];
        let bgra = rgb_to_bgra(&rgb);
        assert_eq!(bgra.len(), 8); // 2 像素 * 4 字节
                                   // 第一个像素: RGB(255,128,64) -> BGRA(64,128,255,255)
        assert_eq!(&bgra[0..4], &[64, 128, 255, 255]);
        // 第二个像素: RGB(0,200,100) -> BGRA(100,200,0,255)
        assert_eq!(&bgra[4..8], &[100, 200, 0, 255]);
    }

    #[test]
    fn test_rgb_to_bgra_empty() {
        let bgra = rgb_to_bgra(&[]);
        assert!(bgra.is_empty());
    }

    #[test]
    fn test_parse_wlr_randr_output() {
        let output = r#"eDP-1 "AU Optronics 0x573D" (eDP-1)
  Enabled: yes
  Modes:
    1920x1080 px, 60.001000 Hz (preferred, current)
    1920x1080 px, 48.001000 Hz
DP-2 "Dell U2720Q" (DP-2)
  Enabled: yes
  Modes:
    3840x2160 px, 59.997000 Hz (preferred, current)
HDMI-A-1 "Inactive" (HDMI-A-1)
  Enabled: no
  Modes:
    1920x1080 px, 60.000000 Hz (preferred, current)
"#;

        let monitors = parse_wlr_randr_output(output).unwrap();
        assert_eq!(monitors.len(), 2); // HDMI-A-1 被过滤（Enabled: no）
        assert_eq!(monitors[0].name, "eDP-1");
        assert_eq!(monitors[0].width, 1920);
        assert_eq!(monitors[0].height, 1080);
        assert!(monitors[0].is_primary);
        assert_eq!(monitors[1].name, "DP-2");
        assert_eq!(monitors[1].width, 3840);
        assert_eq!(monitors[1].height, 2160);
        assert!(!monitors[1].is_primary);
    }

    #[test]
    fn test_parse_wlr_randr_empty() {
        let monitors = parse_wlr_randr_output("").unwrap();
        assert!(monitors.is_empty());
    }

    #[test]
    fn test_parse_swaymsg_output() {
        let json = r#"[
  {
    "name": "eDP-1",
    "active": true,
    "focused": true,
    "current_mode": {
      "width": 1920,
      "height": 1080,
      "refresh": 60001
    }
  },
  {
    "name": "DP-2",
    "active": true,
    "focused": false,
    "current_mode": {
      "width": 2560,
      "height": 1440,
      "refresh": 144000
    }
  }
]"#;

        let monitors = parse_swaymsg_output(json).unwrap();
        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0].name, "eDP-1");
        assert_eq!(monitors[0].width, 1920);
        assert_eq!(monitors[0].height, 1080);
        assert!(monitors[0].is_primary);
        assert_eq!(monitors[1].name, "DP-2");
        assert_eq!(monitors[1].width, 2560);
        assert_eq!(monitors[1].height, 1440);
        assert!(!monitors[1].is_primary);
    }

    #[test]
    fn test_parse_swaymsg_inactive_output() {
        let json = r#"[
  {
    "name": "HDMI-A-1",
    "active": false,
    "focused": false,
    "current_mode": {
      "width": 1920,
      "height": 1080,
      "refresh": 60000
    }
  }
]"#;

        let monitors = parse_swaymsg_output(json).unwrap();
        assert!(monitors.is_empty()); // 非活跃显示器应被过滤
    }

    #[test]
    fn test_extract_json_string() {
        let json = r#"{"name": "eDP-1", "other": "value"}"#;
        assert_eq!(extract_json_string(json, "name"), Some("eDP-1".to_string()));
        assert_eq!(
            extract_json_string(json, "other"),
            Some("value".to_string())
        );
        assert_eq!(extract_json_string(json, "missing"), None);
    }

    #[test]
    fn test_extract_json_number() {
        let json = r#"{"width": 1920, "height": 1080}"#;
        assert_eq!(extract_json_number(json, "width"), Some(1920));
        assert_eq!(extract_json_number(json, "height"), Some(1080));
        assert_eq!(extract_json_number(json, "missing"), None);
    }

    #[test]
    fn test_parse_resolution_from_mode_line() {
        assert_eq!(
            parse_resolution_from_mode_line("1920x1080 px, 60.001 Hz (preferred, current)"),
            Some((1920, 1080))
        );
        assert_eq!(
            parse_resolution_from_mode_line("3840x2160 px, 59.997 Hz (current)"),
            Some((3840, 2160))
        );
        assert_eq!(parse_resolution_from_mode_line("no resolution here"), None);
    }
}
