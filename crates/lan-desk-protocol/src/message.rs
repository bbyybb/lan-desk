use serde::{Deserialize, Serialize};

/// 会话角色（权限级别）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum SessionRole {
    /// 完全控制（键鼠、文件、剪贴板、Shell）
    #[default]
    Controller,
    /// 仅查看（只能观看屏幕）
    Viewer,
}

/// 所有网络消息的统一类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Message {
    // === 连接握手 ===
    Hello {
        version: u32,
        hostname: String,
        screen_width: u32,
        screen_height: u32,
        /// 连接密码（Argon2id 哈希后的 PIN）
        pin: String,
        /// PIN 哈希的随机 salt（每次连接随机生成，防预计算攻击）
        pin_salt: String,
        /// DPI 缩放比例（100 = 100%，150 = 150%）
        dpi_scale: u32,
        /// 请求的会话角色
        requested_role: SessionRole,
    },
    HelloAck {
        version: u32,
        accepted: bool,
        /// 拒绝原因（accepted=false 时有值）
        reject_reason: String,
        /// 实际授予的角色
        granted_role: SessionRole,
    },

    // === 屏幕数据 ===
    FrameData {
        seq: u64,
        timestamp_ms: u64,
        regions: Vec<DirtyRegion>,
        /// 被控端当前鼠标坐标（归一化 0.0~1.0）
        cursor_x: f64,
        cursor_y: f64,
        /// 光标形状类型
        cursor_shape: CursorShape,
    },
    /// 保留：帧确认（协议预留字段，预留用于未来的流控机制，当前版本不发送/不处理）
    /// NOTE: 此变体为协议预留字段，暂未使用。保留以确保协议版本向前兼容，
    /// 未来版本可能启用此字段实现帧级流控。请勿移除。
    #[doc(hidden)]
    FrameAck {
        seq: u64,
    },

    // === 输入事件（控制端 -> 被控端）===
    MouseMove {
        x: f64,
        y: f64,
    },
    MouseButton {
        button: MouseBtn,
        pressed: bool,
    },
    MouseScroll {
        dx: f64,
        dy: f64,
    },
    KeyEvent {
        /// 物理键位标识（KeyboardEvent.code，如 "KeyA", "Enter"）
        code: String,
        pressed: bool,
        modifiers: u8,
    },

    // === 剪贴板 ===
    ClipboardUpdate {
        content_type: ClipboardContentType,
        data: Vec<u8>,
    },

    // === 多显示器 ===
    /// 被控端 -> 控制端：可用显示器列表
    MonitorList {
        monitors: Vec<MonitorInfo>,
    },
    /// 控制端 -> 被控端：切换到指定显示器
    SwitchMonitor {
        index: u32,
    },

    // === 文件传输 ===
    /// 发起文件传输请求
    FileTransferStart {
        filename: String,
        size: u64,
        transfer_id: u32,
    },
    /// 文件数据块
    FileTransferData {
        transfer_id: u32,
        offset: u64,
        data: Vec<u8>,
    },
    /// 文件传输完成（含校验和）
    FileTransferComplete {
        transfer_id: u32,
        /// SHA256 校验和（hex 字符串）
        checksum: String,
    },

    // === 白板标注 ===
    /// 标注绘制数据（归一化坐标）
    Annotation {
        /// 线条颜色 (CSS 格式, 如 "#ff0000")
        color: String,
        /// 线宽 (像素)
        width: f32,
        /// 路径点 (归一化 0.0~1.0)
        points: Vec<(f64, f64)>,
    },
    /// 清除所有标注
    AnnotationClear,

    // === 音频 ===
    /// 音频格式信息（连接建立后发送一次）
    AudioFormat {
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
        encoding: AudioEncoding,
    },
    /// 音频数据块
    AudioData {
        data: Vec<u8>,
        encoding: AudioEncoding,
    },

    // === 远程终端 ===
    /// 请求启动远程 Shell
    ShellStart {
        cols: u16,
        rows: u16,
    },
    /// Shell 启动结果
    ShellStartAck {
        success: bool,
        error: String,
    },
    /// Shell 数据（双向：stdin/stdout）
    ShellData {
        data: Vec<u8>,
    },
    /// Shell 窗口大小调整
    ShellResize {
        cols: u16,
        rows: u16,
    },
    /// 关闭 Shell
    ShellClose,

    // === 控制 ===
    /// 系统信息（被控端定期发送）
    SystemInfo {
        cpu_usage: f32,
        memory_usage: f32,
        memory_total_mb: u64,
    },

    /// 带宽限制设置
    SetBandwidthLimit {
        bytes_per_sec: u64,
    },

    /// 捕获参数设置（画质、帧率）
    CaptureSettings {
        jpeg_quality: u8,
        max_fps: u32,
    },

    // === 文字聊天 ===
    ChatMessage {
        text: String,
        sender: String,
        timestamp_ms: u64,
    },

    // === 快捷键透传 ===
    SpecialKey {
        key: SpecialKeyType,
    },

    // === 屏幕遮蔽 ===
    ScreenBlank {
        enable: bool,
    },

    // === 远程锁屏 ===
    LockScreen,

    // === 远程重启 ===
    RemoteReboot,
    /// 被控端即将重启通知
    RebootPending {
        estimated_seconds: u32,
    },

    // === 双向文件传输：远程文件浏览 ===
    FileListRequest {
        path: String,
        request_id: u32,
    },
    FileListResponse {
        request_id: u32,
        path: String,
        entries: Vec<FileEntry>,
        error: String,
    },
    /// 请求下载被控端文件（反向传输）
    FileDownloadRequest {
        path: String,
        transfer_id: u32,
    },

    /// 请求下载被控端目录（反向目录传输）
    DirectoryDownloadRequest {
        path: String,
        transfer_id: u32,
    },

    // === 目录传输 ===
    DirectoryTransferStart {
        transfer_id: u32,
        base_path: String,
        total_files: u32,
        total_size: u64,
    },
    DirectoryEntry {
        transfer_id: u32,
        relative_path: String,
        is_dir: bool,
        size: u64,
    },
    /// 断点续传：接收端通知发送端从指定偏移继续
    FileTransferResume {
        transfer_id: u32,
        offset: u64,
    },

    Disconnect,
    Ping {
        timestamp_ms: u64,
    },
    Pong {
        timestamp_ms: u64,
    },
}

/// 光标形状类型
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", content = "c")]
pub enum CursorShape {
    #[default]
    Arrow,
    IBeam,
    Hand,
    Crosshair,
    ResizeNS,
    ResizeEW,
    ResizeNESW,
    ResizeNWSE,
    Move,
    Wait,
    Help,
    NotAllowed,
    Hidden,
}

/// 显示器信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
    /// 显示器在虚拟桌面中的左上角 X 偏移（像素）
    #[serde(default)]
    pub left: i32,
    /// 显示器在虚拟桌面中的左上角 Y 偏移（像素）
    #[serde(default)]
    pub top: i32,
}

/// 屏幕变化区域
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirtyRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub encoding: FrameEncoding,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum FrameEncoding {
    Jpeg {
        quality: u8,
    },
    Raw,
    /// H.264 编码帧
    H264 {
        is_keyframe: bool,
    },
    /// H.265/HEVC 编码帧
    H265 {
        is_keyframe: bool,
    },
    /// AV1 编码帧
    Av1 {
        is_keyframe: bool,
    },
}

/// 特殊按键类型（快捷键透传）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", content = "c")]
pub enum SpecialKeyType {
    CtrlAltDel,
    AltTab,
    AltF4,
    PrintScreen,
    WinKey,
    /// Win+L（锁屏快捷键）
    WinL,
    CtrlEsc,
}

/// 远程文件条目（文件浏览器）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", content = "c")]
pub enum MouseBtn {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", content = "c")]
pub enum ClipboardContentType {
    PlainText,
    Image,
}

/// 音频编码格式
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", content = "c")]
pub enum AudioEncoding {
    /// 原始 PCM 16-bit little-endian
    #[default]
    Pcm16,
    /// Opus 压缩编码
    Opus,
}

/// 修饰键位掩码
pub mod modifiers {
    pub const SHIFT: u8 = 0b0000_0001;
    pub const CTRL: u8 = 0b0000_0010;
    pub const ALT: u8 = 0b0000_0100;
    pub const META: u8 = 0b0000_1000;
}

/// 局域网发现消息（通过 UDP 广播）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum DiscoveryMessage {
    Ping {
        hostname: String,
        tcp_port: u16,
        #[serde(default)]
        device_id: String,
    },
    Pong {
        hostname: String,
        tcp_port: u16,
        os: String,
        screen_width: u32,
        screen_height: u32,
        #[serde(default)]
        device_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 辅助函数：序列化后反序列化，验证往返一致性
    fn roundtrip<T: Serialize + serde::de::DeserializeOwned>(val: &T) -> T {
        let bytes = rmp_serde::to_vec_named(val).expect("序列化失败");
        rmp_serde::from_slice(&bytes).expect("反序列化失败")
    }

    #[test]
    fn test_hello_roundtrip() {
        let msg = Message::Hello {
            version: 3,
            hostname: "test-host".to_string(),
            screen_width: 2560,
            screen_height: 1440,
            pin: "abcdef".to_string(),
            pin_salt: "0123456789abcdef".to_string(),
            dpi_scale: 200,
            requested_role: SessionRole::Controller,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::Hello {
                version,
                hostname,
                screen_width,
                screen_height,
                pin,
                pin_salt,
                dpi_scale,
                requested_role,
            } => {
                assert_eq!(version, 3);
                assert_eq!(hostname, "test-host");
                assert_eq!(screen_width, 2560);
                assert_eq!(screen_height, 1440);
                assert_eq!(pin, "abcdef");
                assert_eq!(pin_salt, "0123456789abcdef");
                assert_eq!(dpi_scale, 200);
                assert_eq!(requested_role, SessionRole::Controller);
            }
            _ => panic!("期望 Hello 变体"),
        }
    }

    #[test]
    fn test_hello_ack_roundtrip() {
        let msg = Message::HelloAck {
            version: 3,
            accepted: true,
            reject_reason: String::new(),
            granted_role: SessionRole::Viewer,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::HelloAck {
                version,
                accepted,
                reject_reason,
                granted_role,
            } => {
                assert_eq!(version, 3);
                assert!(accepted);
                assert_eq!(reject_reason, "");
                assert_eq!(granted_role, SessionRole::Viewer);
            }
            _ => panic!("期望 HelloAck 变体"),
        }
    }

    #[test]
    fn test_mouse_move_roundtrip() {
        let msg = Message::MouseMove { x: 0.25, y: 0.75 };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::MouseMove { x, y } => {
                assert!((x - 0.25).abs() < f64::EPSILON);
                assert!((y - 0.75).abs() < f64::EPSILON);
            }
            _ => panic!("期望 MouseMove 变体"),
        }
    }

    #[test]
    fn test_key_event_roundtrip() {
        let msg = Message::KeyEvent {
            code: "Enter".to_string(),
            pressed: true,
            modifiers: modifiers::CTRL | modifiers::SHIFT,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::KeyEvent {
                code,
                pressed,
                modifiers: mods,
            } => {
                assert_eq!(code, "Enter");
                assert!(pressed);
                assert_eq!(mods, modifiers::CTRL | modifiers::SHIFT);
            }
            _ => panic!("期望 KeyEvent 变体"),
        }
    }

    #[test]
    fn test_frame_data_roundtrip() {
        let msg = Message::FrameData {
            seq: 100,
            timestamp_ms: 5000,
            regions: vec![
                DirtyRegion {
                    x: 10,
                    y: 20,
                    width: 640,
                    height: 480,
                    encoding: FrameEncoding::Jpeg { quality: 85 },
                    data: vec![0xAA; 50],
                },
                DirtyRegion {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                    encoding: FrameEncoding::Raw,
                    data: vec![0xBB; 30],
                },
            ],
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_shape: CursorShape::IBeam,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::FrameData {
                seq,
                timestamp_ms,
                regions,
                cursor_x,
                cursor_y,
                cursor_shape,
            } => {
                assert_eq!(seq, 100);
                assert_eq!(timestamp_ms, 5000);
                assert_eq!(regions.len(), 2);
                assert_eq!(regions[0].x, 10);
                assert_eq!(regions[0].width, 640);
                assert_eq!(regions[0].data.len(), 50);
                assert_eq!(regions[1].data, vec![0xBB; 30]);
                assert!((cursor_x - 0.5).abs() < f64::EPSILON);
                assert!((cursor_y - 0.5).abs() < f64::EPSILON);
                assert_eq!(cursor_shape, CursorShape::IBeam);
            }
            _ => panic!("期望 FrameData 变体"),
        }
    }

    #[test]
    fn test_disconnect_roundtrip() {
        let msg = Message::Disconnect;
        let decoded = roundtrip(&msg);
        assert!(matches!(decoded, Message::Disconnect));
    }

    #[test]
    fn test_session_role_roundtrip() {
        let controller = SessionRole::Controller;
        let viewer = SessionRole::Viewer;

        let decoded_controller = roundtrip(&controller);
        let decoded_viewer = roundtrip(&viewer);

        assert_eq!(decoded_controller, SessionRole::Controller);
        assert_eq!(decoded_viewer, SessionRole::Viewer);
    }

    #[test]
    fn test_chat_message_roundtrip() {
        let msg = Message::ChatMessage {
            text: "你好，世界！".to_string(),
            sender: "admin".to_string(),
            timestamp_ms: 1700000000000,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::ChatMessage {
                text,
                sender,
                timestamp_ms,
            } => {
                assert_eq!(text, "你好，世界！");
                assert_eq!(sender, "admin");
                assert_eq!(timestamp_ms, 1700000000000);
            }
            _ => panic!("期望 ChatMessage 变体"),
        }
    }

    #[test]
    fn test_special_key_roundtrip() {
        let msg = Message::SpecialKey {
            key: SpecialKeyType::CtrlAltDel,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::SpecialKey { key } => assert_eq!(key, SpecialKeyType::CtrlAltDel),
            _ => panic!("期望 SpecialKey 变体"),
        }

        // 测试其他特殊按键
        for key_type in [
            SpecialKeyType::AltTab,
            SpecialKeyType::AltF4,
            SpecialKeyType::PrintScreen,
            SpecialKeyType::WinKey,
            SpecialKeyType::WinL,
            SpecialKeyType::CtrlEsc,
        ] {
            let msg = Message::SpecialKey { key: key_type };
            let decoded = roundtrip(&msg);
            match decoded {
                Message::SpecialKey { key } => assert_eq!(key, key_type),
                _ => panic!("期望 SpecialKey 变体"),
            }
        }
    }

    #[test]
    fn test_screen_blank_roundtrip() {
        let msg = Message::ScreenBlank { enable: true };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::ScreenBlank { enable } => assert!(enable),
            _ => panic!("期望 ScreenBlank 变体"),
        }

        let msg = Message::ScreenBlank { enable: false };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::ScreenBlank { enable } => assert!(!enable),
            _ => panic!("期望 ScreenBlank 变体"),
        }
    }

    #[test]
    fn test_lock_screen_roundtrip() {
        let msg = Message::LockScreen;
        let decoded = roundtrip(&msg);
        assert!(matches!(decoded, Message::LockScreen));
    }

    #[test]
    fn test_remote_reboot_roundtrip() {
        let msg = Message::RemoteReboot;
        let decoded = roundtrip(&msg);
        assert!(matches!(decoded, Message::RemoteReboot));
    }

    #[test]
    fn test_reboot_pending_roundtrip() {
        let msg = Message::RebootPending {
            estimated_seconds: 30,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::RebootPending { estimated_seconds } => assert_eq!(estimated_seconds, 30),
            _ => panic!("期望 RebootPending 变体"),
        }
    }

    #[test]
    fn test_file_list_request_roundtrip() {
        let msg = Message::FileListRequest {
            path: "C:\\Users\\test".to_string(),
            request_id: 42,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::FileListRequest { path, request_id } => {
                assert_eq!(path, "C:\\Users\\test");
                assert_eq!(request_id, 42);
            }
            _ => panic!("期望 FileListRequest 变体"),
        }
    }

    #[test]
    fn test_file_list_response_roundtrip() {
        let msg = Message::FileListResponse {
            request_id: 42,
            path: "/home/user".to_string(),
            entries: vec![
                FileEntry {
                    name: "docs".to_string(),
                    is_dir: true,
                    size: 0,
                    modified_ms: 1700000000000,
                },
                FileEntry {
                    name: "readme.txt".to_string(),
                    is_dir: false,
                    size: 1024,
                    modified_ms: 1700000001000,
                },
            ],
            error: String::new(),
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::FileListResponse {
                request_id,
                path,
                entries,
                error,
            } => {
                assert_eq!(request_id, 42);
                assert_eq!(path, "/home/user");
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].name, "docs");
                assert!(entries[0].is_dir);
                assert_eq!(entries[1].name, "readme.txt");
                assert!(!entries[1].is_dir);
                assert_eq!(entries[1].size, 1024);
                assert!(error.is_empty());
            }
            _ => panic!("期望 FileListResponse 变体"),
        }
    }

    #[test]
    fn test_file_download_request_roundtrip() {
        let msg = Message::FileDownloadRequest {
            path: "/tmp/data.bin".to_string(),
            transfer_id: 99,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::FileDownloadRequest { path, transfer_id } => {
                assert_eq!(path, "/tmp/data.bin");
                assert_eq!(transfer_id, 99);
            }
            _ => panic!("期望 FileDownloadRequest 变体"),
        }
    }

    #[test]
    fn test_directory_transfer_start_roundtrip() {
        let msg = Message::DirectoryTransferStart {
            transfer_id: 7,
            base_path: "project/src".to_string(),
            total_files: 150,
            total_size: 5_000_000,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::DirectoryTransferStart {
                transfer_id,
                base_path,
                total_files,
                total_size,
            } => {
                assert_eq!(transfer_id, 7);
                assert_eq!(base_path, "project/src");
                assert_eq!(total_files, 150);
                assert_eq!(total_size, 5_000_000);
            }
            _ => panic!("期望 DirectoryTransferStart 变体"),
        }
    }

    #[test]
    fn test_directory_download_request_roundtrip() {
        let msg = Message::DirectoryDownloadRequest {
            path: "/home/user/Documents".to_string(),
            transfer_id: 80001,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::DirectoryDownloadRequest { path, transfer_id } => {
                assert_eq!(path, "/home/user/Documents");
                assert_eq!(transfer_id, 80001);
            }
            _ => panic!("期望 DirectoryDownloadRequest 变体"),
        }
    }

    #[test]
    fn test_file_transfer_resume_roundtrip() {
        let msg = Message::FileTransferResume {
            transfer_id: 3,
            offset: 1048576,
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::FileTransferResume {
                transfer_id,
                offset,
            } => {
                assert_eq!(transfer_id, 3);
                assert_eq!(offset, 1048576);
            }
            _ => panic!("期望 FileTransferResume 变体"),
        }
    }

    #[test]
    fn test_frame_encoding_h265_roundtrip() {
        let region = DirtyRegion {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            encoding: FrameEncoding::H265 { is_keyframe: true },
            data: vec![0x00, 0x00, 0x01, 0x40],
        };
        let decoded = roundtrip(&region);
        assert_eq!(decoded.x, 0);
        assert_eq!(decoded.width, 1920);
        match decoded.encoding {
            FrameEncoding::H265 { is_keyframe } => assert!(is_keyframe),
            _ => panic!("期望 H265 编码"),
        }
    }

    #[test]
    fn test_frame_encoding_av1_roundtrip() {
        let region = DirtyRegion {
            x: 100,
            y: 200,
            width: 640,
            height: 480,
            encoding: FrameEncoding::Av1 { is_keyframe: false },
            data: vec![0x12, 0x00, 0x0A],
        };
        let decoded = roundtrip(&region);
        assert_eq!(decoded.x, 100);
        assert_eq!(decoded.y, 200);
        match decoded.encoding {
            FrameEncoding::Av1 { is_keyframe } => assert!(!is_keyframe),
            _ => panic!("期望 Av1 编码"),
        }
    }

    #[test]
    fn test_discovery_message_roundtrip() {
        let ping = DiscoveryMessage::Ping {
            hostname: "my-pc".to_string(),
            tcp_port: 25605,
            device_id: "123456789".to_string(),
        };
        let decoded_ping = roundtrip(&ping);
        match decoded_ping {
            DiscoveryMessage::Ping {
                hostname,
                tcp_port,
                device_id,
            } => {
                assert_eq!(hostname, "my-pc");
                assert_eq!(tcp_port, 25605);
                assert_eq!(device_id, "123456789");
            }
            _ => panic!("期望 DiscoveryMessage::Ping"),
        }

        let pong = DiscoveryMessage::Pong {
            hostname: "remote-pc".to_string(),
            tcp_port: 25605,
            os: "Windows".to_string(),
            screen_width: 1920,
            screen_height: 1080,
            device_id: "987654321".to_string(),
        };
        let decoded_pong = roundtrip(&pong);
        match decoded_pong {
            DiscoveryMessage::Pong {
                hostname,
                tcp_port,
                os,
                screen_width,
                screen_height,
                device_id,
            } => {
                assert_eq!(hostname, "remote-pc");
                assert_eq!(tcp_port, 25605);
                assert_eq!(os, "Windows");
                assert_eq!(screen_width, 1920);
                assert_eq!(screen_height, 1080);
                assert_eq!(device_id, "987654321");
            }
            _ => panic!("期望 DiscoveryMessage::Pong"),
        }
    }

    #[test]
    fn test_monitor_info_roundtrip() {
        let info = MonitorInfo {
            index: 1,
            name: "显示器 2".to_string(),
            width: 2560,
            height: 1440,
            is_primary: false,
            left: 1920,
            top: 0,
        };
        let decoded: MonitorInfo = roundtrip(&info);
        assert_eq!(decoded.index, 1);
        assert_eq!(decoded.name, "显示器 2");
        assert_eq!(decoded.width, 2560);
        assert_eq!(decoded.height, 1440);
        assert!(!decoded.is_primary);
        assert_eq!(decoded.left, 1920);
        assert_eq!(decoded.top, 0);
    }

    #[test]
    fn test_monitor_info_default_left_top_zero() {
        // 验证默认构造的 MonitorInfo left/top 为 0
        let info = MonitorInfo {
            index: 0,
            name: "Mon".to_string(),
            width: 1920,
            height: 1080,
            is_primary: true,
            left: 0,
            top: 0,
        };
        let decoded: MonitorInfo = roundtrip(&info);
        assert_eq!(decoded.left, 0);
        assert_eq!(decoded.top, 0);
    }

    #[test]
    fn test_monitor_info_negative_offset() {
        // 多显示器布局中偏移可能为负值
        let info = MonitorInfo {
            index: 0,
            name: "Left".to_string(),
            width: 1920,
            height: 1080,
            is_primary: false,
            left: -1920,
            top: -200,
        };
        let decoded: MonitorInfo = roundtrip(&info);
        assert_eq!(decoded.left, -1920);
        assert_eq!(decoded.top, -200);
    }

    #[test]
    fn test_monitor_list_message_roundtrip() {
        let msg = Message::MonitorList {
            monitors: vec![
                MonitorInfo {
                    index: 0,
                    name: "主显示器".to_string(),
                    width: 1920,
                    height: 1080,
                    is_primary: true,
                    left: 0,
                    top: 0,
                },
                MonitorInfo {
                    index: 1,
                    name: "副显示器".to_string(),
                    width: 2560,
                    height: 1440,
                    is_primary: false,
                    left: 1920,
                    top: -180,
                },
            ],
        };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::MonitorList { monitors } => {
                assert_eq!(monitors.len(), 2);
                assert_eq!(monitors[0].name, "主显示器");
                assert!(monitors[0].is_primary);
                assert_eq!(monitors[0].left, 0);
                assert_eq!(monitors[1].name, "副显示器");
                assert_eq!(monitors[1].left, 1920);
                assert_eq!(monitors[1].top, -180);
            }
            _ => panic!("期望 MonitorList"),
        }
    }

    #[test]
    fn test_switch_monitor_roundtrip() {
        let msg = Message::SwitchMonitor { index: 2 };
        let decoded = roundtrip(&msg);
        match decoded {
            Message::SwitchMonitor { index } => assert_eq!(index, 2),
            _ => panic!("期望 SwitchMonitor"),
        }
    }
}
