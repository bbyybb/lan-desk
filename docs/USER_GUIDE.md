**中文** | [English](#user-guide-english)

# LAN-Desk 用户手册

---

## 1. 安装 

### 1.1 桌面端

前往 [Releases](https://github.com/bbyybb/lan-desk/releases/latest) 下载对应平台的安装包：

| 平台 | 文件 | 安装方式 |
|------|------|----------|
| Windows | `.msi` 或 `_x64-setup.exe` | 双击安装 |
| macOS (Apple Silicon) | `_aarch64.dmg` | 拖入 Applications |
| macOS (Intel) | `_x64.dmg` | 拖入 Applications |
| Linux (Debian/Ubuntu) | `.deb` | `sudo dpkg -i lan-desk_*.deb` |
| Linux (通用) | `.AppImage` | `chmod +x *.AppImage && ./*.AppImage` |

**免安装使用（便携模式）：** 将 `lan-desk.exe` 放到任意文件夹，在同目录下创建一个名为 `.portable` 的空文件，双击即可运行。数据存储在同目录的 `data/` 文件夹中，适合 U 盘携带。

### 1.2 移动端

移动端仅支持**控制端**（用手机/平板控制电脑），不支持作为被控端。

- **Android**：下载 `.apk` 文件，在手机设置中允许「安装未知应用」，然后安装。
- **iOS**：下载 `.ipa` 文件，通过 AltStore 或 TrollStore 侧载安装。

### 1.3 平台特殊说明

- **macOS**：首次使用需在「系统设置 → 隐私与安全性」中授予「屏幕录制」和「辅助功能」权限。
- **Linux**：需安装 X11 或 Wayland 相关依赖。Wayland 环境推荐安装 `grim`（截图）和 `ydotool`（输入注入）。
- **防火墙**：需放行 TCP 25605 和 UDP 25606 端口。

---

## 2. 快速开始

### 2.1 被控端（要被远程控制的电脑）

1. 启动 LAN-Desk，主页显示 **8 位 PIN 码**（控制密码和查看密码）
2. 将 PIN 码告知控制端用户

### 2.2 控制端（发起远程控制的设备）

1. 启动 LAN-Desk，在「快速连接」区域输入被控端 IP 地址（或 9 位设备 ID）
2. 输入被控端的 PIN 码
3. 选择连接角色：「完全控制」或「仅查看」
4. 点击「连接」，被控端确认授权后即可开始远程控制

---

## 3. 界面说明

### 3.1 设备发现页（主页）

| 区域 | 功能 |
|------|------|
| 顶部状态栏 | 显示服务器运行状态和端口号，右侧为「设置」入口 |
| 设备 ID | 9 位数字唯一标识，点击可复制 |
| 本机地址 | 显示所有网络接口 IP，VPN 地址（Tailscale/ZeroTier）会高亮标记 |
| 谁连接了我 | 显示当前连入的远程会话（主机名、IP、角色） |
| 我正在控制 | 显示你正在控制的远程设备，点击可切换标签 |
| PIN 码区域 | 显示控制密码（大号）和查看密码（小号），可刷新和复制 |
| 快速连接 | IP/设备 ID 输入框 + 密码 + 角色选择 + 连接按钮 |
| 局域网设备 | 点击「扫描」发现同网段设备，点击设备卡片可快速填充连接信息 |
| 远程唤醒 | 输入 MAC 地址发送 Wake-on-LAN 魔术包 |
| 连接历史 | 最近 10 条连接记录，支持编辑别名、移除、导出 CSV |

### 3.2 远程桌面界面

| 区域 | 功能 |
|------|------|
| 顶部状态栏 | 断开按钮、远程地址、TLS 标志、FPS/延迟/带宽/RTT 等实时统计 |
| 主画面 | 远程桌面画面，支持鼠标和键盘操作 |
| 右侧工具面板 | 可折叠，包含所有工具按钮（详见 §4） |
| 重连提示 | 断线时自动显示重连状态（支持远程重启后自动重连） |

### 3.3 设置页面

详见 [§9 设置项参考](#9-设置项参考)。

### 3.4 系统托盘

关闭窗口后 LAN-Desk 会最小化到系统托盘继续运行。便携模式下托盘图标旁会显示 `[P]` 标记。

右键托盘图标：
- **打开 LAN-Desk**：恢复主窗口
- **服务运行中**：状态指示（不可点击）
- **退出**：完全退出应用

---

## 4. 核心功能

### 4.1 远程桌面控制

连接成功后，你可以直接用鼠标和键盘操控远程电脑。

- **画面缩放模式**：工具面板中可切换「适应窗口」/「原始大小」/「拉伸填充」三种模式
- **全屏**：点击工具面板的「全屏」按钮或使用浏览器全屏快捷键
- **编码器**：状态栏右侧显示当前编码器类型（H.264 / HEVC / JPEG），系统自动选择最佳 GPU 硬件编码器，无需手动配置
- **自适应画质**：系统根据网络状况和屏幕变化自动调整帧率（5-60fps）和画质，大面积变化时降低画质保帧率，静态画面时提升画质

### 4.2 文件传输

点击工具面板的「文件」按钮打开文件浏览器（仅完全控制模式可用）。

**界面布局：**
- **左栏**：本地操作区 — 「上传文件」和「上传目录」按钮，下方显示传输进度
- **右栏**：远程文件列表 — 面包屑路径导航，文件/目录列表，每个文件有「下载」按钮

**操作方法：**
- **上传文件**：点击「上传文件」按钮选择文件，或直接将文件拖拽到远程桌面画面上
- **上传目录**：点击「上传目录」按钮选择整个目录
- **下载文件**：在远程文件列表中点击文件右侧的「下载」按钮
- **导航目录**：双击目录进入，点击 `..` 或面包屑路径返回上级
- **取消传输**：在传输进度列表中点击「取消」按钮
- **断点续传**：传输中断后重新发起，已完成的部分会自动跳过

**限制：** 单文件最大 2GB，最多 5 个并发传输，传输完成后自动进行 SHA-256 校验。

### 4.3 远程终端

点击工具面板的「终端」按钮打开远程 Shell（仅完全控制模式可用，需在设置中开启「允许远程终端」）。

- 底部面板形式，可拖拽上边缘调整高度
- Windows 远程端使用 PowerShell（自动 UTF-8 编码），Linux/macOS 使用系统默认 Shell
- 空闲 30 分钟自动关闭（可在设置中调整）
- 点击右上角 × 关闭终端

### 4.4 文字聊天

点击工具面板的「聊天」按钮打开聊天面板（仅完全控制模式可用）。

- 右侧面板形式，气泡消息样式
- 按 Enter 发送消息，Shift+Enter 换行
- 对方不在前台时会播放提示音通知
- 被控端也可以发送消息给所有控制端

### 4.5 会话录制与回放

点击工具面板的「录制」按钮开始录制远程画面。

- 录制格式为 WebM（VP9 编码）
- 再次点击「停止录制」结束录制
- 录制完成后自动保存，点击「录制历史」查看所有录制文件
- 录制历史面板中可以：播放（内置播放器）、下载（保存为 .webm 文件）、删除
- 录制数据存储在浏览器 IndexedDB 中

### 4.6 白板标注

点击工具面板的「标注」按钮开启标注模式（仅完全控制模式可用）。

开启后鼠标操作变为画笔绘制，不再控制远程电脑。

- **画笔工具**：自由绘制线条
- **文字工具**：点击画面输入文字
- **颜色选择**：点击取色器选择画笔颜色
- **线宽**：细(1px) / 中(3px) / 粗(6px)
- **撤销**：撤销上一步操作
- **清除标注**：清除所有标注内容
- 关闭标注模式后恢复正常远程控制

### 4.7 音频转发

连接后远程电脑的音频会自动转发到本地播放。

- 点击工具面板的「静音/声音」按钮开关音频
- 音频质量在设置中可选：低(64kbps) / 中(128kbps) / 高(256kbps)
- 支持 Opus 压缩编码（带宽仅需 ~128kbps），自动降级为 PCM

**各平台音频配置：**
- **Windows**：开箱即用，自动捕获系统音频
- **macOS**：需安装虚拟音频设备（如 [BlackHole](https://github.com/ExistentialAudio/BlackHole)），将系统输出路由到虚拟设备
- **Linux**：需要 PulseAudio 或 PipeWire 音频服务

### 4.8 远程截图

点击工具面板的「截图保存」按钮，将当前远程画面保存为 PNG 图片。文件会自动下载到本地。

---

## 5. 连接管理

### 5.1 连接方式

LAN-Desk 支持三种连接方式：

| 方式 | 操作 | 适用场景 |
|------|------|----------|
| IP 地址连接 | 在连接框输入被控端 IP（如 `192.168.1.100`） | 知道对方 IP 时 |
| 设备 ID 连接 | 输入 9 位设备 ID（如 `123456789`） | 不知道 IP，但在同一局域网 |
| 局域网扫描 | 点击「扫描」按钮，点击设备卡片 | 快速发现同网段设备 |

设备 ID 查看位置：主页顶部显示本机的 9 位设备 ID，点击可复制。

### 5.2 连接历史与别名

- 主页底部显示最近 10 条连接记录
- 点击连接记录可快速填充 IP 和密码（如果勾选了「记住密码」）
- 点击编辑图标可为连接设置自定义别名
- 点击「导出 CSV」将所有连接历史导出为 CSV 文件

### 5.3 Wake-on-LAN 远程唤醒

在主页底部的「远程唤醒」区域输入被控端的 MAC 地址，点击「唤醒」发送魔术包。

**前提条件：**
1. 被控端电脑的 BIOS 中需开启「Wake on LAN」选项
2. 被控端网卡驱动中需开启「允许此设备唤醒计算机」
3. 两台电脑需在同一局域网内
4. MAC 地址格式：`AA:BB:CC:DD:EE:FF` 或 `AA-BB-CC-DD-EE-FF`

**获取 MAC 地址：** Windows 运行 `ipconfig /all`，Linux/macOS 运行 `ip link` 或 `ifconfig`。

### 5.4 多标签/多会话

LAN-Desk 支持同时连接多台远程设备：

- 每次连接会创建一个新标签页
- 在主页的「我正在控制」区域点击不同连接可切换标签
- 关闭标签页会断开对应的远程连接

### 5.5 断线自动重连

网络中断时 LAN-Desk 会自动尝试重新连接：

- 状态栏显示「正在重新连接... (第 N 次)」
- 重连间隔采用指数退避策略（1秒 → 2秒 → 4秒 → ...）
- 可在设置中配置最大重试次数（默认 5 次）
- 远程电脑重启后也会自动重连（状态栏显示「远程计算机正在重启」）

---

## 6. 多显示器

如果远程电脑有多个显示器，工具面板中会出现「显示器」选择区：

- 每个显示器显示为一个按钮，标注分辨率
- 主显示器用 ★ 标记
- 点击切换要查看/控制的显示器
- 鼠标坐标会自动映射到当前选中的显示器

---

## 7. 高级功能

### 7.1 无人值守模式

适合需要随时远程访问的场景（如远程办公电脑）。

**配置步骤：**
1. 进入「设置 → 无人值守」
2. 开启「自动接受连接」
3. 开启「使用固定密码」
4. 设置控制密码和查看密码（6-20 位）
5. 建议同时开启「开机自启动」

**安全建议：** 使用强密码、开启「断开时自动锁屏」、限制空闲超时时间。

### 7.2 便携模式

无需安装，适合 U 盘携带使用。

1. 将 `lan-desk.exe` 放到任意文件夹
2. 在同目录创建名为 `.portable` 的空文件（Windows 可用 `echo.> .portable` 命令）
3. 双击运行，所有数据（TLS 证书、配置）保存在同目录的 `data/` 文件夹中
4. 系统托盘显示 `[P]` 标记表示正在便携模式运行

### 7.3 开机自启动

在「设置 → 通用」中开启「开机自启动」，LAN-Desk 会在系统登录后自动启动并最小化到托盘。

### 7.4 连接统计导出

在主页「连接历史」区域点击「导出 CSV」，导出内容包含：主机名、IP 地址、端口、角色、别名、连接时间。

### 7.5 屏幕遮蔽

在工具面板的「远程控制」区域点击「遮蔽屏幕」，可关闭远程电脑的显示器，防止旁人窥视远程操作。再次点击「取消遮蔽」恢复显示。

### 7.6 特殊按键

工具面板的「远程控制」区域提供以下特殊按键：

| 按钮 | 功能 |
|------|------|
| Ctrl+Alt+Del | 发送安全注意序列（需系统权限） |
| Alt+Tab | 切换窗口 |
| Alt+F4 | 关闭当前窗口 |
| Win | 打开开始菜单 |
| Win+L | 锁定屏幕 |
| PrtSc | 截屏 |
| Ctrl+Esc | 打开开始菜单（替代 Win 键） |

### 7.7 远程重启

点击工具面板的「远程重启」按钮（红色），系统会弹出二次确认对话框。确认后远程电脑将在 10 秒后重启。如果开启了自动重连，LAN-Desk 会在重启后自动恢复连接。

---

## 8. 安全设置

### 8.1 双 PIN 权限控制

- **控制密码**：允许完全操控（键鼠、文件传输、剪贴板、终端）
- **查看密码**：仅允许观看屏幕
- 每次启动随机生成 8 位数字 PIN，点击主页刷新按钮可更换
- 固定密码模式下使用用户自定义密码（6-20 位）

### 8.2 TOFU 证书管理

首次连接远程主机时，LAN-Desk 会记录其 TLS 证书指纹。后续连接时若指纹不匹配（可能是中间人攻击），连接会被拒绝。

- 在「设置 → 已信任主机」中可查看所有已信任的主机和证书指纹
- 点击「移除」可撤销对某台主机的信任（下次连接需重新确认）

### 8.3 断开锁屏 / 屏幕遮蔽

- **断开时自动锁屏**：在「设置 → 安全」中开启，远程会话断开时自动锁定被控端屏幕
- **屏幕遮蔽**：在远程控制过程中手动开关，关闭远程电脑显示器防窥

### 8.4 安全最佳实践

- 不使用时关闭 LAN-Desk 或使用空闲超时自动断开
- 使用固定密码时设置强密码，避免纯数字
- 开启「断开时自动锁屏」
- 定期在「已信任主机」中检查和清理不认识的主机
- 远程终端默认关闭，仅在需要时开启

---

## 9. 设置项参考

### 网络

| 设置项 | 说明 | 默认值 | 范围 |
|--------|------|--------|------|
| TCP 端口 | 远程控制监听端口 | 25605 | 1024-65535 |

### 画质

| 设置项 | 说明 | 默认值 | 范围 |
|--------|------|--------|------|
| JPEG 画质 | 静态画面编码质量 | 75 | 20-95 |
| 最大帧率 | 画面刷新上限 | 30 fps | 5-60 fps |
| 音频质量 | Opus 编码码率 | 中(128kbps) | 低/中/高 |

### 连接

| 设置项 | 说明 | 默认值 |
|--------|------|--------|
| 断线自动重连 | 网络中断后自动重连 | 开 |
| 最大重试次数 | 自动重连尝试次数 | 5 次 |

### 通用

| 设置项 | 说明 | 默认值 |
|--------|------|--------|
| 带宽限制 | 最大传输带宽（0=不限） | 0 Mbps |
| 剪贴板同步 | 自动双向同步剪贴板（文本+图片） | 开 |
| 语言 | 界面语言 | 自动检测 |
| 主题 | 深色/浅色/跟随系统 | 深色 |
| 开机自启动 | 登录后自动启动 | 关 |

### 安全

| 设置项 | 说明 | 默认值 |
|--------|------|--------|
| 允许远程终端 | 是否允许远程 Shell 访问 | 关 |
| 空闲超时 | 无操作自动断开（0=不限） | 30 分钟 |
| 断开时自动锁屏 | 会话断开后锁定屏幕 | 关 |

### 无人值守

| 设置项 | 说明 | 默认值 |
|--------|------|--------|
| 自动接受连接 | 跳过授权弹窗 | 关 |
| 使用固定密码 | 使用自定义密码代替随机 PIN | 关 |
| 控制密码 / 查看密码 | 自定义密码（6-20 位） | 空 |

---

## 10. 快捷键参考

### 远程桌面界面

| 快捷键 | 功能 |
|--------|------|
| F1 | 打开/关闭快捷键帮助面板 |
| 所有键盘按键 | 控制模式下透传到远程电脑 |
| 鼠标操作 | 控制模式下透传到远程电脑 |
| 滚轮 | 控制模式下透传到远程电脑 |

### 聊天面板

| 快捷键 | 功能 |
|--------|------|
| Enter | 发送消息 |
| Shift+Enter | 换行（不发送） |

### 移动端手势

| 手势 | 功能 |
|------|------|
| 单指移动 | 移动鼠标 |
| 单击 | 左键点击 |
| 双击 | 左键双击 |
| 长按 | 右键点击 |
| 双指滚动 | 滚轮滚动 |

---

## 11. 移动端使用

移动端作为**仅控制端**使用，不支持被控。

- 连接流程与桌面端相同：输入 IP/设备 ID + PIN 码
- 使用触屏手势操作远程电脑（见上方手势表）
- 底部有悬浮虚拟键盘按钮，提供 Esc、Tab、Enter、方向键等常用按键
- 界面自动适应手机/平板屏幕尺寸

---

## 12. 跨网络使用

默认情况下 LAN-Desk 仅在同一局域网内使用。如需跨网络远程控制，需借助内网穿透工具：

- **Tailscale**（推荐）：两端安装并登录同一账号，使用 Tailscale 分配的 IP 连接
- **ZeroTier**：类似 Tailscale 的虚拟局域网方案
- **frp / Cloudflare Tunnel**：自建穿透方案

详细配置见 [跨网络远程访问指南](REMOTE_ACCESS.md)。

---
---

<a id="user-guide-english"></a>

# LAN-Desk User Guide

---

## 1. Installation

### 1.1 Desktop

Download the installer for your platform from [Releases](https://github.com/bbyybb/lan-desk/releases/latest):

| Platform | File | Install |
|----------|------|---------|
| Windows | `.msi` or `_x64-setup.exe` | Double-click to install |
| macOS (Apple Silicon) | `_aarch64.dmg` | Drag to Applications |
| macOS (Intel) | `_x64.dmg` | Drag to Applications |
| Linux (Debian/Ubuntu) | `.deb` | `sudo dpkg -i lan-desk_*.deb` |
| Linux (Universal) | `.AppImage` | `chmod +x *.AppImage && ./*.AppImage` |

**Portable mode (no install):** Place `lan-desk.exe` in any folder, create an empty file named `.portable` in the same directory, then run. Data is stored in a `data/` subfolder — ideal for USB drives.

### 1.2 Mobile

Mobile devices are supported as **controller only** (remote control a computer from your phone/tablet).

- **Android**: Download the `.apk` file, allow "Install unknown apps" in settings, then install.
- **iOS**: Download the `.ipa` file, sideload via AltStore or TrollStore.

### 1.3 Platform Notes

- **macOS**: Grant "Screen Recording" and "Accessibility" permissions in System Settings → Privacy & Security on first use.
- **Linux**: Install X11 or Wayland dependencies. For Wayland, install `grim` (screenshot) and `ydotool` (input injection).
- **Firewall**: Allow TCP 25605 and UDP 25606.

---

## 2. Quick Start

### 2.1 Host (computer to be controlled)

1. Launch LAN-Desk — the main page displays an **8-digit PIN** (control + view passwords)
2. Share the PIN with the controller

### 2.2 Controller (device initiating remote control)

1. Launch LAN-Desk, enter the host's IP address (or 9-digit Device ID) in the Quick Connect area
2. Enter the host's PIN
3. Choose role: "Full Control" or "View Only"
4. Click "Connect" — after the host approves, remote control begins

---

## 3. Interface Overview

### 3.1 Discovery Page (Home)

| Area | Function |
|------|----------|
| Top status bar | Server status and port number; "Settings" button on the right |
| Device ID | 9-digit unique identifier, click to copy |
| Local addresses | All network interface IPs; VPN addresses (Tailscale/ZeroTier) are highlighted |
| Connected clients | Inbound remote sessions (hostname, IP, role) |
| Active sessions | Outbound sessions you are controlling, click to switch tabs |
| PIN area | Control PIN (large) and View PIN (small), with refresh and copy buttons |
| Quick Connect | IP/Device ID input + password + role selector + Connect button |
| LAN devices | Click "Scan" to discover devices on the same subnet |
| Wake-on-LAN | Enter MAC address to send a magic packet |
| Connection history | Last 10 connections; supports aliases, removal, and CSV export |

### 3.2 Remote Desktop

| Area | Function |
|------|----------|
| Top status bar | Disconnect button, remote address, TLS badge, FPS/latency/bandwidth/RTT stats |
| Main canvas | Remote desktop display with mouse and keyboard control |
| Right tool panel | Collapsible panel with all tool buttons (see §4) |
| Reconnect banner | Auto-reconnect status during disconnections |

### 3.3 Settings

See [§9 Settings Reference](#9-settings-reference-1).

### 3.4 System Tray

Closing the window minimizes LAN-Desk to the system tray. Portable mode shows a `[P]` badge.

Right-click the tray icon:
- **Open LAN-Desk**: Restore the main window
- **Running**: Status indicator (not clickable)
- **Quit**: Fully exit the application

---

## 4. Core Features

### 4.1 Remote Desktop Control

After connecting, use your mouse and keyboard to control the remote computer.

- **Scaling modes**: Switch between "Fit Window" / "Original Size" / "Stretch" in the tool panel
- **Full screen**: Click the full screen button in the tool panel
- **Encoder**: The status bar shows the active encoder (H.264 / HEVC / JPEG) — the system automatically selects the best available GPU encoder
- **Adaptive quality**: The system dynamically adjusts framerate (5-60fps) and quality based on network conditions

### 4.2 File Transfer

Click "File" in the tool panel to open the file browser (Full Control mode only).

**Layout:**
- **Left pane**: Local operations — "Upload File" and "Upload Directory" buttons, transfer progress list below
- **Right pane**: Remote file list — breadcrumb path navigation, files/directories with "Download" buttons

**Operations:**
- **Upload file**: Click "Upload File" or drag-and-drop files onto the remote desktop
- **Upload directory**: Click "Upload Directory" to select an entire directory
- **Download**: Click the "Download" button next to any remote file
- **Navigate**: Double-click directories to enter; click `..` or breadcrumbs to go back
- **Cancel**: Click "Cancel" in the transfer progress list
- **Resume**: Interrupted transfers automatically skip completed portions when retried

**Limits:** Max 2GB per file, max 5 concurrent transfers, SHA-256 verification on completion.

### 4.3 Remote Terminal

Click "Terminal" in the tool panel (Full Control mode; must enable "Allow Remote Terminal" in Settings).

- Appears as a bottom panel, drag the top edge to resize
- Windows hosts use PowerShell (auto UTF-8); Linux/macOS use the default shell
- Auto-closes after 30 minutes of inactivity (configurable)

### 4.4 Text Chat

Click "Chat" in the tool panel (Full Control mode only).

- Side panel with bubble-style messages
- Press Enter to send, Shift+Enter for a new line
- Audio notification when receiving messages while the panel is hidden
- The host can also send messages to all controllers

### 4.5 Session Recording & Playback

Click "Record" in the tool panel to start recording.

- Format: WebM (VP9 codec)
- Click "Stop Recording" to finish
- Click "Recording History" to view all recordings
- In the history panel: Play (built-in player), Download (.webm file), or Delete
- Recordings are stored in the browser's IndexedDB

### 4.6 Whiteboard Annotation

Click "Annotate" in the tool panel (Full Control mode only). Mouse actions switch to drawing instead of remote control.

- **Brush tool**: Freehand drawing
- **Text tool**: Click to place text
- **Color picker**: Choose brush color
- **Line width**: Thin (1px) / Medium (3px) / Thick (6px)
- **Undo**: Undo the last action
- **Clear**: Remove all annotations
- Closing annotation mode returns to normal remote control

### 4.7 Audio Forwarding

Remote audio is automatically forwarded after connecting.

- Toggle audio with the Mute/Unmute button in the tool panel
- Audio quality options in Settings: Low (64kbps) / Medium (128kbps) / High (256kbps)
- Supports Opus compression (auto-fallback to PCM)

**Platform-specific setup:**
- **Windows**: Works out of the box
- **macOS**: Requires a virtual audio device (e.g., [BlackHole](https://github.com/ExistentialAudio/BlackHole))
- **Linux**: Requires PulseAudio or PipeWire

### 4.8 Remote Screenshot

Click "Screenshot" in the tool panel to save the current remote screen as a PNG file. The file downloads automatically.

---

## 5. Connection Management

### 5.1 Connection Methods

| Method | How | Use case |
|--------|-----|----------|
| IP address | Enter the host's IP (e.g., `192.168.1.100`) | When you know the IP |
| Device ID | Enter the 9-digit Device ID | Same LAN, but IP unknown |
| LAN scan | Click "Scan", then click a device card | Quick discovery |

Device ID location: Displayed at the top of the home page; click to copy.

### 5.2 Connection History & Aliases

- The home page shows the last 10 connections
- Click a record to auto-fill the IP and password (if "Remember password" was checked)
- Click the edit icon to set a custom alias
- Click "Export CSV" to export all connection history

### 5.3 Wake-on-LAN

Enter the host's MAC address in the "Wake-on-LAN" section and click "Wake".

**Prerequisites:**
1. Enable "Wake on LAN" in the host's BIOS
2. Enable "Allow this device to wake the computer" in the host's network adapter settings
3. Both devices must be on the same LAN
4. MAC format: `AA:BB:CC:DD:EE:FF` or `AA-BB-CC-DD-EE-FF`

**Finding MAC address:** Windows: `ipconfig /all`; Linux/macOS: `ip link` or `ifconfig`.

### 5.4 Multi-Tab / Multi-Session

- Each connection opens a new tab
- Switch between sessions in the "Active Sessions" area on the home page
- Closing a tab disconnects that session

### 5.5 Auto-Reconnect

When the connection drops, LAN-Desk automatically attempts to reconnect:

- Status bar shows "Reconnecting... (attempt N)"
- Exponential backoff intervals (1s → 2s → 4s → ...)
- Max retry count configurable in Settings (default: 5)
- Also reconnects after the remote computer restarts

---

## 6. Multi-Monitor

If the remote computer has multiple monitors, a monitor selector appears in the tool panel:

- Each monitor shown as a button with its resolution
- Primary monitor marked with ★
- Click to switch the target monitor
- Mouse coordinates automatically map to the selected monitor

---

## 7. Advanced Features

### 7.1 Unattended Mode

For computers that need to be accessible at all times.

1. Go to Settings → Unattended
2. Enable "Auto-accept connections"
3. Enable "Use fixed password"
4. Set control and view passwords (6-20 characters)
5. Recommended: also enable "Start on login"

**Security tip:** Use strong passwords, enable "Lock screen on disconnect", and set an idle timeout.

### 7.2 Portable Mode

1. Place `lan-desk.exe` in any folder
2. Create an empty `.portable` file in the same directory
3. Run — all data (TLS certs, config) is stored in `data/` alongside the executable
4. Tray icon shows `[P]` to indicate portable mode

### 7.3 Start on Login

Enable in Settings → General → "Start on login". LAN-Desk launches minimized to the tray after system login.

### 7.4 Export Connection Stats

Click "Export CSV" in the Connection History section. Exported fields: hostname, IP, port, role, alias, connection time.

### 7.5 Screen Blanking

Click "Blank Screen" in the Remote Control section of the tool panel to turn off the remote monitor (prevents bystanders from seeing the remote session). Click again to restore.

### 7.6 Special Keys

| Button | Function |
|--------|----------|
| Ctrl+Alt+Del | Send Secure Attention Sequence |
| Alt+Tab | Switch windows |
| Alt+F4 | Close current window |
| Win | Open Start menu |
| Win+L | Lock screen |
| PrtSc | Print Screen |
| Ctrl+Esc | Open Start menu (Win key alternative) |

### 7.7 Remote Reboot

Click "Remote Reboot" (red button) in the tool panel. A confirmation dialog appears. The remote computer restarts after 10 seconds. If auto-reconnect is enabled, LAN-Desk will automatically resume the session after reboot.

---

## 8. Security Settings

### 8.1 Dual-PIN Access Control

- **Control PIN**: Full access (keyboard, mouse, files, clipboard, terminal)
- **View PIN**: Screen viewing only
- Random 8-digit PINs generated on each launch; click refresh to regenerate
- Fixed password mode uses custom passwords (6-20 characters)

### 8.2 TOFU Certificate Management

On first connection, LAN-Desk records the host's TLS certificate fingerprint. Future connections reject mismatched fingerprints (possible MITM attack).

- View trusted hosts and fingerprints in Settings → Trusted Hosts
- Click "Remove" to revoke trust (next connection will require re-confirmation)

### 8.3 Lock on Disconnect / Screen Blanking

- **Lock on disconnect**: Enable in Settings → Security; automatically locks the host screen when the session ends
- **Screen blanking**: Manual toggle during a remote session to turn off the host's display

### 8.4 Security Best Practices

- Close LAN-Desk when not in use, or set an idle timeout
- Use strong passwords in fixed password mode
- Enable "Lock screen on disconnect"
- Periodically review and clean up trusted hosts
- Keep "Allow remote terminal" disabled unless needed

---

## 9. Settings Reference

### Network

| Setting | Description | Default | Range |
|---------|-------------|---------|-------|
| TCP Port | Remote control listen port | 25605 | 1024-65535 |

### Quality

| Setting | Description | Default | Range |
|---------|-------------|---------|-------|
| JPEG Quality | Static frame encoding quality | 75 | 20-95 |
| Max FPS | Frame rate cap | 30 | 5-60 |
| Audio Quality | Opus encoding bitrate | Medium (128kbps) | Low/Medium/High |

### Connection

| Setting | Description | Default |
|---------|-------------|---------|
| Auto-reconnect | Reconnect on network drop | On |
| Max retries | Auto-reconnect attempts | 5 |

### General

| Setting | Description | Default |
|---------|-------------|---------|
| Bandwidth limit | Max bandwidth (0 = unlimited) | 0 Mbps |
| Clipboard sync | Auto-sync clipboard (text + images) | On |
| Language | UI language | Auto-detect |
| Theme | Dark / Light / System | Dark |
| Start on login | Auto-start on login | Off |

### Security

| Setting | Description | Default |
|---------|-------------|---------|
| Allow remote terminal | Enable remote Shell access | Off |
| Idle timeout | Auto-disconnect after inactivity (0 = off) | 30 min |
| Lock on disconnect | Lock screen when session ends | Off |

### Unattended

| Setting | Description | Default |
|---------|-------------|---------|
| Auto-accept | Skip authorization dialog | Off |
| Fixed password | Use custom passwords | Off |
| Control / View password | Custom passwords (6-20 chars) | Empty |

---

## 10. Keyboard Shortcuts

### Remote Desktop

| Shortcut | Function |
|----------|----------|
| F1 | Toggle keyboard shortcut help panel |
| All keyboard keys | Forwarded to remote computer in control mode |
| Mouse actions | Forwarded to remote computer in control mode |
| Scroll wheel | Forwarded to remote computer in control mode |

### Chat Panel

| Shortcut | Function |
|----------|----------|
| Enter | Send message |
| Shift+Enter | New line (without sending) |

### Mobile Gestures

| Gesture | Function |
|---------|----------|
| Single finger move | Move mouse cursor |
| Single tap | Left click |
| Double tap | Double click |
| Long press | Right click |
| Two-finger scroll | Scroll wheel |

---

## 11. Mobile Usage

Mobile devices function as **controller only** — you cannot use a phone/tablet as a host.

- Connection flow is identical to desktop: enter IP/Device ID + PIN
- Use touch gestures to operate the remote computer (see gesture table above)
- A floating virtual keyboard button provides Esc, Tab, Enter, arrow keys, etc.
- The interface automatically adapts to phone/tablet screen sizes

---

## 12. Cross-Network Usage

By default, LAN-Desk works within the same LAN. For remote access across different networks, use a tunneling solution:

- **Tailscale** (recommended): Install on both devices, log into the same account, connect using the Tailscale IP
- **ZeroTier**: Similar virtual LAN solution
- **frp / Cloudflare Tunnel**: Self-hosted tunneling

See [Cross-Network Remote Access Guide](REMOTE_ACCESS.md) for detailed setup instructions.
