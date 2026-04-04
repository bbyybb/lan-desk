**中文** | [English](#troubleshooting-english)

# 故障排查指南

## 连接问题

### 连接超时 / 连接被拒绝

**症状**：输入 IP 后提示 "连接失败" 或长时间无响应。

**排查步骤**：
1. **ping 测试**：`ping 192.168.1.10` 确认网络可达
2. **端口检查**：被控端是否在监听 TCP 25605 端口
   - Windows: `netstat -an | findstr 25605`
   - macOS/Linux: `lsof -i :25605`
3. **防火墙**：
   - Windows: 控制面板 → Windows 防火墙 → 允许应用 → 添加 LAN-Desk
   - macOS: 系统设置 → 网络 → 防火墙 → 允许 LAN-Desk
   - Linux: `sudo ufw allow 25605/tcp && sudo ufw allow 25606/udp`

### TLS 握手失败

**症状**：提示 "TLS 握手失败"。

**原因**：通常是网络中间设备（如某些企业防火墙）拦截了 TLS 流量。

**解决**：确认两台电脑之间没有 SSL 检查设备，或使用 Tailscale 等 VPN 绕过。

### PIN 验证失败

**症状**：提示 "密码错误"。

**排查**：
- 确认输入的是被控端当前显示的 PIN（PIN 每次启动会刷新）
- 如果开启了固定密码模式，确认输入的是设置中配置的密码
- 控制密码和查看密码是不同的，注意区分

### 连续输错密码后无法连接

**症状**：之前能连接，但突然提示连接失败或超时。

**原因**：连续 5 次 PIN 验证失败后，该 IP 会被锁定 5 分钟，后续失败锁定时间指数递增（15分钟→45分钟→…→最长24小时），并设有全局速率限制（每分钟最多 10 次失败）。请等待锁定时间结束后重试，或检查 PIN 是否输入正确。

## 画面问题

### 黑屏 / 无画面

**Windows**：
- 如果在锁屏、UAC 弹窗后黑屏：程序会自动恢复（DXGI auto-recovery），等待 1-2 秒
- 如果持续黑屏：尝试重新连接

**macOS**：
- LAN-Desk 会自动检测屏幕录制和辅助功能权限，如未授权会弹出引导提示
- 需要授权 系统设置 → 隐私与安全 → **屏幕录制** 权限
- 授权后需要重启 LAN-Desk

**Linux**：
- 确认运行在 X11 环境下（`echo $XDG_SESSION_TYPE` 应输出 `x11`）
- Wayland 环境需要 XWayland 支持（大多数桌面默认启用）
- Wayland 原生截图工具：`grim`（推荐）或 `spectacle`（KDE Plasma）
- Wayland 原生输入注入：`ydotool`（推荐）或 `dotool`（不需要 root 权限）
- PipeWire/Portal 捕获（首选，无需外部工具）：需要系统安装 `libpipewire-0.3`，首次使用会弹出系统授权对话框

### Wayland PipeWire/Portal 捕获问题

**症状**：日志显示 PipeWire 不可用，降级到 grim/spectacle 或 X11。

**排查步骤**：
1. **确认 PipeWire 已安装**：
   ```bash
   # Debian/Ubuntu
   dpkg -l | grep libpipewire
   # 应显示 libpipewire-0.3-0 或类似包

   # Fedora
   rpm -qa | grep pipewire
   ```
   未安装时：`sudo apt-get install libpipewire-0.3-0`（Debian/Ubuntu）或 `sudo dnf install pipewire`（Fedora）

2. **确认 XDG Desktop Portal 服务运行中**：
   ```bash
   systemctl --user status xdg-desktop-portal
   # 应显示 active (running)
   ```

3. **确认合成器支持 ScreenCast Portal**：
   ```bash
   busctl --user call org.freedesktop.portal.Desktop \
     /org/freedesktop/portal/desktop \
     org.freedesktop.DBus.Properties Get ss \
     org.freedesktop.portal.ScreenCast version
   ```
   应返回版本号。如报错说明桌面环境未正确配置 Portal 后端。

4. **Portal 授权失败/被拒绝**：
   - 首次使用时系统弹出授权对话框，如果被关闭或拒绝，PipeWire 捕获会失败
   - 删除缓存的 restore_token 并重试（程序会重新弹出授权对话框）
   - 部分合成器（如 Sway）需要安装 `xdg-desktop-portal-wlr`；GNOME 需要 `xdg-desktop-portal-gnome`；KDE 需要 `xdg-desktop-portal-kde`

5. **PipeWire 版本过低**：建议使用 PipeWire 0.3.x 及以上版本。Ubuntu 22.04+ / Fedora 35+ 默认满足要求。

### 画面模糊 / 色块

- 进入 设置 → 提高 JPEG 画质（如 75 → 90）
- 如果使用 H.264 编码（日志中显示 "H.264"），画面应比 JPEG 更清晰

## 输入问题

### 键鼠不生效

**macOS**：需要授权 系统设置 → 隐私与安全 → **辅助功能** 权限。

**Linux**：确认 XTest 扩展可用：
```bash
xdpyinfo | grep -i test
# 应输出 XTEST
```

### 部分键位不生效

某些特殊键（如 PrintScreen、Pause）可能不在映射表中。请在 GitHub 提交 Issue 说明具体键位。

## 音频问题

### 无法听到远程声音

**macOS**：系统不支持直接捕获音频输出。需要：
1. 安装 [BlackHole](https://github.com/ExistentialAudio/BlackHole)（推荐 2ch 版本）
2. 系统设置 → 声音 → 输出 → 选择 BlackHole
3. 重启 LAN-Desk

**Linux**：确认 PulseAudio 或 PipeWire 音频服务正在运行。

## GPU 编码

### 如何确认是否使用了 GPU 编码？

查看程序启动日志：
- `使用 NVENC GPU 编码器` — NVIDIA GPU 硬件编码
- `使用 VideoToolbox GPU 编码器` — macOS 硬件编码
- `使用 OpenH264 软编码器` — CPU 软编码（回退）

### NVENC 不可用

- 确认已安装 NVIDIA 显卡驱动（版本 ≥ 471.41）
- 确认显卡支持 NVENC（GTX 600 系列及以上）
- 程序会自动回退到 OpenH264 软编码，不影响使用

## 远程终端问题

### 终端无法打开 / 点击无反应

- 确认使用的是**控制密码**连接（查看密码不支持终端功能）
- 检查被控端日志是否有 PTY 相关错误（设置 `RUST_LOG=debug`）

### 终端打开后无输出 / 卡住

**Windows**：
- 确认被控端系统上安装了 PowerShell（通常默认自带）
- 检查是否有安全软件拦截了进程创建

**macOS/Linux**：
- 确认用户有合法的登录 Shell：`echo $SHELL`
- 检查 `/etc/shells` 中是否包含该 Shell 路径

### 终端自动断开

**症状**：终端使用过程中突然关闭。

**原因**：Shell 会话有 30 分钟空闲超时，无操作将自动关闭。

**解决**：重新打开终端即可。保持终端活跃可避免超时断开。

### 终端显示乱码

- 确认被控端 Shell 的字符编码设置为 UTF-8
- Windows 用户可尝试在 PowerShell 中执行 `chcp 65001` 切换到 UTF-8 代码页

## TOFU 证书问题

### 连接提示证书指纹变化

**症状**：连接远程主机时提示证书指纹与之前不一致。

**原因**：被控端重新生成了 TLS 证书（如重装系统或删除了证书文件）。

**解决**：在设置中撤销旧主机指纹后重新连接即可自动信任新证书。

## 日志查看

设置环境变量 `RUST_LOG=debug` 可查看详细日志：

```bash
# Windows (PowerShell)
$env:RUST_LOG="debug"; .\LAN-Desk.exe

# macOS / Linux
RUST_LOG=debug ./lan-desk
```

---

<a id="troubleshooting-english"></a>

# Troubleshooting (English)

## Connection Issues

### Connection timeout / refused

1. **Ping test**: `ping <host_ip>` to confirm network reachability
2. **Port check**: Verify host is listening on TCP 25605
3. **Firewall**: Allow TCP 25605 and UDP 25606

### TLS handshake failed

Usually caused by network middleboxes intercepting TLS. Use Tailscale to bypass.

### PIN verification failed

- Verify you're entering the currently displayed PIN (refreshes on each startup)
- Control PIN and View PIN are different — check which one you're using

### Locked out after multiple wrong PINs

After 5 consecutive failed PIN attempts, the IP is locked for 5 minutes. Subsequent failures increase the lockout exponentially (15 min → 45 min → … → up to 24 hours), with a global rate limit (max 10 failures per minute). Wait for the lockout period to expire and try again, or verify you're entering the correct PIN.

## Display Issues

### Black screen

- **Windows**: Auto-recovers after lock screen/UAC (DXGI auto-recovery)
- **macOS**: LAN-Desk automatically detects permissions and shows a guided prompt if not yet granted. Grant Screen Recording permission, then restart app
- **Linux**: Ensure running under X11 (`echo $XDG_SESSION_TYPE`). For Wayland: native screenshot via `grim` (recommended) or `spectacle` (KDE Plasma); native input injection via `ydotool` (recommended) or `dotool` (no root required); PipeWire/Portal capture (preferred, no external tools needed): requires `libpipewire-0.3` installed, system authorization dialog on first use

### Wayland PipeWire/Portal Capture Issues

**Symptom**: Log shows PipeWire unavailable, falling back to grim/spectacle or X11.

**Troubleshooting steps**:
1. **Verify PipeWire is installed**:
   ```bash
   # Debian/Ubuntu
   dpkg -l | grep libpipewire
   # Should show libpipewire-0.3-0 or similar

   # Fedora
   rpm -qa | grep pipewire
   ```
   If not installed: `sudo apt-get install libpipewire-0.3-0` (Debian/Ubuntu) or `sudo dnf install pipewire` (Fedora)

2. **Verify XDG Desktop Portal service is running**:
   ```bash
   systemctl --user status xdg-desktop-portal
   # Should show active (running)
   ```

3. **Verify compositor supports ScreenCast Portal**:
   ```bash
   busctl --user call org.freedesktop.portal.Desktop \
     /org/freedesktop/portal/desktop \
     org.freedesktop.DBus.Properties Get ss \
     org.freedesktop.portal.ScreenCast version
   ```
   Should return a version number. An error means the desktop environment has not properly configured the Portal backend.

4. **Portal authorization failed/denied**:
   - On first use, the system shows an authorization dialog. If closed or denied, PipeWire capture will fail
   - Delete the cached restore_token and retry (the program will show the authorization dialog again)
   - Some compositors (e.g., Sway) require `xdg-desktop-portal-wlr`; GNOME requires `xdg-desktop-portal-gnome`; KDE requires `xdg-desktop-portal-kde`

5. **PipeWire version too old**: PipeWire 0.3.x or later is recommended. Ubuntu 22.04+ / Fedora 35+ meet this requirement by default.

### Blurry image / blocky artifacts

- Go to Settings → increase JPEG quality (e.g., 75 → 90)
- If using H.264 encoding (log shows "H.264"), the image should be clearer than JPEG

## Input Issues

### Keyboard/mouse not working

- **macOS**: Grant Accessibility permission
- **Linux**: Verify XTest extension: `xdpyinfo | grep -i test`

### Some keys not working

Certain special keys (e.g., PrintScreen, Pause) may not be in the key mapping table. Please submit a GitHub Issue specifying the exact key.

## Audio Issues

### No remote audio

- **macOS**: Install [BlackHole](https://github.com/ExistentialAudio/BlackHole) virtual audio device
- **Linux**: Ensure PulseAudio/PipeWire is running

## GPU Encoding

Check startup log for encoder type:
- `NVENC GPU` — NVIDIA hardware encoding
- `VideoToolbox GPU` — macOS hardware encoding
- `OpenH264` — CPU software encoding (fallback)

NVENC requires NVIDIA driver ≥ 471.41 and GTX 600+ GPU.

## Remote Terminal Issues

### Terminal won't open / no response on click

- Ensure you connected with the **Control PIN** (View PIN does not support terminal)
- Check host logs for PTY-related errors (`RUST_LOG=debug`)

### Terminal opens but no output / hangs

- **Windows**: Verify PowerShell is installed (usually pre-installed)
- **macOS/Linux**: Verify the user has a valid login shell: `echo $SHELL`

### Terminal disconnects automatically

**Symptom**: Terminal closes unexpectedly during use.

**Cause**: Shell sessions have a 30-minute idle timeout and will close automatically when inactive.

**Solution**: Reopen the terminal. Keeping the terminal active prevents timeout disconnection.

### Terminal shows garbled characters

- Ensure the host shell encoding is UTF-8
- Windows users: try `chcp 65001` in PowerShell to switch to UTF-8 code page

## TOFU Certificate Issues

### Connection warns about certificate fingerprint change

**Symptom**: When connecting to a remote host, a warning indicates the certificate fingerprint has changed.

**Cause**: The host has regenerated its TLS certificate (e.g., after reinstalling the OS or deleting the certificate file).

**Solution**: Revoke the old host fingerprint in Settings, then reconnect to automatically trust the new certificate.

## Debug Logs

```bash
RUST_LOG=debug ./lan-desk
```
