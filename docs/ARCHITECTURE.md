**中文** | [English](#architecture-english)

# 架构设计

本文档描述 LAN-Desk 的整体架构设计。

## 项目结构

LAN-Desk 采用 Rust + Vue 3 + Tauri 架构，后端由 7 个 crate 组成：

```
lan-desk/
├── src-tauri/            # Tauri 主应用（前后端桥接、窗口管理、IPC）
├── crates/
│   ├── lan-desk-protocol/    # 协议定义（消息类型、序列化/反序列化、TLS）
│   ├── lan-desk-capture/     # 屏幕捕获（DXGI/CGDisplay/XShm/PipeWire Portal/Wayland grim）+ GPU 编码（NVENC/VideoToolbox/VAAPI）
│   ├── lan-desk-input/       # 输入注入（SendInput/CGEvent/XTest/Wayland ydotool·wtype）
│   ├── lan-desk-clipboard/   # 剪贴板同步（跨平台剪贴板读写）
│   ├── lan-desk-audio/       # 音频捕获与播放（WASAPI/CoreAudio/PulseAudio）+ Opus 编解码
│   └── lan-desk-server/      # 被控端服务器（TCP/TLS 监听、会话管理、PTY 终端）
└── src/                  # Vue 3 前端（UI 界面）
```

### AppState 状态管理

AppState 按语义分为 3 个 Mutex 分组：AuthState（PIN 和认证）、ServerState（端口和运行状态）、ConnState（连接和 WebSocket）。

PIN 验证失败采用指数退避策略：连续失败 5 次后锁定 5 分钟，继续失败依次递增（15 分钟→1 小时→…→24 小时封顶），同时全局限流 10 次/分钟。

### Crate 依赖关系

```
src-tauri (主应用)
├── lan-desk-protocol     # 协议层（所有网络通信）
├── lan-desk-capture      # 屏幕捕获
├── lan-desk-input        # 输入注入
├── lan-desk-clipboard    # 剪贴板同步
├── lan-desk-audio        # 音频桥接
└── lan-desk-server       # 被控端服务器

lan-desk-server
└── lan-desk-protocol
```

## 数据流

### 传输通道

LAN-Desk 采用**双通道**架构传输数据：

| 通道 | 用途 | 说明 |
|------|------|------|
| **TLS TCP** | 控制指令、文件传输、终端 I/O | 低频事件，MessagePack 序列化 |
| **本地 WebSocket** | 帧数据、音频数据 | 127.0.0.1 本地 WebSocket 二进制推送，绕过 Tauri JSON IPC，消除 base64 ~33% 开销 |

高带宽数据（屏幕帧 + 音频流）通过本地 WebSocket 以二进制帧直接推送给前端，低频事件（键鼠、剪贴板、文件传输等）继续走 Tauri emit。

### WebSocket 安全说明

本地 WebSocket 桥接的安全模型基于三层防护：

1. **仅绑定 127.0.0.1** — WebSocket 服务器仅监听本地回环地址，不接受任何远程连接，消除了网络层面的攻击面。
2. **32 字节随机 Token 认证** — 连接建立后，客户端必须发送由 Rust 后端生成的 32 字节密码学安全随机 token，服务端验证后才开始推送数据。该 token 通过 Tauri IPC 传递给前端，IPC 通道仅限当前应用进程内部通信，外部进程无法获取。
3. **临时端口 + 端口验证** — WebSocket 监听端口由操作系统随机分配（临时端口），前端在连接前验证端口号处于合法范围（1024-65535）。

**关于 CSP 中的 `ws://127.0.0.1:*`**：由于端口在运行时随机分配，无法在静态 CSP 策略中指定具体端口号，因此 CSP 的 `connect-src` 使用通配端口。这不会降低安全性，因为实际连接受上述 token 认证保护。

### 远程桌面数据流

```
控制端 (Controller)                          被控端 (Host)
┌─────────────┐                        ┌─────────────────┐
│  Vue 3 UI   │                        │  屏幕捕获线程    │
│  (Canvas)   │◄── 本地 WebSocket ────│  (DXGI/CG/XShm/ │
│   grim)          │
│             │    帧/音频二进制       │       │          │
│             │◄── TLS 连接 ──────────│  GPU/软编码      │
│             │    FrameData           │       │          │
│  键鼠事件 ──│──► MouseMove/KeyEvent ─│──►输入注入线程   │
│             │    (TLS 加密)          │  (SendInput/etc) │
└─────────────┘                        └─────────────────┘
```

### 文件传输数据流

```
控制端选择文件 -> FileTransferRequest -> 被控端确认
              -> FileTransferData (分块) -> 写入磁盘
              -> FileTransferComplete -> 完成
```

### 远程终端数据流

```
控制端终端 UI <-> ShellData <-> 被控端 PTY 线程 <-> Shell 进程
                  (TLS)         (伪终端)          (bash/zsh/PowerShell)
```

> Shell 会话空闲 30 分钟后自动超时断开。所有终端操作均记录审计日志（target: `"audit"`）。

## 线程模型

LAN-Desk 被控端运行时包含以下主要线程：

| 线程 | 职责 | 说明 |
|------|------|------|
| **主线程** | Tauri 窗口管理、IPC 处理 | 运行 Tauri 事件循环 |
| **捕获线程** | 屏幕截图 + 编码 | 按配置帧率循环捕获，GPU 编码器优先（NVENC/VideoToolbox/VAAPI），回退 OpenH264，最终回退 JPEG。Wayland 下三级 fallback：(1) PipeWire/Portal（通过 XDG Desktop Portal + PipeWire，独立线程接收帧，首次弹出系统授权对话框，后续 restore_token 自动跳过）→ (2) 外部工具（grim/spectacle）→ (3) X11 XShm（XWayland）。光标形状检测：macOS（objc2 NSCursor）、Linux（XFixes 扩展） |
| **网络线程** | TLS 连接管理、消息收发 | tokio 异步运行时，处理所有网络 I/O |
| **PTY 线程** | 远程终端 I/O | 管理伪终端进程，转发 Shell 输入/输出 |
| **音频线程** | 音频捕获与编码 | PCM 捕获 -> Opus 编码 -> 网络传输 -> Opus 解码 -> WebSocket 推送前端 |
| **WebSocket accept 线程** | 本地 WebSocket 服务 | 监听 127.0.0.1，接受前端 WebSocket 连接，推送帧/音频二进制数据 |
| **心跳线程** | 连接保活 | 每 5 秒发送 Ping，15 秒无 Pong 则断开 |

控制端线程模型类似，但捕获线程替换为**解码/渲染线程**（接收帧数据并绘制到 Canvas），输入注入替换为**输入捕获**。

### Wayland 屏幕捕获架构

Wayland 下屏幕捕获采用三级 fallback 策略：

```
PipeWire/Portal（首选）→ 外部工具（grim/spectacle）→ X11 XShm（XWayland）
```

**第一级：PipeWire/Portal（零外部工具）**

通过 XDG Desktop Portal 的 `org.freedesktop.portal.ScreenCast` D-Bus 接口请求屏幕共享，PipeWire 提供帧流。该方案：
- 无需安装 grim/spectacle 等外部截图工具
- 首次使用时由合成器弹出系统授权对话框，用户选择共享的屏幕/窗口
- 授权后获得 `restore_token`，后续捕获自动跳过授权对话框
- 独立 PipeWire 线程持续接收帧数据，通过 `Mutex<Option<Vec<u8>>>` 共享给捕获主线程
- 通过 feature gate `pipewire-capture` 控制，默认启用
- 编译时需要 `libpipewire-0.3-dev`，运行时需要 `libpipewire-0.3`
- 支持 GNOME、KDE、Sway、Hyprland 等所有实现了 XDG Desktop Portal 的 Wayland 合成器

**第二级：外部截图工具**

当 PipeWire 不可用时（未安装依赖或 Portal 授权失败），自动降级到 grim（通用）或 spectacle（KDE Plasma）截图。

**第三级：X11 XShm**

当 Wayland 外部工具也不可用时，回退到 XWayland 上的 X11 XShm 捕获。

**Wayland 检测条件**

环境变量 `WAYLAND_DISPLAY` 已设置即视为 Wayland 环境（不再要求 `DISPLAY` 未设置，因为大多数 Wayland 合成器同时运行 XWayland）。

## 前端架构

前端使用 **Vue 3 + TypeScript + Vite**，通过 Tauri IPC 与 Rust 后端通信。

### 页面结构

```
App.vue
├── DiscoveryView    # 设备发现页（首页）
├── RemoteView       # 远程桌面视图
├── SettingsView     # 设置页面
└── TerminalPanel    # 远程终端面板（嵌入 RemoteView）
```

### Composable 模块

前端核心逻辑通过 Vue Composable 拆分：

- `useSettings` — 全局设置管理（单例模式）
- `useFrameRenderer` — 帧渲染（Canvas / H.264 VideoDecoder）
- `useInputHandler` — 鼠标键盘事件处理
- `useRemoteCursor` — 远程光标叠加绘制
- `useReconnect` — 断线重连逻辑
- `useStats` — 统计指标（FPS / 延迟 / 带宽）
- `useAudio` — 音频播放
- `useAnnotation` — 白板标注
- `useRecording` — 会话录制
- `useToast` — 全局 Toast 通知

### 路由方式

采用**手动视图路由**（基于响应式变量切换组件），未使用 vue-router。通过 `currentView` 状态变量控制当前显示的视图。

### Tauri IPC

前端通过 `@tauri-apps/api` 的 `invoke` 函数调用 Rust 后端命令，通过 `listen` 监听后端事件。

**事件监听：**

- `listen('frame-data', callback)` — 接收屏幕帧
- `listen('connection-status', callback)` — 连接状态变更
- `listen('shell-output', callback)` — 终端输出

**IPC 命令（共 45 个）：**

连接管理：
- `invoke('connect_to_peer', { ip, pin })` — 发起连接
- `invoke('disconnect')` — 断开连接
- `invoke('reconnect_to_peer')` — 重新连接
- `invoke('get_status')` — 获取连接状态
- `invoke('get_ws_port')` — 获取本地 WebSocket 端口

输入控制：
- `invoke('send_mouse_move', { x, y })` — 发送鼠标移动
- `invoke('send_mouse_button', { button, pressed })` — 发送鼠标按键
- `invoke('send_mouse_scroll', { delta })` — 发送鼠标滚轮
- `invoke('send_key_event', { key, pressed })` — 发送键盘事件

服务器管理：
- `invoke('start_server')` — 启动被控端服务器
- `invoke('stop_server')` — 停止被控端服务器
- `invoke('get_pins')` — 获取当前 PIN
- `invoke('refresh_pins')` — 刷新 PIN
- `invoke('set_unattended', { enabled })` — 设置无人值守模式
- `invoke('set_fixed_pins', { pin })` — 设置固定 PIN

显示器：
- `listen('monitor-list')` — 接收被控端推送的远程显示器列表（MonitorInfo 含 left/top 偏移）
- `invoke('switch_monitor', { index })` — 切换被控端显示器（自动更新坐标映射）

文件传输：
- `invoke('send_file', { path })` — 发送文件

设置：
- `invoke('set_bandwidth_limit', { limit })` — 设置带宽限制
- `invoke('apply_capture_settings', { settings })` — 应用捕获设置
- `invoke('set_clipboard_sync', { enabled })` — 设置剪贴板同步
- `invoke('set_shell_enabled', { enabled })` — 设置终端功能开关

远程终端：
- `invoke('start_shell')` — 启动远程终端
- `invoke('send_shell_input', { data })` — 发送终端输入
- `invoke('resize_shell', { cols, rows })` — 调整终端大小
- `invoke('close_shell')` — 关闭远程终端

设备发现：
- `invoke('discover_peers')` — 发现局域网设备
- `invoke('wake_on_lan', { mac })` — 网络唤醒

安全：
- `invoke('list_trusted_hosts')` — 列出已信任的主机
- `invoke('remove_trusted_host', { host })` — 移除已信任的主机

> **TOFU 指纹存储说明**：TOFU 证书指纹按连接地址（IP:port）存储，而非按域名存储。这意味着同一台主机如果使用不同端口，将被视为不同的信任条目。

---

<a id="architecture-english"></a>

# Architecture (English)

This document describes the overall architecture of LAN-Desk.

## Project Structure

LAN-Desk uses a Rust + Vue 3 + Tauri architecture. The backend consists of 7 crates:

```
lan-desk/
├── src-tauri/            # Tauri main app (frontend-backend bridge, window management, IPC)
├── crates/
│   ├── lan-desk-protocol/    # Protocol definitions (message types, ser/de, TLS)
│   ├── lan-desk-capture/     # Screen capture (DXGI/CGDisplay/XShm/PipeWire Portal/Wayland grim) + GPU encoding (NVENC/VideoToolbox/VAAPI)
│   ├── lan-desk-input/       # Input injection (SendInput/CGEvent/XTest/Wayland ydotool·wtype)
│   ├── lan-desk-clipboard/   # Clipboard sync (cross-platform clipboard R/W)
│   ├── lan-desk-audio/       # Audio capture & playback (WASAPI/CoreAudio/PulseAudio) + Opus codec
│   └── lan-desk-server/      # Host server (TCP/TLS listener, session management, PTY terminal)
└── src/                  # Vue 3 frontend (UI)
```

### AppState Management

AppState is divided into 3 semantic Mutex groups: AuthState (PIN and authentication), ServerState (port and running state), and ConnState (connections and WebSocket).

PIN verification uses an exponential backoff strategy: after 5 consecutive failures the IP is locked for 5 minutes, with subsequent failures increasing the lockout duration (15min → 1h → ... → 24h cap), plus a global rate limit of 10 attempts/minute.

### Crate Dependency Graph

```
src-tauri (main app)
├── lan-desk-protocol     # Protocol layer (all network communication)
├── lan-desk-capture      # Screen capture
├── lan-desk-input        # Input injection
├── lan-desk-clipboard    # Clipboard sync
├── lan-desk-audio        # Audio bridge
└── lan-desk-server       # Host server

lan-desk-server
└── lan-desk-protocol
```

## Data Flow

### Transport Channels

LAN-Desk uses a **dual-channel** architecture:

| Channel | Purpose | Notes |
|---------|---------|-------|
| **TLS TCP** | Control commands, file transfer, terminal I/O | Low-frequency events, MessagePack serialization |
| **Local WebSocket** | Frame data, audio data | 127.0.0.1 local WebSocket binary push, bypasses Tauri JSON IPC, eliminates base64 ~33% overhead |

High-bandwidth data (screen frames + audio streams) is pushed to the frontend via local WebSocket as binary frames. Low-frequency events (keyboard/mouse, clipboard, file transfer, etc.) continue through Tauri emit.

### WebSocket Security Notes

The local WebSocket bridge security model is based on three layers of defense:

1. **Bound to 127.0.0.1 only** — The WebSocket server listens exclusively on the loopback address and does not accept any remote connections, eliminating network-level attack surface.
2. **32-byte random token authentication** — After connection is established, the client must send a 32-byte cryptographically secure random token generated by the Rust backend. The server only begins pushing data after successful verification. This token is passed to the frontend via Tauri IPC, which is restricted to intra-process communication and inaccessible to external processes.
3. **Ephemeral port + port validation** — The WebSocket listening port is randomly assigned by the OS (ephemeral port). The frontend validates that the port number is within the legal range (1024-65535) before connecting.

**Regarding `ws://127.0.0.1:*` in CSP**: Since the port is randomly assigned at runtime, it cannot be specified in a static CSP policy. Therefore, the `connect-src` CSP directive uses a wildcard port. This does not reduce security because actual connections are protected by the token authentication described above.

### Remote Desktop Data Flow

```
Controller                                   Host
┌─────────────┐                        ┌──────────────────┐
│  Vue 3 UI   │                        │  Capture thread   │
│  (Canvas)   │◄── Local WebSocket ───│  (DXGI/CG/XShm/  │
│   grim)           │
│             │    Frame/audio binary  │       │           │
│             │◄── TLS connection ────│  GPU/soft encode   │
│             │    FrameData           │       │           │
│  Input ─────│──► MouseMove/KeyEvent ─│──► Input thread   │
│  events     │    (TLS encrypted)     │  (SendInput/etc)  │
└─────────────┘                        └──────────────────┘
```

### File Transfer Data Flow

```
Controller selects file -> FileTransferRequest -> Host confirms
                        -> FileTransferData (chunked) -> Write to disk
                        -> FileTransferComplete -> Done
```

### Remote Terminal Data Flow

```
Controller terminal UI <-> ShellData <-> Host PTY thread <-> Shell process
                           (TLS)         (pseudo-terminal)   (bash/zsh/PowerShell)
```

> Shell sessions are automatically timed out after 30 minutes of inactivity. All terminal operations are recorded in the audit log (target: `"audit"`).

## Thread Model

The LAN-Desk host runs the following main threads:

| Thread | Responsibility | Notes |
|--------|---------------|-------|
| **Main thread** | Tauri window management, IPC | Runs the Tauri event loop |
| **Capture thread** | Screen capture + encoding | Captures at configured FPS, GPU encoder preferred (NVENC/VideoToolbox/VAAPI), falls back to OpenH264, then JPEG. On Wayland, uses three-level fallback: (1) PipeWire/Portal (via XDG Desktop Portal + PipeWire, dedicated thread receives frames, system authorization dialog on first use, restore_token auto-skip on subsequent use) → (2) External tools (grim/spectacle) → (3) X11 XShm (XWayland). Cursor shape detection: macOS (objc2 NSCursor), Linux (XFixes extension) |
| **Network thread** | TLS connection management, message I/O | tokio async runtime for all network I/O |
| **PTY thread** | Remote terminal I/O | Manages pseudo-terminal process, relays shell I/O |
| **Audio thread** | Audio capture & encoding | PCM capture -> Opus encode -> network transfer -> Opus decode -> WebSocket push to frontend |
| **WebSocket accept thread** | Local WebSocket server | Listens on 127.0.0.1, accepts frontend WebSocket connections, pushes frame/audio binary data |
| **Heartbeat thread** | Connection keep-alive | Sends Ping every 5s, disconnects after 15s without Pong |

The controller has a similar thread model, but the capture thread is replaced by a **decode/render thread** (receives frame data and draws to Canvas), and input injection is replaced by **input capture**.

### Wayland Screen Capture Architecture

On Wayland, screen capture uses a three-level fallback strategy:

```
PipeWire/Portal (preferred) → External tools (grim/spectacle) → X11 XShm (XWayland)
```

**Level 1: PipeWire/Portal (zero external tools)**

Requests screen sharing via the XDG Desktop Portal `org.freedesktop.portal.ScreenCast` D-Bus interface, with PipeWire providing the frame stream. This approach:
- Requires no external screenshot tools like grim/spectacle
- On first use, the compositor displays a system authorization dialog for the user to select the screen/window to share
- After authorization, a `restore_token` is obtained; subsequent captures automatically skip the authorization dialog
- A dedicated PipeWire thread continuously receives frame data, shared with the main capture thread via `Mutex<Option<Vec<u8>>>`
- Controlled by the `pipewire-capture` feature gate, enabled by default
- Requires `libpipewire-0.3-dev` at compile time and `libpipewire-0.3` at runtime
- Supports GNOME, KDE, Sway, Hyprland, and all Wayland compositors that implement XDG Desktop Portal

**Level 2: External screenshot tools**

When PipeWire is unavailable (dependencies not installed or Portal authorization failed), automatically falls back to grim (generic) or spectacle (KDE Plasma) for screenshots.

**Level 3: X11 XShm**

When Wayland external tools are also unavailable, falls back to X11 XShm capture on XWayland.

**Wayland detection**

The environment variable `WAYLAND_DISPLAY` being set is sufficient to detect a Wayland environment (no longer requires `DISPLAY` to be unset, since most Wayland compositors run XWayland simultaneously).

## Frontend Architecture

The frontend uses **Vue 3 + TypeScript + Vite**, communicating with the Rust backend via Tauri IPC.

### Page Structure

```
App.vue
├── DiscoveryView    # Device discovery page (home)
├── RemoteView       # Remote desktop view
├── SettingsView     # Settings page
└── TerminalPanel    # Remote terminal panel (embedded in RemoteView)
```

### Composable Modules

Core frontend logic is split into Vue Composables:

- `useSettings` — Global settings management (singleton pattern)
- `useFrameRenderer` — Frame rendering (Canvas / H.264 VideoDecoder)
- `useInputHandler` — Mouse and keyboard event handling
- `useRemoteCursor` — Remote cursor overlay rendering
- `useReconnect` — Disconnection reconnect logic
- `useStats` — Statistics metrics (FPS / latency / bandwidth)
- `useAudio` — Audio playback
- `useAnnotation` — Whiteboard annotation
- `useRecording` — Session recording
- `useToast` — Global toast notifications

### Routing

Uses **manual view routing** (component switching based on reactive variables) without vue-router. The `currentView` state variable controls which view is displayed.

### Tauri IPC

The frontend calls Rust backend commands via `invoke` from `@tauri-apps/api` and listens to backend events via `listen`.

**Event Listeners:**

- `listen('frame-data', callback)` — receive screen frames
- `listen('connection-status', callback)` — connection status changes
- `listen('shell-output', callback)` — terminal output

**IPC Commands (45 total):**

Connection:
- `invoke('connect_to_peer', { ip, pin })` — initiate connection
- `invoke('disconnect')` — disconnect
- `invoke('reconnect_to_peer')` — reconnect
- `invoke('get_status')` — get connection status
- `invoke('get_ws_port')` — get local WebSocket port

Input:
- `invoke('send_mouse_move', { x, y })` — send mouse movement
- `invoke('send_mouse_button', { button, pressed })` — send mouse button event
- `invoke('send_mouse_scroll', { delta })` — send mouse scroll
- `invoke('send_key_event', { key, pressed })` — send keyboard event

Server:
- `invoke('start_server')` — start host server
- `invoke('stop_server')` — stop host server
- `invoke('get_pins')` — get current PINs
- `invoke('refresh_pins')` — refresh PINs
- `invoke('set_unattended', { enabled })` — set unattended mode
- `invoke('set_fixed_pins', { pin })` — set fixed PIN

Display:
- `listen('monitor-list')` — receive remote monitor list pushed by host (MonitorInfo includes left/top offsets)
- `invoke('switch_monitor', { index })` — switch host display (auto-updates coordinate mapping)

File:
- `invoke('send_file', { path })` — send file

Settings:
- `invoke('set_bandwidth_limit', { limit })` — set bandwidth limit
- `invoke('apply_capture_settings', { settings })` — apply capture settings
- `invoke('set_clipboard_sync', { enabled })` — set clipboard sync
- `invoke('set_shell_enabled', { enabled })` — enable/disable shell

Shell:
- `invoke('start_shell')` — start remote shell
- `invoke('send_shell_input', { data })` — send shell input
- `invoke('resize_shell', { cols, rows })` — resize shell
- `invoke('close_shell')` — close remote shell

Discovery:
- `invoke('discover_peers')` — discover LAN devices
- `invoke('wake_on_lan', { mac })` — Wake-on-LAN

Security:
- `invoke('list_trusted_hosts')` — list trusted hosts
- `invoke('remove_trusted_host', { host })` — remove a trusted host

> **TOFU fingerprint storage note**: TOFU certificate fingerprints are stored per connection address (IP:port), not per domain name. This means the same host using different ports will be treated as separate trust entries.
