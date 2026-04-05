# Changelog / 更新日志

All notable changes to this project will be documented in this file. / 本文件记录项目的所有重要变更。

The format is based on [Keep a Changelog](https://keepachangelog.com/).

**中文** | [English version below](#english-changelog)

---

## [v1.0.0] - 2026-04-06

首个正式版，包含全部核心功能以及大量安全/平台/架构增强。以下为完整变更记录。

### Added / 新增（核心功能）

- **多显示器精确坐标映射**：InputInjector 新增 `set_active_monitor()` 方法，`cursor_position()` 和 `move_mouse()` 基于当前捕获显示器归一化（而非虚拟桌面全局），Windows/macOS/Linux/Wayland 全平台适配
- **远程显示器列表推送**：被控端会话建立后通过 `MonitorList` 协议消息自动推送显示器列表（含 left/top 像素偏移），`SwitchMonitor` 时同步更新输入坐标映射
- 远程控制界面右侧可折叠工具面板（180px 宽），顶栏精简为 28px 状态信息栏，不遮挡远程桌面任务栏区域
- **连接状态显示**：被控端 Discovery 页面显示 "谁连接了我"（主机名/IP/角色），控制端显示 "我正在控制"（可点击切换）
- **被控端聊天发送**：通过 broadcast 通道向所有远程控制端广播消息（修复被控端无法发送聊天的问题）
- **共享捕获切换显示器**：`shared_capture_loop` 支持 `CaptureCommand`，多显示器切换命令可正确到达捕获线程
- **macOS Retina 坐标修复**：`set_active_monitor` 通过 `CGDisplay.bounds()` 获取逻辑点尺寸，解决 Retina 屏幕放大问题
- **固定密码启动时序**：先同步固定密码到后端再启动服务器（修复重启后固定密码连接被拒绝的问题）
- **移动端编译修复**：`get_device_id` / Opus 解码 / `ClipboardChange` 类型正确添加 `#[cfg]` 条件编译门控
- **MonitorList 竞态修复**：缓存远程显示器列表到 ConnState，RemoteView 挂载时通过 `get_remote_monitors` 命令获取（解决事件在监听器注册前丢失的问题）
- **终端设置动态生效**：`shell_enabled` 改为 `Arc<AtomicBool>` 共享，运行中的 Server 每次 accept 读取最新值
- **WebM 录制进度条修复**：录制结束时自动注入 Duration 元数据到 EBML 头，播放器可正确显示/拖动进度条
- **完整用户手册** `docs/USER_GUIDE.md`：中英双语，12 个章节覆盖全部功能的详细操作说明
- **代码签名指南** `docs/CODE_SIGNING.md`：iOS/macOS/Windows/Android 四平台签名配置文档
- **cargo audit** CI：新增 Security Audit job，自动检测依赖安全漏洞
- **Android 自动签名**：Release CI 支持通过 GitHub Secrets 自动签名 APK
- **Android x86_64 支持**：APK 包含 ARM64 + x86_64 双架构，支持模拟器运行

- PIN 暴力破解防护：指数退避锁定策略（5次失败后锁定5分钟→15分钟→45分钟→…→最长24小时），全局速率限制每分钟最多10次失败
- 文件传输过滤 Windows 保留设备名（CON/NUL/AUX/PRN/COM1-9/LPT1-9），防止写入系统设备
- 远程终端（PTY Shell）30分钟空闲超时自动关闭 + 结构化审计日志（target: "audit"）
- TOFU 证书管理 UI：在设置页面查看和撤销已信任的远程主机指纹
- macOS：屏幕录制权限（CGPreflightScreenCaptureAccess）和辅助功能权限（AXIsProcessTrusted）自动检测与引导
- Windows：多显示器 DPI 使用 GetDpiForSystem，鼠标坐标使用虚拟屏幕（SM_CXVIRTUALSCREEN + MOUSEEVENTF_VIRTUALDESK）
- macOS：剪贴板变更检测使用 NSPasteboard.changeCount 原生 FFI
- Vitest 前端单元测试框架 + 示例测试用例
- CI 添加 ESLint 前端代码检查
- useToast composable，统一 Toast 通知管理
- Windows：PowerShell 远程终端强制 UTF-8 编码（-NoExit -Command "[Console]::OutputEncoding = UTF8"）
- Linux/macOS：Shell 启动时自动设置 LANG/LC_ALL=en_US.UTF-8（如未配置）
- JPEG 编码器缓冲区复用（rgb_buf/jpeg_buf 提升为结构体字段）
- 色彩转换（BGRA→I420）chunks_exact(4) 优化 + 输出缓冲区跨帧复用
- TLS 证书持久化：重启后复用证书，保持 TOFU 指纹一致
- Linux VAAPI 硬件编码检测（占位，自动回退 OpenH264）
- 原生 Wayland 屏幕捕获检测（占位，自动回退 XWayland）
- PipeWire/Portal Wayland 原生屏幕捕获：通过 XDG Desktop Portal + PipeWire 实现零外部工具截屏，三级 fallback（PipeWire/Portal → Wayland 外部工具 → X11 XShm），首次使用弹出系统授权对话框，后续通过 restore_token 自动跳过；支持 GNOME、KDE、Sway、Hyprland 等主流 Wayland 合成器；feature gate `pipewire-capture` 默认启用
- macOS 光标形状同步（NSCursor，支持 8 种光标类型）
- Linux 光标形状同步（XFixes，支持 12 种光标类型）
- 系统托盘菜单国际化（根据系统语言自动切换中/英文）
- CI Linux 构建 job
- CI 代码签名支持（Windows Authenticode + macOS notarize）
- DPI 检测增强：Linux QT_SCALE_FACTOR/GDK_DPI_SCALE，macOS Retina 2x
- AudioContext suspended 自动恢复
- JPEG 帧序列号防乱序（WebSocket 路径）
- WebSocket 二进制传输通道（帧/音频数据绕过 Tauri JSON IPC，消除 base64 ~33% 开销）
- Opus 音频编码（带宽从 ~1.5Mbps 降至 ~128kbps，降低 92%，支持 PCM 自动降级）
- GPU 硬件编码器完整实现（NVENC Windows / VideoToolbox macOS，自动检测 + OpenH264 回退）
- 共享色彩空间转换模块（bgra_to_i420 + bgra_to_nv12，供 OpenH264 和 NVENC 共用）
- Linux 平台支持（X11 SHM 屏幕捕获 + XTest 输入注入 + ALSA/PulseAudio 音频）
- 远程终端（PTY Shell 双向交互，基于 xterm.js）
- 双 PIN 权限控制（控制 PIN 完全权限 / 查看 PIN 仅观看）
- 无人值守模式（固定密码 + 自动接受连接）
- 被控端服务器 crate `lan-desk-server`（TLS TCP 监听、会话管理、共享帧广播）
- 国际化翻译扩展至 229 个 key，全 UI 覆盖
- 授权弹窗显示请求的权限类型（完全控制/仅查看）
- 剪贴板同步开关：可在设置中关闭自动剪贴板同步
- 远程终端开关：可在设置中禁止远程终端访问，增强安全性
- Wayland 新增 spectacle（KDE）截图工具支持
- Wayland 新增 dotool 输入注入支持（不需要 root 权限）
- AMD AMF / Intel QSV 硬件编码器支持（骨架）
- 协议层从 bincode 迁移到 MessagePack，支持向前兼容
- Windows 剪贴板使用 GetClipboardSequenceNumber 优化变化检测
- 前端大规模重构：commands.rs / session.rs / RemoteView.vue 模块化拆分
- 新增 useSettings composable 统一设置管理
- 版本号一致性检查脚本 `scripts/check_versions.py`，自动校验 9 个文件的版本号是否一致
- 补充 `useFrameRenderer` JPEG 路径单元测试（IPC + WebSocket 二进制模式）
- 补充 `message.rs` 8 个消息类型的序列化/反序列化往返测试
- 补充 `ws_bridge.rs` 7 个二进制编码单元测试（帧头、音频格式、音频数据、光标映射等）
- 补充 `ct_eq_str` / `ct_eq_bytes` 常量时间比较函数 6 个单元测试
- 文字聊天：远程会话中支持即时文字通信
- 快捷键透传：支持 Ctrl+Alt+Del、Alt+Tab、Alt+F4 等系统快捷键透传到被控端
- 双向文件传输 + 远程文件浏览器（双栏布局，支持上传/下载）
- 目录传输 + 断点续传：支持整个目录传输，断点续传避免重复传输已完成部分
- H.265/HEVC 硬件编码：NVENC HEVC 编码支持，自动回退 H.264
- AV1 编码支持（预留接口，待 GPU 硬件编码器可用时启用）
- 会话空闲超时：可配置空闲超时（默认 30 分钟），超时自动断开连接
- 断开时自动锁屏：远程会话断开时自动锁定被控端屏幕（跨平台，可配置）
- 屏幕遮蔽：防止被控端旁人窥视远程操作内容（可配置）
- 远程重启后自动重连：检测到被控端重启后自动恢复会话
- 自适应比特率：基于 RTT 和带宽利用率动态调整编码质量
- 多标签/多会话：同时连接多台远程设备，标签页切换
- 深色/浅色主题切换：支持跟随系统主题自动切换
- 免安装运行：便携模式，exe 同目录放置 `.portable` 标记文件即可免安装运行
- 文件传输取消：传输过程中可随时取消
- 目录下载：从被控端下载整个目录
- 文件拖拽上传：拖拽文件到远程桌面窗口直接上传
- 开机自启动：设置中可开启登录时自动启动
- JPEG 并行编码：rayon 多核并行编码脏区域
- 连接历史别名：可为历史连接设置自定义名称
- 终端高度可拖拽调整
- 工具栏分组收纳：工具和远程控制下拉菜单
- 编码器类型实时显示（H.264/HEVC/JPEG）
- 音频质量可配置（低/中/高三档）
- 网络质量 RTT 趋势图
- TOFU 证书变更确认框
- 设备 ID：9 位数字唯一标识
- 会话录制回放：IndexedDB 存储 + 内置播放器
- 文件传输完成通知：系统 Notification 提醒
- 远程截图保存：一键 PNG 截图
- 连接密码记忆：可选记住密码
- 标注文字工具：画笔 + 文字双工具
- 多显示器下拉菜单：显示分辨率和主屏标记
- 快捷键帮助面板：F1 弹出
- 连接统计导出：CSV 格式
- 便携模式托盘标识：[P] 标记
- 聊天消息通知音
- 系统信息增强：内存总量显示
- 连接信息复制：设备 ID + 端口
- 设备 ID 连接：输入 9 位设备 ID 自动 UDP 扫描匹配连接
- Tailscale/ZeroTier 自动检测：VPN 虚拟网卡 IP 高亮显示
- Android / iOS 移动端支持：手机/平板作为控制端，触屏手势映射 + 虚拟键盘 + 响应式布局

### Fixed / 修复

- Windows 滚轮滚动方向（i32→u32 位转换明确化）
- NumpadEnter 扩展键标志缺失
- Windows 主显示器检测（使用 MONITORINFOF_PRIMARY 替代索引判断）
- Windows 分辨率变化后鼠标坐标映射错误
- Linux GetImage 回退模式 stride 计算
- macOS 像素格式检测（根据 bitmap_info 判断 BGRA/RGBA）
- 文件传输乱序数据块支持（seek+write 替代 append）
- JPEG 帧序列号检查逻辑（=== 替代 >=）
- QUICK_START.md 截图路径
- 远程终端中文 Windows GBK 乱码（改用 PowerShell + UTF-8 环境变量）
- **RemoteView 资源泄漏**：修复 4 个 Tauri event listener 未在组件卸载时清理，以及 VideoDecoder/AudioContext 未释放的问题
- **H.264 时间戳**：改用严格递增计数器替代 `Date.now()`，避免同毫秒内重复时间戳导致解码异常
- **integrity.ts SPA 兼容**：修复 `window.load` 事件在 SPA 单页应用中可能不触发的问题
- **session.rs 异步 IO**：文件传输接收改用 `tokio::fs` 替代 `std::fs`，避免阻塞 tokio runtime
- **macOS 滚轮事件**：改用 CoreGraphics C FFI (`CGEventCreateScrollEvent`) 替代 `core-graphics` crate 已移除的 `new_scroll_event` 封装，兼容 core-graphics 0.24+
- **文件传输大小限制**：接收端新增 2GB 文件大小限制，防止恶意对端填满磁盘
- **PIN 日志脱敏**：服务器启动日志中 PIN 码仅显示前 3 位
- **Server accept() 稳定性**：TCP accept 瞬态错误（如文件描述符耗尽）不再终止整个服务循环，改为记录日志并继续监听
- **useSettings reset()**：`reset()` 后 `loaded` 标志正确重置为 `false`，确保后续 `load()` 能从 localStorage 重新读取
- **Discovery 扫描错误提示**：`scanPeers` 失败时使用 `toast.error` 提示用户，与其他操作保持一致
- **build.sh Linux**：Linux 分支补充 cargo/node 前置依赖检查
- **PipeWire Portal API**：修复 `ashpd` 0.10 的 `PersistMode` 导入路径、`start().response()` 调用、`SourceType::Monitor.into()` 类型转换、`OwnedFd` vs `BorrowedFd`、`StreamRef` vs `Stream`、`mainloop.loop_().iterate()` 等 API 不匹配
- **x11rb trait scope**：`Connection`/`ConnectionExt`/`WrapperConnectionExt` 提升到模块顶层，修复 `flush()`/`sync()`/`get_atom_name()` 找不到的问题
- **LinuxCapture Sync**：添加 `unsafe impl Sync for LinuxCapture` 满足 `ScreenCapture: Send + Sync` 约束
- **macOS DPI 测试**：`catch_unwind` 包裹 `NSScreen` 调用，修复无头 CI 环境 panic
- **sanitize_filename 跨平台**：先 `replace('\\', "/")` 再 `file_name()`，修复 macOS/Linux 上反斜杠未被识别为分隔符的问题
- **openh264 安全漏洞**：从 0.6 升级到 0.8，修复 RUSTSEC-2025-0008 堆溢出漏洞
- **VA-API clippy**：`div_ceil`、`is_multiple_of`、移除多余 cast、`identity_op`、`transmute` 注解
- **Clippy 全量修复**：`derivable_impls`、`single_match`、`while_let_loop`、`repeat_n`、`drop_non_drop`、`type_complexity`、dead code 等 30+ 处
- **前端 lint**：修复 22 处 `no-empty`、3 处 `Function` 类型、3 处 `prefer-const`
- **Android NDK 工具链**：设置 `CC_aarch64-linux-android` 等环境变量修复交叉编译找不到 clang
- **Android mobile 编译**：`state.server` 添加 `#[cfg(feature = "desktop")]` 分支，移动端使用默认端口
- **Release workflow**：修复 `secrets` 条件语法、`rm -f` PowerShell 兼容、产物路径 `src-tauri/target/` → `target/`

### Changed / 变更

- AppState 重构为 3 个语义分组（AuthState/ServerState/ConnState）
- session.rs 捕获/编码逻辑去重（提取 encode_and_build_frame）
- start_server 和 discover_peers 读取用户配置端口
- WebSocket broadcast channel 容量 32→128
- 被控端帧广播 channel 容量 8→32
- Raw RGB 渲染优化（Uint32Array 批量写入）
- **Settings 实时生效**：保存设置时通过 `apply_capture_settings` 命令将画质/帧率实时推送到后端
- **重置默认值同步后端**：恢复默认设置时同步重置后端带宽和捕获参数
- **发现协议**：`send_pong` 现在正确填充 `screen_width/screen_height`（原先硬编码为 0）
- **屏幕录制降级**：macOS WKWebView 不支持 MediaRecorder 时自动隐藏录制按钮
- **PIN 哈希算法升级**：从 SHA-256 升级为 Argon2id（内存 19MB，迭代 2 次，并行度 1），大幅提升抗暴力破解能力
- **安全存储**：PIN 和密码不再存入 localStorage，改为后端内存管理；TOFU 证书指纹持久化到磁盘
- **WebSocket 认证**：本地 WsBridge 新增 32 字节随机 token 认证，防止同机其他进程窃取帧数据
- **剪贴板大小限制**：文本最大 1MB，图片最大 10MB，防止内存耗尽
- **前端重构**：RemoteView.vue 拆分为 useAudio/useAnnotation/useRecording 三个 composable，消除约 200 行重复代码
- **色彩转换并行化**：BGRA→I420/NV12 使用 rayon 按行并行处理，提升多核 CPU 利用率
- **音频设备兼容性**：新增 loopback→麦克风两级 fallback 和 U16/I32 采样格式支持
- **NVENC 内存泄漏修复**：D3D11 设备引用计数正确管理，编码器销毁时释放

- **CI/CD**：新增 Linux 平台协议测试；clippy 检查范围扩大到 clipboard crate；Release 新增 SHA-256 checksum 文件
- **文档**：CHANGELOG v1.0.0 补充完整变更记录；CODE_OF_CONDUCT 添加联系邮箱；REMOTE_ACCESS 英文版补充 frp UDP 配置
- **协议**：新增 `CaptureSettings` 消息类型，支持前端动态调整画质和帧率
- **安全存储升级**：TLS 私钥加密从 SHA256-CTR (v2) 升级为 AES-256-GCM 认证加密 (v3)，密钥派生从裸 SHA-256 升级为 HKDF-SHA256，基于跨平台机器标识（Windows MachineGuid / Linux machine-id / macOS IOPlatformUUID）绑定，保持向后兼容自动迁移旧格式
- **PIN 内存安全**：自制 `ZeroizeString` 替换为 `zeroize` crate 的 `Zeroizing<String>`（`SecurePin` 包装），PIN 从 AppState 到 Server 全链路 drop 时自动清零
- **常量时间比较**：引入 `subtle` crate 替代 `auth.rs` 和 `integrity.rs` 中自实现的常量时间比较，统一为 `lan-desk-protocol` 公共函数
- **帧广播性能**：`EncodedFrame.regions` 改为 `Arc<Vec<DirtyRegion>>` 包装，多连接场景避免深拷贝帧数据
- **构建脚本**：Windows/macOS/Linux 三平台构建脚本统一 cmake 版本检测（>= 3.10）
- **CI**：所有 Linux CI job 添加 `libpipewire-0.3-dev` 系统依赖；移除 lint job 中重复的前端测试
- **前端常量提取**：`RemoteView.vue` 帧头大小提取为 `FRAME_HEADER_SIZE`；`useStats.ts` 延迟阈值提取为 `MAX_REASONABLE_LATENCY_MS`
- **.gitignore**：补充 `*.pfx` 和 `*.p12` 代码签名证书文件类型
- **PIN 码增强**：随机 PIN 从 6 位增加到 8 位（9000 万种组合 vs 90 万种）
- **语言检测默认英文**：检测不到系统语言时默认使用英文（而非中文）
- **Mutex 中毒恢复**：`lock().unwrap()` 改为 `lock().unwrap_or_else(|e| e.into_inner())`（17 处）
- **PipeWire 错误传播**：6 处 `expect()` 改为 `?` + `map_err`，不再 panic
- **目录传输路径统一**：`to_string_lossy()` 后添加 `.replace('\\', "/")`，确保跨平台兼容
- **is_safe_path 符号链接**：验证前先 `canonicalize()` 解析符号链接
- **Android 设备 ID**：移动端改用持久化 UUID，避免 hostname 重复
- **Cargo.toml 元数据**：所有 crate 添加 `description` 和 `repository`（workspace 继承）
- **Android/iOS CI**：`continue-on-error: true`，移动端构建失败不阻塞桌面端发布

---

## [v1.0.0] - 2026-04-06 (English)

First official release with full-featured LAN remote desktop capabilities.

### Added (Core Features)

- **Multi-monitor precise coordinate mapping**: InputInjector `set_active_monitor()` method; `cursor_position()` and `move_mouse()` normalize to the active captured monitor (not the entire virtual desktop). Windows/macOS/Linux/Wayland all adapted
- **Remote monitor list push**: Host sends `MonitorList` protocol message (with left/top pixel offsets) after session establishment; `SwitchMonitor` auto-updates input coordinate mapping
- Collapsible right-side tool panel (180px) for remote view; top bar reduced to 28px status-only strip, no longer blocking the remote desktop taskbar
- **Connection status display**: Host Discovery page shows "Connected Clients" (hostname/IP/role); Controller shows "Active Remote Sessions" (clickable to switch)
- **Host chat sending**: Broadcasts to all remote controllers via broadcast channel (fixed host unable to send chat)
- **Shared capture monitor switch**: `shared_capture_loop` supports `CaptureCommand`, multi-monitor switch commands now reach the capture thread
- **macOS Retina coordinate fix**: `set_active_monitor` uses `CGDisplay.bounds()` for logical point dimensions, fixing Retina screen scaling
- **Fixed password startup order**: Settings synced to backend before starting server (fixed connection rejection after restart with fixed passwords)
- **Mobile compilation fixes**: `get_device_id` / Opus decoding / `ClipboardChange` type properly gated with `#[cfg]`
- **MonitorList race condition fix**: Cache remote monitor list in ConnState; RemoteView fetches via `get_remote_monitors` on mount (fixes event lost before listener registered)
- **Dynamic terminal settings**: `shell_enabled` changed to `Arc<AtomicBool>`, running Server reads latest value on each accept
- **WebM recording seekbar fix**: Auto-inject Duration metadata into EBML header after recording stops
- **Complete User Guide** `docs/USER_GUIDE.md`: Bilingual, 12 chapters covering all features
- **Code Signing Guide** `docs/CODE_SIGNING.md`: iOS/macOS/Windows/Android signing setup
- **cargo audit** CI: Automatic dependency vulnerability scanning
- **Android auto-signing**: Release CI supports APK signing via GitHub Secrets
- **Android x86_64 support**: APK includes ARM64 + x86_64 for emulator compatibility

- PIN brute-force protection: exponential backoff lockout (5 failures → 5min → 15min → 45min → … → 24h max), global rate limit of 10 failures per minute
- File transfer filters Windows reserved device names (CON/NUL/AUX/PRN/COM1-9/LPT1-9) to prevent writing to system devices
- Remote terminal (PTY Shell) 30-minute idle timeout auto-close + structured audit logging (target: "audit")
- TOFU certificate management UI: view and revoke trusted remote host fingerprints in Settings
- macOS: Auto-detect and guide Screen Recording (CGPreflightScreenCaptureAccess) and Accessibility (AXIsProcessTrusted) permissions
- Windows: Multi-monitor DPI uses GetDpiForSystem, mouse coordinates use virtual screen (SM_CXVIRTUALSCREEN + MOUSEEVENTF_VIRTUALDESK)
- macOS: Clipboard change detection uses NSPasteboard.changeCount native FFI
- Vitest frontend unit testing framework + example test cases
- CI adds ESLint frontend code linting
- useToast composable for unified Toast notification management
- Windows: PowerShell remote terminal forces UTF-8 encoding (-NoExit -Command "[Console]::OutputEncoding = UTF8")
- Linux/macOS: Shell startup auto-sets LANG/LC_ALL=en_US.UTF-8 (if not configured)
- JPEG encoder buffer reuse (rgb_buf/jpeg_buf promoted to struct fields)
- Color conversion (BGRA→I420) chunks_exact(4) optimization + cross-frame output buffer reuse
- TLS certificate persistence: reuse cert across restarts, maintain TOFU fingerprint
- Linux VAAPI hardware encoding detection (placeholder, auto-fallback to OpenH264)
- Native Wayland screen capture detection (placeholder, auto-fallback to XWayland)
- PipeWire/Portal native Wayland screen capture: zero external tool capture via XDG Desktop Portal + PipeWire, three-level fallback (PipeWire/Portal → Wayland external tools → X11 XShm), system authorization dialog on first use with restore_token auto-skip on subsequent use; supports GNOME, KDE, Sway, Hyprland and other major Wayland compositors; feature gate `pipewire-capture` enabled by default
- macOS cursor shape sync (NSCursor, 8 cursor types)
- Linux cursor shape sync (XFixes, 12 cursor types)
- System tray menu i18n (auto-detect Chinese/English)
- CI Linux build job
- CI code signing support (Windows Authenticode + macOS notarize)
- DPI detection enhancement: Linux QT_SCALE_FACTOR/GDK_DPI_SCALE, macOS Retina 2x
- AudioContext suspended auto-resume
- JPEG frame sequence number anti-reordering (WebSocket path)
- WebSocket binary transport channel (frame/audio data bypasses Tauri JSON IPC, eliminates base64 ~33% overhead)
- Opus audio encoding (bandwidth reduced from ~1.5Mbps to ~128kbps, 92% reduction, with PCM auto-fallback)
- Full GPU hardware encoder implementation (NVENC Windows / VideoToolbox macOS, auto-detection + OpenH264 fallback)
- Shared color space conversion module (bgra_to_i420 + bgra_to_nv12, used by both OpenH264 and NVENC)
- Linux platform support (X11 SHM screen capture + XTest input injection + ALSA/PulseAudio audio)
- Remote terminal (PTY Shell bidirectional I/O, powered by xterm.js)
- Dual-PIN permission control (Control PIN for full access / View PIN for view-only)
- Unattended mode (fixed password + auto-accept connections)
- Host server crate `lan-desk-server` (TLS TCP listener, session management, shared frame broadcast)
- i18n expanded to 229 translation keys, full UI coverage
- Authorization popup shows requested permission type (Full Control / View Only)
- Clipboard sync toggle: can be disabled in Settings
- Remote terminal toggle: can be disabled in Settings for enhanced security
- Wayland: added spectacle (KDE) screen capture support
- Wayland: added dotool input injection support (no root required)
- AMD AMF / Intel QSV hardware encoder support (skeleton)
- Protocol layer migrated from bincode to MessagePack for forward compatibility
- Windows clipboard uses GetClipboardSequenceNumber for optimized change detection
- Major frontend refactor: commands.rs / session.rs / RemoteView.vue modular split
- Added useSettings composable for unified settings management
- Version consistency check script `scripts/check_versions.py` — validates version numbers across 9 files
- Added JPEG path unit tests for `useFrameRenderer` (IPC + WebSocket binary modes)
- Added 8 serialization round-trip tests for `message.rs`
- Added 7 binary encoding unit tests for `ws_bridge.rs` (frame header, audio format, audio data, cursor mapping)
- Added 6 unit tests for `ct_eq_str` / `ct_eq_bytes` constant-time comparison functions
- Text chat: instant text messaging during remote sessions
- Special key passthrough: supports Ctrl+Alt+Del, Alt+Tab, Alt+F4 and other system shortcuts to remote host
- Bidirectional file transfer + remote file browser (dual-pane layout, supports upload/download)
- Directory transfer + resume: supports entire directory transfer with resume capability
- H.265/HEVC hardware encoding: NVENC HEVC encoding with automatic H.264 fallback
- AV1 encoding support (interface reserved, to be enabled when GPU hardware encoders become available)
- Session idle timeout: configurable idle timeout (default 30 minutes), auto-disconnect on expiry
- Auto lock screen on disconnect: automatically locks host screen when session disconnects (cross-platform, configurable)
- Screen blanking: prevents bystanders on host side from viewing remote operations (configurable)
- Auto reconnect after reboot: automatically resumes session after detecting remote host reboot
- Adaptive bitrate: dynamically adjusts encoding quality based on RTT and bandwidth utilization
- Multi-tab / multi-session: connect to multiple remote devices simultaneously with tab switching
- Dark / light theme toggle: supports following system theme for automatic switching
- Portable mode: no installation required, place a `.portable` marker file in the same directory as the exe
- File transfer cancel: cancel file transfers at any time during transmission
- Directory download: download entire directories from the remote host
- Drag & drop file upload: drag files onto the remote desktop window to upload directly
- Autostart on login: enable automatic startup on login in Settings
- Parallel JPEG encoding: rayon multi-core parallel encoding of dirty regions
- Connection history alias: set custom names for connection history entries
- Resizable terminal panel: drag to resize the terminal panel height
- Toolbar dropdown menus: tools and remote control grouped into dropdown menus
- Live encoder type display (H.264/HEVC/JPEG)
- Configurable audio quality (Low/Medium/High)
- RTT sparkline trend chart
- TOFU certificate change confirmation dialog
- Device ID: 9-digit unique identifier
- Session recording playback: IndexedDB storage + built-in player
- File transfer completion notification: system Notification alert
- Remote screenshot save: one-click PNG screenshot
- PIN remember: optionally remember connection password
- Annotation text tool: brush + text dual tools
- Multi-monitor dropdown menu: shows resolution and primary display indicator
- Keyboard shortcut help panel: F1 to open
- Connection history export: CSV format
- Portable mode tray indicator: [P] mark
- Chat message notification sound
- Enhanced system info: total memory display
- Connection info copy: Device ID + port
- Device ID connection: enter 9-digit ID for automatic UDP scan and connect
- Tailscale/ZeroTier auto-detection: VPN virtual interface IPs highlighted
- Android / iOS mobile support: phone/tablet as controller with touch gestures + virtual keyboard + responsive layout

### Fixed

- Windows scroll wheel direction (i32→u32 bit-reinterpret clarified)
- NumpadEnter missing extended key flag
- Windows primary monitor detection (MONITORINFOF_PRIMARY instead of index)
- Windows mouse coordinate mapping after resolution change
- Linux GetImage fallback stride calculation
- macOS pixel format detection (bitmap_info check for BGRA/RGBA)
- File transfer out-of-order chunk support (seek+write instead of append)
- JPEG frame sequence check logic (=== instead of >=)
- QUICK_START.md screenshot paths
- Remote terminal Chinese Windows GBK encoding (switched to PowerShell + UTF-8 env)
- **RemoteView resource leak**: Fix 4 Tauri event listeners not cleaned up on unmount, VideoDecoder/AudioContext not closed
- **H.264 timestamp**: Use strictly incrementing counter instead of `Date.now()` to avoid duplicate timestamps
- **integrity.ts SPA compatibility**: Fix `window.load` event possibly not firing in SPA context
- **session.rs async IO**: File transfer receiving now uses `tokio::fs` instead of blocking `std::fs`
- **macOS scroll wheel**: Use CoreGraphics C FFI (`CGEventCreateScrollEvent`) instead of removed `new_scroll_event` wrapper, compatible with core-graphics 0.24+
- **File transfer size limit**: Added 2GB max file size limit on receiver side
- **PIN log masking**: Server startup log now only shows first 3 digits of PIN
- **Server accept() stability**: Transient TCP accept errors no longer terminate the server loop — logged and continued
- **useSettings reset()**: `loaded` flag properly reset to `false`, ensuring subsequent `load()` reads from localStorage
- **Discovery scan error feedback**: `scanPeers` failures now show `toast.error` instead of silent `console.error`
- **build.sh Linux**: Added cargo/node prerequisite checks for Linux branch
- **PipeWire Portal API**: Fixed `ashpd` 0.10 import paths, `Request.response()`, `SourceType.into()`, `OwnedFd`, `StreamRef`, `mainloop.loop_().iterate()`
- **x11rb trait scope**: Moved `Connection`/`ConnectionExt` to module-level imports
- **LinuxCapture Sync**: Added `unsafe impl Sync` for `ScreenCapture` trait bound
- **macOS DPI test**: `catch_unwind` for headless CI environments
- **sanitize_filename cross-platform**: Normalize `\` to `/` before `file_name()`
- **openh264 vulnerability**: Upgraded 0.6 → 0.8, fixing RUSTSEC-2025-0008
- **30+ clippy fixes**: `derivable_impls`, `single_match`, `div_ceil`, `is_multiple_of`, dead code, etc.
- **Frontend lint**: Fixed 22 `no-empty`, 3 `Function` type, 3 `prefer-const` errors
- **Android NDK toolchain**: Set `CC_aarch64-linux-android` env vars for cross-compilation
- **Release workflow**: Fixed `secrets` condition syntax, PowerShell `rm -f`, artifact paths

### Changed

- AppState refactored into 3 semantic groups (AuthState/ServerState/ConnState)
- session.rs capture/encode deduplication (extracted encode_and_build_frame)
- start_server and discover_peers read user-configured port
- WebSocket broadcast channel capacity 32→128
- Host frame broadcast channel capacity 8→32
- Raw RGB rendering optimization (Uint32Array batch write)
- **Settings take effect immediately**: Save now pushes quality/fps to backend via `apply_capture_settings`
- **Reset defaults syncs backend**: Reset to defaults now also resets backend bandwidth and capture settings
- **Discovery protocol**: `send_pong` now correctly fills `screen_width/screen_height` (was hardcoded to 0)
- **Screen recording fallback**: Hide record button on macOS WKWebView when MediaRecorder is unsupported
- **PIN hash algorithm upgrade**: Upgraded from SHA-256 to Argon2id (19MB memory, 2 iterations, parallelism 1), significantly improving brute-force resistance
- **Secure storage**: PIN and passwords no longer stored in localStorage, managed in backend memory; TOFU certificate fingerprints persisted to disk
- **WebSocket authentication**: Local WsBridge now requires 32-byte random token authentication, preventing other processes from intercepting frame data
- **Clipboard size limit**: Text max 1MB, image max 10MB, preventing memory exhaustion
- **Frontend refactor**: RemoteView.vue split into useAudio/useAnnotation/useRecording composables, eliminating ~200 lines of duplicate code
- **Color conversion parallelization**: BGRA→I420/NV12 uses rayon row-level parallelism for better multi-core CPU utilization
- **Audio device compatibility**: Added loopback→microphone two-level fallback and U16/I32 sample format support
- **NVENC memory leak fix**: D3D11 device reference counting properly managed, released on encoder drop

- **CI/CD**: Added Linux protocol test job; expanded clippy scope; release builds now generate SHA-256 checksums
- **Docs**: Expanded CHANGELOG v1.0.0; added contact email to CODE_OF_CONDUCT; added frp UDP config to English REMOTE_ACCESS
- **Protocol**: Added `CaptureSettings` message type for dynamic quality/fps adjustment
- **Secure storage upgrade**: TLS private key encryption upgraded from SHA256-CTR (v2) to AES-256-GCM authenticated encryption (v3), key derivation upgraded from bare SHA-256 to HKDF-SHA256 based on cross-platform machine identity (Windows MachineGuid / Linux machine-id / macOS IOPlatformUUID), backward compatible with automatic migration from legacy formats
- **PIN memory safety**: Custom `ZeroizeString` replaced with `zeroize` crate's `Zeroizing<String>` (`SecurePin` wrapper), PIN automatically zeroed on drop across the full chain from AppState to Server
- **Constant-time comparison**: Introduced `subtle` crate replacing hand-rolled implementations in `auth.rs` and `integrity.rs`, unified as public functions in `lan-desk-protocol`
- **Frame broadcast performance**: `EncodedFrame.regions` wrapped in `Arc<Vec<DirtyRegion>>`, avoiding deep copies in multi-connection scenarios
- **Build scripts**: Unified cmake version detection (>= 3.10) across Windows/macOS/Linux build scripts
- **CI**: All Linux CI jobs now install `libpipewire-0.3-dev`; removed duplicate frontend test from lint job
- **Frontend constants**: Extracted magic numbers into named constants (`FRAME_HEADER_SIZE`, `MAX_REASONABLE_LATENCY_MS`)
- **.gitignore**: Added `*.pfx` and `*.p12` code signing certificate patterns
- **PIN strength**: Random PIN increased from 6 to 8 digits (90M vs 900K combinations)
- **Language detection**: Default to English when system language is undetectable
- **Mutex poisoning recovery**: 17 `lock().unwrap()` → `lock().unwrap_or_else(|e| e.into_inner())`
- **PipeWire error propagation**: 6 `expect()` → `?` with `map_err`
- **Directory transfer paths**: Normalize `\` to `/` for cross-platform compatibility
- **is_safe_path symlink**: `canonicalize()` before validation
- **Android Device ID**: Persistent UUID instead of hostname
- **Cargo.toml metadata**: Added `description` and `repository` to all crates
- **Android/iOS CI**: `continue-on-error: true` — mobile failures don't block desktop releases

<!-- Version comparison links -->
[v1.0.0]: https://github.com/bbyybb/lan-desk/releases/tag/v1.0.0
