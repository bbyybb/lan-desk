**中文** | [English](#faq-english)

# 常见问题 (FAQ)

## 连接问题

### Q: 输入 IP 后连接不上怎么办？

1. 确认两台电脑在**同一局域网**（同一 WiFi 或交换机）
2. 检查被控端的**防火墙**是否放行了 TCP 25605 和 UDP 25606 端口
3. 确认被控端 LAN-Desk 已启动且显示「等待连接中」
4. 尝试在控制端 `ping` 被控端 IP 地址

### Q: 扫描不到设备？

- UDP 广播可能被路由器/交换机屏蔽，请改为手动输入 IP
- 确认两台电脑在同一网段（如都是 192.168.1.x）

### Q: 可以跨网络/公网使用吗？

可以，需要搭配内网穿透工具。推荐方案：
- **Tailscale**（最简单）：两端安装并登录同一账号
- **Cloudflare Tunnel**（免费）：利用全球 CDN
- 详见 [跨网络远程访问指南](REMOTE_ACCESS.md)

## 密码和权限

### Q: 控制密码和查看密码有什么区别？

- **控制密码**：允许键鼠操控、文件传输、剪贴板同步、远程终端
- **查看密码**：只能观看远程画面，无法操作

### Q: 密码每次启动都变，有没有固定密码？

进入 设置 → 无人值守 → 开启「使用固定密码」，即可自定义 6-20 位密码。

### Q: 什么是无人值守模式？

开启后远程连接不再弹窗确认，适合需要远程访问无人看管电脑的场景。

### Q: 连续输错密码后无法连接？

A: 安全防护机制：连续 5 次 PIN 错误后该 IP 被锁定 5 分钟，后续失败锁定时间指数递增（15分钟→45分钟→…→最长24小时），并设有全局速率限制（每分钟最多 10 次失败）。请耐心等待后重试。

## 画质和性能

### Q: 画面延迟高/卡顿怎么办？

1. 进入 **设置** 降低 JPEG 画质（如从 75 降到 50）
2. 降低最大帧率（如从 30 降到 15）
3. 检查网络带宽是否充足（建议 ≥10 Mbps）
4. 如有独立显卡，程序会自动使用 GPU 硬件编码：NVENC (Windows NVIDIA) / VideoToolbox (macOS) / VAAPI (Linux)。日志中会显示对应编码器名称

### Q: 怎么看当前连接质量？

工具栏显示实时指标：FPS、延迟、带宽、RTT、CPU/内存使用率，以及三色信号灯（绿/黄/红）。

## 音频

### Q: 听不到远程电脑的声音？

- **Windows**：通常开箱即用
- **macOS**：需要安装虚拟音频设备（如 [BlackHole](https://github.com/ExistentialAudio/BlackHole)），并在系统设置中设为默认输出设备

### Q: Opus 音频编码是什么？怎么启用？

Opus 是一种高效音频压缩编码，可将音频传输带宽从 ~1.5Mbps 降至 ~128kbps（降低 92%）。构建时**自动检测**系统是否安装了 `cmake`（>= 3.10）：
- **有 cmake**：自动启用 Opus 编码
- **无 cmake**：自动回退到 PCM 原始传输（功能完全正常，仅带宽稍大）

你也可以通过环境变量强制控制：
- `FORCE_OPUS=1 npm run build:release` — 强制启用（需有 cmake）
- `FORCE_OPUS=0 npm run build:release` — 强制禁用

安装 cmake 后重新构建即可自动获得 Opus 支持，无需修改任何代码。

## 平台相关

### Q: macOS 上画面黑屏？

需要授权 系统设置 → 隐私与安全 → **屏幕录制** 权限。授权后需要重启应用。

### Q: macOS 上键鼠不生效？

需要授权 系统设置 → 隐私与安全 → **辅助功能** 权限。

### Q: Linux 上能用吗？

支持。X11 环境通过 XShm + XTest 实现。Wayland 支持原生捕获（需安装 grim/spectacle 截图 + ydotool/dotool 输入注入），也兼容 XWayland。

### Q: Wayland 下如何使用屏幕捕获？

LAN-Desk 在 Wayland 下采用三级自动检测方案：

1. **PipeWire/Portal（首选）**：通过 XDG Desktop Portal + PipeWire 实现原生屏幕捕获，无需安装 grim/spectacle 等外部工具。首次使用时系统会弹出授权对话框，选择要共享的屏幕后，后续捕获自动跳过授权（通过 restore_token）。支持 GNOME、KDE、Sway、Hyprland 等主流合成器。
2. **外部截图工具（降级）**：如果 PipeWire 不可用，自动降级到 grim 或 spectacle 截图。
3. **X11 XShm（最终降级）**：如果外部工具也不可用，回退到 XWayland 上的 X11 XShm 捕获。

运行时依赖：系统需安装 `libpipewire-0.3`。大多数现代 Linux 桌面（Fedora、Ubuntu 22.04+）已默认安装。如未安装，程序会自动降级到外部工具或 X11，不影响使用。

## 远程终端

### Q: 远程终端是什么？怎么用？

远程终端允许你在控制端打开被控端电脑的命令行（Shell）。远程终端默认禁用，需要在被控端的 **设置** 中手动启用「允许远程终端」。启用后，连接时点击工具栏的 **终端** 按钮即可打开。
- Windows 被控端：默认打开 PowerShell
- macOS/Linux 被控端：默认打开用户登录 Shell（通常是 bash 或 zsh）

### Q: 远程终端安全吗？

远程终端仅在使用**控制密码**连接时可用（查看密码不支持终端）。终端数据通过 TLS 加密传输，与远程桌面共享同一安全通道。

### Q: 终端窗口大小能调整吗？

可以。拖动终端面板边缘即可调整大小，PTY 会自动同步新的行列数。

### Q: 远程终端为什么自动断开了？

Shell 会话有 30 分钟空闲超时，无操作将自动关闭。重新打开终端即可。

## macOS 权限

### Q: macOS 首次运行时如何授权权限？

LAN-Desk 会自动检测屏幕录制和辅助功能权限，如未授权会弹出引导提示。请按提示前往系统偏好设置 > 隐私与安全性中授权。

## 便携模式

### Q: 如何使用免安装/便携模式？

在 LAN-Desk 可执行文件（exe）所在目录创建一个名为 `.portable` 的空文件即可。程序检测到该文件后会将所有配置和数据存储在 exe 同目录下，无需安装到系统，方便 U 盘携带使用。

## 快捷键

### Q: 如何发送 Ctrl+Alt+Del 等系统快捷键？

在远程桌面工具栏点击"特殊按键"菜单，可以发送 Ctrl+Alt+Del、Alt+Tab、Alt+F4 等系统级快捷键到被控端。

## 文件浏览器

### Q: 如何使用文件浏览器？

在远程桌面工具栏点击"文件"按钮，可打开双栏文件浏览器。左侧显示本地目录，右侧显示远程目录，支持拖拽上传/下载、目录传输和断点续传。

## 主题切换

### Q: 如何切换深色/浅色主题？

在设置页面的"主题"选项中可以选择深色、浅色或跟随系统主题自动切换。

## 证书管理

### Q: 如何管理已信任的远程主机证书？

在设置页面底部的"已信任主机"区域可以查看所有 TOFU 证书指纹，支持单独撤销。

---

<a id="faq-english"></a>

# FAQ (English)

## Connection

### Q: Can't connect after entering IP?

1. Ensure both computers are on the **same LAN**
2. Check firewall allows TCP 25605 and UDP 25606
3. Confirm host LAN-Desk is running
4. Try pinging the host IP

### Q: Scan finds no devices?

- UDP broadcast may be blocked by router. Use manual IP entry instead.

### Q: Can I use it over the internet?

Yes, with NAT traversal tools like Tailscale, Cloudflare Tunnel, or frp. See [Remote Access Guide](REMOTE_ACCESS.md).

## Passwords & Permissions

### Q: What's the difference between Control PIN and View PIN?

- **Control PIN**: Full access (keyboard, mouse, files, clipboard, terminal)
- **View PIN**: Screen viewing only

### Q: Can I set a fixed password?

Yes. Settings → Unattended Access → Enable "Use Fixed Password".

### Q: Locked out after entering wrong PIN multiple times?

A: Security feature: after 5 consecutive wrong PINs, the IP is locked for 5 minutes. Subsequent failures increase the lockout exponentially (15 min → 45 min → … → up to 24 hours), with a global rate limit (max 10 failures per minute). Please wait and try again.

## Performance

### Q: High latency or stuttering?

1. Lower JPEG quality in Settings
2. Lower max FPS
3. Ensure network bandwidth ≥10 Mbps
4. GPU hardware encoding is automatic when available: NVENC (Windows NVIDIA) / VideoToolbox (macOS) / VAAPI (Linux)

## Audio

### Q: No sound from remote computer?

- **Windows**: Usually works out of the box
- **macOS**: Requires a virtual audio device (e.g. [BlackHole](https://github.com/ExistentialAudio/BlackHole)), set as default output in System Settings

### Q: What is Opus audio encoding? How to enable it?

Opus is an efficient audio codec that reduces audio bandwidth from ~1.5Mbps to ~128kbps (92% reduction). The build system **auto-detects** whether `cmake` (>= 3.10) is installed:
- **cmake available**: Opus encoding is automatically enabled
- **cmake not found**: Falls back to raw PCM (fully functional, just higher bandwidth)

You can also force it via environment variables:
- `FORCE_OPUS=1 npm run build:release` — Force enable (requires cmake)
- `FORCE_OPUS=0 npm run build:release` — Force disable

Install cmake and rebuild to get Opus support — no code changes needed.

## Platform

### Q: Black screen on macOS?

Grant Screen Recording permission in System Settings → Privacy & Security.

### Q: Keyboard/mouse not working on macOS?

Grant Accessibility permission in System Settings → Privacy & Security.

### Q: Linux support?

Yes. X11 is supported via XShm + XTest. Wayland is supported with native capture (requires grim/spectacle for screenshots + ydotool/dotool for input injection), and also works through XWayland compatibility.

### Q: How does screen capture work on Wayland?

LAN-Desk uses a three-level auto-detection strategy on Wayland:

1. **PipeWire/Portal (preferred)**: Native screen capture via XDG Desktop Portal + PipeWire, requiring no external tools like grim/spectacle. On first use, the system displays an authorization dialog to select the screen to share. Subsequent captures skip authorization automatically (via restore_token). Supports GNOME, KDE, Sway, Hyprland, and other major compositors.
2. **External screenshot tools (fallback)**: If PipeWire is unavailable, automatically falls back to grim or spectacle.
3. **X11 XShm (final fallback)**: If external tools are also unavailable, falls back to X11 XShm capture on XWayland.

Runtime dependency: the system needs `libpipewire-0.3` installed. Most modern Linux desktops (Fedora, Ubuntu 22.04+) have it pre-installed. If not installed, the program gracefully degrades to external tools or X11.

## Remote Terminal

### Q: What is the remote terminal? How do I use it?

The remote terminal lets you open a command-line shell on the host computer. The remote terminal is disabled by default and must be enabled in the host's **Settings** ("Allow Remote Terminal"). Once enabled, click the **Terminal** button in the toolbar after connecting.
- Windows host: Opens PowerShell by default
- macOS/Linux host: Opens the user's login shell (usually bash or zsh)

### Q: Is the remote terminal secure?

The remote terminal is only available when connected with the **Control PIN** (View PIN does not support terminal). Terminal data is encrypted via TLS, sharing the same secure channel as the remote desktop.

### Q: Can I resize the terminal window?

Yes. Drag the terminal panel edge to resize. The PTY automatically syncs the new row/column dimensions.

### Q: Why did the remote terminal disconnect automatically?

Shell sessions have a 30-minute idle timeout and will close automatically when inactive. Simply reopen the terminal.

## macOS Permissions

### Q: How do I grant permissions on first launch on macOS?

LAN-Desk automatically detects Screen Recording and Accessibility permissions. If not yet granted, it will display a guided prompt. Follow the prompt to grant permissions in System Preferences > Privacy & Security.

## Portable Mode

### Q: How do I use portable / no-install mode?

Create an empty file named `.portable` in the same directory as the LAN-Desk executable. When the program detects this file, it stores all configuration and data in the exe's directory, requiring no system installation -- ideal for USB drive use.

## Shortcuts

### Q: How do I send Ctrl+Alt+Del or other system shortcuts?

Click the "Special Keys" menu in the remote desktop toolbar to send system-level shortcuts like Ctrl+Alt+Del, Alt+Tab, Alt+F4, etc. to the remote host.

## File Browser

### Q: How do I use the file browser?

Click the "File" button in the remote desktop toolbar to open a dual-pane file browser. The left pane shows local directories, the right pane shows remote directories. Supports drag-and-drop upload/download, directory transfer, and resume.

## Theme

### Q: How do I switch themes?

In the Settings page, use the "Theme" option to choose between dark, light, or system-follow (automatic switching).

## Certificate Management

### Q: How do I manage trusted remote host certificates?

In the "Trusted Hosts" section at the bottom of the Settings page, you can view all TOFU certificate fingerprints and revoke them individually.
