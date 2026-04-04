pub mod chat;
mod connection;
mod discovery;
mod file_browser;
mod file_transfer;
mod input;
pub mod remote_control;
pub mod screen_blank;
mod server;
mod shell;
pub mod special_keys;

use serde::Serialize;

// ──────────────── 共享类型 ────────────────

#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    pub addr: String,
    pub hostname: String,
    pub os: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub encoding: String,
    /// Data URL (data:image/jpeg;base64,...) 或 base64 raw
    pub data_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameEvent {
    pub seq: u64,
    pub timestamp_ms: u64,
    pub regions: Vec<FrameRegion>,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_shape: String,
}

/// 授权请求事件（推送到前端弹窗）
#[derive(Debug, Clone, Serialize)]
pub struct AuthRequestEvent {
    pub hostname: String,
    pub addr: String,
    /// 实际授予的角色："Controller" 或 "Viewer"
    pub granted_role: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PinPair {
    pub control_pin: String,
    pub view_pin: String,
}

// ──────────────── 重新导出所有命令 ────────────────

pub use connection::*;
pub use discovery::*;
pub use file_browser::*;
pub use file_transfer::*;
pub use input::*;
pub use server::*;
pub use shell::*;
