**中文** | [English](#protocol-english)

# 协议规范

本文档描述 LAN-Desk 使用的网络协议。

## 协议概述

LAN-Desk 采用 **TCP + TLS 1.3 + 自定义二进制帧协议** 进行通信，同时使用**本地 WebSocket 二进制通道**向前端推送高带宽数据。当前协议版本：**v3**。

- **传输层**：TCP（默认端口 25605）
- **安全层**：TLS 1.3（自签名证书，防被动嗅探）
- **应用层**：自定义二进制帧协议（长度前缀 + MessagePack 序列化）
- **本地 WebSocket**：127.0.0.1 本地 WebSocket，用于帧数据和音频数据的二进制推送，绕过 Tauri JSON IPC，消除 base64 ~33% 开销

## 帧格式

所有消息使用统一的帧格式：

```
+-------------------+-----------------------------+
| 长度前缀 (4字节)  | 消息载荷 (MessagePack 序列化) |
| u32 big-endian    | Message enum                |
+-------------------+-----------------------------+
```

- **长度前缀**：4 字节大端序无符号整数，表示后续载荷的字节数
- **消息载荷**：使用 Rust `rmp-serde` 库（MessagePack 格式）序列化的 `Message` 枚举

## 消息类型

| 消息 | 方向 | 说明 |
|------|------|------|
| `Hello` | 控制端 -> 被控端 | 发起连接，携带认证信息 |
| `HelloAck` | 被控端 -> 控制端 | 认证结果响应 |
| `FrameData` | 被控端 -> 控制端 | 屏幕帧数据（JPEG/H.264） |
| `FrameAck` | 控制端 -> 被控端 | 帧确认（协议预留字段，当前版本不发送/不处理，用于未来帧级流控） |
| `MouseMove` | 控制端 -> 被控端 | 鼠标移动事件 |
| `MouseButton` | 控制端 -> 被控端 | 鼠标按键事件 |
| `MouseScroll` | 控制端 -> 被控端 | 鼠标滚轮事件 |
| `KeyEvent` | 控制端 -> 被控端 | 键盘按键事件 |
| `ClipboardUpdate` | 双向 | 剪贴板内容同步 |
| `FileTransferStart` | 控制端 -> 被控端 | 文件传输请求 |
| `FileTransferData` | 控制端 -> 被控端 | 文件数据块（offset 字段指示数据块在文件中的起始偏移量，接收端使用 seek 按 offset 写入，支持乱序到达） |
| `FileTransferComplete` | 控制端 -> 被控端 | 文件传输完成 |
| `ShellStart` | 控制端 -> 被控端 | 请求启动远程终端 |
| `ShellStartAck` | 被控端 -> 控制端 | 终端启动结果（成功/失败） |
| `ShellData` | 双向 | 终端输入/输出数据 |
| `ShellResize` | 控制端 -> 被控端 | 终端窗口大小变更 |
| `ShellClose` | 双向 | 关闭远程终端。Shell 有 30 分钟空闲超时，超时后服务端自动发送 ShellClose |
| `AudioData` | 被控端 -> 控制端 | 音频数据（encoding 字段：Pcm16 或 Opus） |
| `AudioFormat` | 被控端 -> 控制端 | 音频格式信息（encoding 字段：AudioEncoding::Pcm16 / AudioEncoding::Opus） |
| `Ping` | 双向 | 心跳探测 |
| `Pong` | 双向 | 心跳响应 |
| `MonitorList` | 被控端 -> 控制端 | 可用显示器列表（会话建立后自动发送）。MonitorInfo 含 index/name/width/height/is_primary/left/top |
| `SwitchMonitor` | 控制端 -> 被控端 | 切换到指定显示器（被控端同步更新输入坐标映射） |
| `Annotation` | 控制端 -> 被控端 | 白板标注绘制数据（归一化坐标） |
| `AnnotationClear` | 控制端 -> 被控端 | 清除所有标注 |
| `CaptureSettings` | 控制端 -> 被控端 | 画质和帧率动态调整 |
| `SetBandwidthLimit` | 控制端 -> 被控端 | 带宽限制设置 |
| `SystemInfo` | 被控端 -> 控制端 | 系统信息（CPU/内存使用率） |
| `Disconnect` | 双向 | 断开连接 |
| `ChatMessage` | 双向 | 文字聊天消息 |
| `SpecialKey` | 控制端 -> 被控端 | 系统快捷键透传（Ctrl+Alt+Del、Alt+Tab、Alt+F4 等） |
| `ScreenBlank` | 控制端 -> 被控端 | 屏幕遮蔽开关（防止被控端旁人窥视） |
| `LockScreen` | 控制端 -> 被控端 | 远程锁屏请求 |
| `RemoteReboot` | 控制端 -> 被控端 | 远程重启请求 |
| `RebootPending` | 被控端 -> 控制端 | 重启即将执行通知（控制端准备自动重连） |
| `FileListRequest` | 控制端 -> 被控端 | 远程文件浏览：请求目录列表 |
| `FileListResponse` | 被控端 -> 控制端 | 远程文件浏览：返回目录内容 |
| `FileDownloadRequest` | 控制端 -> 被控端 | 反向文件下载：请求从被控端下载文件 |
| `DirectoryDownloadRequest` | 控制端 -> 被控端 | 反向目录下载：请求从被控端下载整个目录 |
| `DirectoryTransferStart` | 控制端 -> 被控端 | 目录传输：开始传输整个目录 |
| `DirectoryEntry` | 控制端 -> 被控端 | 目录传输：单个目录项元数据 |
| `FileTransferResume` | 双向 | 断点续传：携带已传输偏移量，从断点恢复传输 |

> 以上为 TCP 通道的 42 种消息类型。加上 UDP 发现协议的 `DiscoveryPing` / `DiscoveryPong`，协议共定义 **44 种消息类型**。

## 发现协议

设备发现使用 **UDP 广播**（默认端口 25606）：

1. 控制端向局域网广播地址发送 `DiscoveryPing`（包含设备名称）
2. 被控端收到后回复 `DiscoveryPong`（包含设备名称、IP、端口）
3. 发现协议**无加密、无认证**，仅用于便捷发现，不应作为可信来源

## WebSocket 二进制协议格式

本地 WebSocket 通道使用统一的二进制帧格式，所有消息以 1 字节类型标识开头：

```
+-------------------+-----------------------------+
| 类型标识 (1字节)  | 载荷 (变长)                  |
+-------------------+-----------------------------+
```

### 消息类型

| 类型标识 | 名称 | 载荷格式 |
|----------|------|----------|
| `0x01` | 帧数据 (Frame) | `[encoding: u8][width: u16 BE][height: u16 BE][data: ...]` |
| `0x02` | 音频数据 (Audio) | `[encoding: u8][data: ...]`，encoding: 0=PCM16, 1=Opus |
| `0x03` | 音频格式 (AudioFormat) | `[encoding: u8][sample_rate: u32 BE][channels: u16 BE]` |

- **0x01 帧数据**：encoding 标识编码格式（0=JPEG, 1=H.264），后跟宽高和原始编码数据
- **0x02 音频数据**：encoding 标识 PCM16(0) 或 Opus(1)，后跟音频样本/Opus 包
- **0x03 音频格式**：在音频流开始时发送一次，告知前端采样率、声道数和编码格式

## 认证流程

LAN-Desk 使用双 PIN 码认证机制：

1. 被控端启动时生成两个 8 位随机 PIN：**控制 PIN** 和 **查看 PIN**
2. 控制端发送 `Hello` 消息，携带：
   - 随机生成的 salt（32 字节）
   - PIN 的 Argon2id 加盐哈希值：`Argon2id(PIN, salt)`（内存 19MB，迭代 2 次，并行度 1）
   - `requested_role: SessionRole` — 请求的角色（Controller / Viewer）
3. 被控端使用相同 salt 计算本地 PIN 的哈希值并比对
4. 匹配控制 PIN → 授予完全控制权限；匹配查看 PIN → 授予仅查看权限。验证失败后实施指数退避锁定：首次 5 分钟，后续 15 分钟、45 分钟...最长 24 小时，并设有全局速率限制（每分钟最多 10 次失败）
5. 被控端弹窗确认（除非开启了无人值守模式）。授权弹窗会向被控端用户显示对方请求的权限类型（完全控制/仅查看）
6. 发送 `HelloAck` 返回认证结果，字段如下：
   - `version: u32` — 协议版本号
   - `accepted: bool` — 是否接受连接
   - `reject_reason: String` — 拒绝原因（accepted 为 true 时为空）
   - `granted_role: SessionRole` — 实际授予的角色（Controller / Viewer）

---

<a id="protocol-english"></a>

# Protocol Specification (English)

This document describes the network protocol used by LAN-Desk.

## Protocol Overview

LAN-Desk uses **TCP + TLS 1.3 + custom binary frame protocol** for communication, plus a **local WebSocket binary channel** for pushing high-bandwidth data to the frontend. Current protocol version: **v3**.

- **Transport layer**: TCP (default port 25605)
- **Security layer**: TLS 1.3 (self-signed certificate, prevents passive sniffing)
- **Application layer**: Custom binary frame protocol (length-prefixed + MessagePack serialization)
- **Local WebSocket**: 127.0.0.1 local WebSocket for binary push of frame and audio data, bypassing Tauri JSON IPC and eliminating base64 ~33% overhead

## Frame Format

All messages use a unified frame format:

```
+---------------------+-------------------------------+
| Length prefix (4B)   | Message payload (MessagePack) |
| u32 big-endian       | Message enum                  |
+---------------------+-------------------------------+
```

- **Length prefix**: 4-byte big-endian unsigned integer indicating the payload size in bytes
- **Message payload**: Rust `rmp-serde` (MessagePack format) serialized `Message` enum

## Message Types

| Message | Direction | Description |
|---------|-----------|-------------|
| `Hello` | Controller -> Host | Initiate connection with auth info |
| `HelloAck` | Host -> Controller | Authentication result response |
| `FrameData` | Host -> Controller | Screen frame data (JPEG/H.264) |
| `FrameAck` | Controller -> Host | Frame acknowledgement (reserved for future flow control, not sent/processed in current version) |
| `MouseMove` | Controller -> Host | Mouse move event |
| `MouseButton` | Controller -> Host | Mouse button event |
| `MouseScroll` | Controller -> Host | Mouse scroll event |
| `KeyEvent` | Controller -> Host | Keyboard event |
| `ClipboardUpdate` | Bidirectional | Clipboard content sync |
| `FileTransferStart` | Controller -> Host | File transfer request |
| `FileTransferData` | Controller -> Host | File data chunk (offset field indicates the starting offset in the file; receiver uses seek to write at offset, supporting out-of-order arrival) |
| `FileTransferComplete` | Controller -> Host | File transfer complete |
| `ShellStart` | Controller -> Host | Request to start remote terminal |
| `ShellStartAck` | Host -> Controller | Terminal start result (success/failure) |
| `ShellData` | Bidirectional | Terminal input/output data |
| `ShellResize` | Controller -> Host | Terminal window resize |
| `ShellClose` | Bidirectional | Close remote terminal. Shell sessions have a 30-minute idle timeout; the server automatically sends ShellClose upon timeout |
| `AudioData` | Host -> Controller | Audio data (encoding field: Pcm16 or Opus) |
| `AudioFormat` | Host -> Controller | Audio format info (encoding field: AudioEncoding::Pcm16 / AudioEncoding::Opus) |
| `Ping` | Bidirectional | Heartbeat probe |
| `Pong` | Bidirectional | Heartbeat response |
| `MonitorList` | Host -> Controller | Available monitor list (auto-sent after session established). MonitorInfo contains index/name/width/height/is_primary/left/top |
| `SwitchMonitor` | Controller -> Host | Switch to specified monitor (host updates input coordinate mapping) |
| `Annotation` | Controller -> Host | Whiteboard annotation draw data (normalized coords) |
| `AnnotationClear` | Controller -> Host | Clear all annotations |
| `CaptureSettings` | Controller -> Host | Dynamic quality/fps adjustment |
| `SetBandwidthLimit` | Controller -> Host | Bandwidth limit setting |
| `SystemInfo` | Host -> Controller | System info (CPU/memory usage) |
| `Disconnect` | Bidirectional | Disconnect |
| `ChatMessage` | Bidirectional | Text chat message |
| `SpecialKey` | Controller -> Host | System shortcut passthrough (Ctrl+Alt+Del, Alt+Tab, Alt+F4, etc.) |
| `ScreenBlank` | Controller -> Host | Screen blanking toggle (prevents bystanders from viewing) |
| `LockScreen` | Controller -> Host | Remote lock screen request |
| `RemoteReboot` | Controller -> Host | Remote reboot request |
| `RebootPending` | Host -> Controller | Reboot imminent notification (controller prepares for auto-reconnect) |
| `FileListRequest` | Controller -> Host | Remote file browsing: request directory listing |
| `FileListResponse` | Host -> Controller | Remote file browsing: return directory contents |
| `FileDownloadRequest` | Controller -> Host | Reverse file download: request file download from host |
| `DirectoryDownloadRequest` | Controller -> Host | Reverse directory download: request entire directory from host |
| `DirectoryTransferStart` | Controller -> Host | Directory transfer: start transferring entire directory |
| `DirectoryEntry` | Controller -> Host | Directory transfer: single directory entry metadata |
| `FileTransferResume` | Bidirectional | Resume transfer: carries transferred offset, resumes from breakpoint |

> The above lists 42 TCP message types. Together with UDP discovery protocol's `DiscoveryPing` / `DiscoveryPong`, the protocol defines a total of **44 message types**.

## Discovery Protocol

Device discovery uses **UDP broadcast** (default port 25606):

1. Controller broadcasts `DiscoveryPing` (includes device name) to the LAN broadcast address
2. Host responds with `DiscoveryPong` (includes device name, IP, port)
3. The discovery protocol is **unencrypted and unauthenticated** — for convenience only, not a trusted source

## WebSocket Binary Protocol Format

The local WebSocket channel uses a unified binary frame format. All messages start with a 1-byte type identifier:

```
+-------------------+-----------------------------+
| Type ID (1 byte)  | Payload (variable length)   |
+-------------------+-----------------------------+
```

### Message Types

| Type ID | Name | Payload Format |
|---------|------|----------------|
| `0x01` | Frame Data | `[encoding: u8][width: u16 BE][height: u16 BE][data: ...]` |
| `0x02` | Audio Data | `[encoding: u8][data: ...]`, encoding: 0=PCM16, 1=Opus |
| `0x03` | Audio Format | `[encoding: u8][sample_rate: u32 BE][channels: u16 BE]` |

- **0x01 Frame Data**: encoding identifies the codec (0=JPEG, 1=H.264), followed by width, height, and raw encoded data
- **0x02 Audio Data**: encoding identifies PCM16(0) or Opus(1), followed by audio samples or Opus packets
- **0x03 Audio Format**: Sent once at audio stream start to inform the frontend of sample rate, channel count, and encoding format

## Authentication Flow

LAN-Desk uses a dual-PIN authentication mechanism:

1. Host generates two 8-digit random PINs on startup: **Control PIN** and **View PIN**
2. Controller sends `Hello` message containing:
   - Randomly generated salt (32 bytes)
   - Argon2id salted hash of the PIN: `Argon2id(PIN, salt)` (19MB memory, 2 iterations, parallelism 1)
   - `requested_role: SessionRole` — the requested role (Controller / Viewer)
3. Host computes the hash of its local PINs using the same salt and compares
4. Matches Control PIN -> grants full control; matches View PIN -> grants view-only access. On verification failure, exponential backoff lockout is enforced: first lockout is 5 minutes, then 15 minutes, 45 minutes, and so on up to a maximum of 24 hours, with a global rate limit (max 10 failures per minute)
5. Host shows confirmation dialog (unless unattended mode is enabled). The dialog shows the requester's requested permission type (full control / view only) to the host user
6. Sends `HelloAck` with the authentication result, containing the following fields:
   - `version: u32` — protocol version number
   - `accepted: bool` — whether the connection is accepted
   - `reject_reason: String` — rejection reason (empty when accepted is true)
   - `granted_role: SessionRole` — the actually granted role (Controller / Viewer)
