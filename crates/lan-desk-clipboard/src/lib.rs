use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arboard::Clipboard;
use lan_desk_protocol::message::ClipboardContentType;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};
use xxhash_rust::xxh3::xxh3_64;

// ─── 平台原生剪贴板变化检测 ───

/// 获取系统剪贴板序列号（轻量变化检测，避免每次读取完整内容）
///
/// - Windows: 调用 GetClipboardSequenceNumber()，每次剪贴板内容变化时递增
/// - macOS: 调用 NSPasteboard.generalPasteboard.changeCount，每次剪贴板变化时递增
/// - Linux: 返回 0（无轻量变化检测 API，使用内容哈希）
#[cfg(target_os = "windows")]
fn clipboard_change_count() -> u64 {
    // Windows: GetClipboardSequenceNumber() 返回剪贴板序列号
    // 每次剪贴板内容改变时递增，非常轻量（无需打开剪贴板）
    use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
    unsafe { GetClipboardSequenceNumber() as u64 }
}

#[cfg(target_os = "macos")]
fn clipboard_change_count() -> u64 {
    // macOS: 通过 raw Objective-C FFI 调用 [NSPasteboard generalPasteboard].changeCount
    // 使用 objc_msgSend + objc_getClass，无需 objc2 依赖
    use std::os::raw::c_char;

    // 链接 Objective-C 运行时和 AppKit 框架（NSPasteboard 定义在 AppKit 中）
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const c_char) -> *mut std::ffi::c_void;
        fn objc_msgSend() -> isize;
    }
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    unsafe {
        // 获取 NSPasteboard 类
        let class = objc_getClass(b"NSPasteboard\0".as_ptr() as *const c_char);
        if class.is_null() {
            return 0;
        }

        // 调用 [NSPasteboard generalPasteboard]
        let sel_general = sel_registerName(b"generalPasteboard\0".as_ptr() as *const c_char);
        let general_pasteboard: *mut std::ffi::c_void = {
            let f: unsafe extern "C" fn(
                *mut std::ffi::c_void,
                *mut std::ffi::c_void,
            ) -> *mut std::ffi::c_void = std::mem::transmute(objc_msgSend as *const ());
            f(class, sel_general)
        };
        if general_pasteboard.is_null() {
            return 0;
        }

        // 调用 [generalPasteboard changeCount]
        // changeCount 返回 NSInteger（即 isize）
        let sel_change_count = sel_registerName(b"changeCount\0".as_ptr() as *const c_char);
        let count: isize = {
            let f: unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> isize =
                std::mem::transmute(objc_msgSend as *const ());
            f(general_pasteboard, sel_change_count)
        };

        count as u64
    }
}

#[cfg(target_os = "linux")]
fn clipboard_change_count() -> u64 {
    // Linux 无轻量变化检测 API，使用内容哈希
    0
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn clipboard_change_count() -> u64 {
    0
}

/// 剪贴板变更事件
#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub content_type: ClipboardContentType,
    pub data: Vec<u8>,
}

/// 剪贴板管理器：轮询本地剪贴板并与远端同步
pub struct ClipboardManager {
    /// 本地变更 -> 发送给网络层
    local_change_tx: mpsc::Sender<ClipboardChange>,
    /// 来自远端的剪贴板 -> 写入本地
    remote_rx: mpsc::Receiver<ClipboardChange>,
    /// 防回环：上次写入本地剪贴板内容的哈希
    last_set_hash: u64,
    /// 上次读到的本地内容哈希
    last_local_hash: u64,
    /// 剪贴板同步开关（外部可通过 Arc 共享控制）
    pub enabled: Arc<AtomicBool>,
    /// 上次检测到的系统剪贴板序列号（用于轻量变化检测）
    last_change_count: u64,
}

impl ClipboardManager {
    /// 创建剪贴板管理器，返回 (管理器, 本地变更接收端, 远端发送端)
    pub fn new() -> (
        Self,
        mpsc::Receiver<ClipboardChange>,
        mpsc::Sender<ClipboardChange>,
    ) {
        let (local_tx, local_rx) = mpsc::channel(16);
        let (remote_tx, remote_rx) = mpsc::channel(16);

        let mgr = Self {
            local_change_tx: local_tx,
            remote_rx,
            last_set_hash: 0,
            last_local_hash: 0,
            enabled: Arc::new(AtomicBool::new(true)),
            last_change_count: clipboard_change_count(),
        };

        (mgr, local_rx, remote_tx)
    }

    /// 启动剪贴板监听循环（后台 tokio task 中运行）
    pub async fn run(&mut self) {
        let mut poll_interval = tokio::time::interval(Duration::from_millis(300));

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    if self.enabled.load(Ordering::Relaxed) {
                        self.check_local_clipboard();
                    }
                }
                Some(change) = self.remote_rx.recv() => {
                    self.apply_remote_clipboard(change);
                }
            }
        }
    }

    /// 检查本地剪贴板是否有变化
    fn check_local_clipboard(&mut self) {
        // 轻量变化检测：在 Windows 上使用 GetClipboardSequenceNumber()，
        // 只有序列号变化时才读取完整剪贴板内容，大大降低 CPU 消耗。
        // 返回 0 的平台（macOS/Linux）跳过此优化，始终读取内容进行哈希比对。
        let current_count = clipboard_change_count();
        if current_count != 0 {
            if current_count == self.last_change_count {
                trace!("剪贴板序列号未变化 ({}), 跳过读取", current_count);
                return;
            }
            self.last_change_count = current_count;
            debug!("剪贴板序列号变化: {} -> 读取内容", current_count);
        }

        // 每次创建新的 Clipboard 实例，因为 arboard 在某些平台上持有全局资源锁，
        // 长时间持有可能阻塞其他应用的剪贴板访问
        let Ok(mut clipboard) = Clipboard::new() else {
            return;
        };

        // 剪贴板大小限制
        const TEXT_MAX_BYTES: usize = 1_048_576; // 1 MB
        const IMAGE_MAX_BYTES: usize = 10_485_760; // 10 MB

        // 检查文本
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                if text.len() > TEXT_MAX_BYTES {
                    warn!(
                        "本地剪贴板文本超过大小限制 ({} bytes > {} bytes)，跳过同步",
                        text.len(),
                        TEXT_MAX_BYTES
                    );
                    return;
                }
                let hash = xxh3_64(text.as_bytes());
                if hash != self.last_local_hash && hash != self.last_set_hash {
                    self.last_local_hash = hash;
                    let change = ClipboardChange {
                        content_type: ClipboardContentType::PlainText,
                        data: text.into_bytes(),
                    };
                    if self.local_change_tx.try_send(change).is_err() {
                        warn!("剪贴板变更发送队列已满");
                    } else {
                        debug!("检测到本地文本剪贴板变更");
                    }
                    return;
                }
            }
        }

        // 检查图片
        if let Ok(img) = clipboard.get_image() {
            if img.bytes.len() > IMAGE_MAX_BYTES {
                warn!(
                    "本地剪贴板图片超过大小限制 ({} bytes > {} bytes)，跳过同步",
                    img.bytes.len(),
                    IMAGE_MAX_BYTES
                );
                return;
            }
            let hash = xxh3_64(&img.bytes);
            if hash != self.last_local_hash && hash != self.last_set_hash {
                self.last_local_hash = hash;
                // 将 RGBA 图片数据编码为 PNG
                let mut png_data = Vec::new();
                if encode_rgba_to_png(&img.bytes, img.width, img.height, &mut png_data) {
                    let change = ClipboardChange {
                        content_type: ClipboardContentType::Image,
                        data: png_data,
                    };
                    if self.local_change_tx.try_send(change).is_err() {
                        warn!("剪贴板图片发送队列已满");
                    } else {
                        debug!("检测到本地图片剪贴板变更 ({}x{})", img.width, img.height);
                    }
                }
            }
        }
    }

    /// 将远端剪贴板内容写入本地
    fn apply_remote_clipboard(&mut self, change: ClipboardChange) {
        // 写入前先记录当前序列号+1，避免下次轮询误检测为本地变更
        // （写入后序列号会递增，提前更新可防止回环）
        const TEXT_MAX_BYTES: usize = 1_048_576; // 1 MB
        const IMAGE_MAX_BYTES: usize = 10_485_760; // 10 MB

        let Ok(mut clipboard) = Clipboard::new() else {
            warn!("无法打开剪贴板");
            return;
        };

        match change.content_type {
            ClipboardContentType::PlainText => {
                if change.data.len() > TEXT_MAX_BYTES {
                    warn!(
                        "远端剪贴板文本超过大小限制 ({} bytes > {} bytes)，跳过写入",
                        change.data.len(),
                        TEXT_MAX_BYTES
                    );
                    return;
                }
                if let Ok(text) = String::from_utf8(change.data.clone()) {
                    let hash = xxh3_64(text.as_bytes());
                    self.last_set_hash = hash;
                    if clipboard.set_text(&text).is_ok() {
                        debug!("已写入远端剪贴板内容到本地");
                    }
                }
            }
            ClipboardContentType::Image => {
                if change.data.len() > IMAGE_MAX_BYTES {
                    warn!(
                        "远端剪贴板图片超过大小限制 ({} bytes > {} bytes)，跳过写入",
                        change.data.len(),
                        IMAGE_MAX_BYTES
                    );
                    return;
                }
                // 解码 PNG 并设置到剪贴板
                if let Some((rgba, w, h)) = decode_png_to_rgba(&change.data) {
                    let hash = xxh3_64(&rgba);
                    self.last_set_hash = hash;
                    let img = arboard::ImageData {
                        width: w,
                        height: h,
                        bytes: std::borrow::Cow::Owned(rgba),
                    };
                    if clipboard.set_image(img).is_ok() {
                        debug!("已写入远端图片剪贴板 ({}x{})", w, h);
                    }
                } else {
                    debug!("远端图片解码失败");
                }
            }
        }
        // 写入完成后更新序列号，防止下次轮询将远端写入误判为本地变更
        self.last_change_count = clipboard_change_count();
    }
}

/// RGBA -> PNG 编码
fn encode_rgba_to_png(rgba: &[u8], width: usize, height: usize, out: &mut Vec<u8>) -> bool {
    let mut encoder = png::Encoder::new(out, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let Ok(mut writer) = encoder.write_header() else {
        return false;
    };
    writer.write_image_data(rgba).is_ok()
}

/// PNG -> RGBA 解码
fn decode_png_to_rgba(png_data: &[u8]) -> Option<(Vec<u8>, usize, usize)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_data));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some((buf, info.width as usize, info.height as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_png_roundtrip() {
        // 创建一个 2x2 的 RGBA 测试图像
        let rgba = vec![
            255, 0, 0, 255, // 红色
            0, 255, 0, 255, // 绿色
            0, 0, 255, 255, // 蓝色
            255, 255, 0, 255, // 黄色
        ];
        let mut encoded = Vec::new();
        assert!(encode_rgba_to_png(&rgba, 2, 2, &mut encoded));
        assert!(!encoded.is_empty());

        let (decoded, w, h) = decode_png_to_rgba(&encoded).expect("解码失败");
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(decoded, rgba);
    }

    #[test]
    fn test_png_decode_invalid_data() {
        assert!(decode_png_to_rgba(&[]).is_none());
        assert!(decode_png_to_rgba(&[1, 2, 3, 4]).is_none());
    }

    #[test]
    fn test_png_encode_empty_image() {
        let mut out = Vec::new();
        // 0x0 图像应该能编码（或者优雅失败）
        let result = encode_rgba_to_png(&[], 0, 0, &mut out);
        // 不管成功还是失败，不应 panic
        let _ = result;
    }

    #[test]
    fn test_clipboard_change_count_does_not_panic() {
        // 各平台调用不应 panic
        let count = clipboard_change_count();
        // Windows: 返回非负值（可能为 0 如果没有任何剪贴板操作）
        // macOS/Linux: 返回 0
        let _ = count;
    }

    #[test]
    fn test_clipboard_manager_new_default_enabled() {
        let (mgr, _local_rx, _remote_tx) = ClipboardManager::new();
        assert!(mgr.enabled.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn test_clipboard_manager_disable_enable() {
        let (mgr, _local_rx, _remote_tx) = ClipboardManager::new();

        // 禁用
        mgr.enabled
            .store(false, std::sync::atomic::Ordering::Relaxed);
        assert!(!mgr.enabled.load(std::sync::atomic::Ordering::Relaxed));

        // 重新启用
        mgr.enabled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(mgr.enabled.load(std::sync::atomic::Ordering::Relaxed));
    }
}
