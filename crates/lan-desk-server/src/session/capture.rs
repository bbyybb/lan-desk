use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, info, warn};

use lan_desk_capture::encoder::FrameEncoder;
use lan_desk_capture::{create_capturer, create_capturer_for_display, ScreenCapture};
#[allow(unused_imports)]
use lan_desk_protocol::message::DirtyRegion;

use super::Session;

const MIN_FPS: u32 = super::MIN_FPS;
const MAX_FPS: u32 = super::MAX_FPS;

/// 编码后的帧数据（regions 使用 Arc 包装以避免 broadcast channel 的深拷贝开销）
#[derive(Clone)]
pub struct EncodedFrame {
    pub(super) seq: u64,
    pub(super) timestamp_ms: u64,
    pub(super) regions: Arc<Vec<DirtyRegion>>,
}

/// 捕获线程控制指令
#[allow(dead_code)]
pub enum CaptureCommand {
    Stop,
    SwitchMonitor(u32),
    UpdateSettings { jpeg_quality: u8, max_fps: u32 },
}

/// 帧来源
pub(super) enum FrameSource {
    /// 独立捕获线程（旧模式，兼容）
    Dedicated(tokio_mpsc::Receiver<EncodedFrame>),
    /// 共享广播（新模式）
    Broadcast(tokio::sync::broadcast::Receiver<EncodedFrame>),
}

/// 编码一帧：尝试 VideoEncoder (GPU/H264)，回退 JPEG
/// 返回 (编码后的脏区域, video_encoder 是否应被清除)
fn encode_frame(
    frame: &lan_desk_capture::frame::CapturedFrame,
    video_encoder: &mut Option<Box<dyn lan_desk_capture::gpu_encoder::VideoEncoder>>,
    jpeg_encoder: &mut FrameEncoder,
) -> Result<(Vec<DirtyRegion>, bool), ()> {
    let regions = if let Some(ref mut enc) = video_encoder {
        match enc.encode(frame) {
            Ok(Some(region)) => vec![region],
            Ok(None) => vec![],
            Err(e) => {
                warn!("{} 编码失败，回退 JPEG: {}", enc.name(), e);
                return Ok((vec![], true)); // 标记清除，后续仅用 JPEG
            }
        }
    } else {
        match jpeg_encoder.encode(frame) {
            Ok(r) => r,
            Err(e) => {
                warn!("JPEG 编码失败: {}", e);
                return Err(());
            }
        }
    };
    Ok((regions, false))
}

/// 编码一帧并构建 EncodedFrame（共享的编码逻辑）
/// 返回 (编码后的帧, video_encoder 是否应被清除)
pub(super) fn encode_and_build_frame(
    frame: &lan_desk_capture::frame::CapturedFrame,
    video_encoder: &mut Option<Box<dyn lan_desk_capture::gpu_encoder::VideoEncoder>>,
    jpeg_encoder: &mut FrameEncoder,
    seq: u64,
) -> (Option<EncodedFrame>, bool) {
    if frame.data.is_empty() {
        return (None, false);
    }

    let (regions, clear_enc) = match encode_frame(frame, video_encoder, jpeg_encoder) {
        Ok(r) => r,
        Err(()) => return (None, false),
    };

    if clear_enc {
        return (None, true);
    }

    if regions.is_empty() {
        return (None, false);
    }

    (
        Some(EncodedFrame {
            seq,
            timestamp_ms: frame.timestamp_ms,
            regions: Arc::new(regions),
        }),
        false,
    )
}

/// 根据空闲帧数和编码器状态计算自适应帧率
pub(super) fn adaptive_fps(idle_frames: u32, use_h264: bool, jpeg_dirty_ratio: f32) -> u32 {
    if idle_frames > 30 {
        MIN_FPS
    } else if idle_frames > 10 {
        10
    } else if use_h264 {
        if idle_frames > 5 {
            15
        } else {
            MAX_FPS
        }
    } else if jpeg_dirty_ratio > 0.5 {
        MAX_FPS
    } else if jpeg_dirty_ratio > 0.1 {
        20
    } else {
        15
    }
}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static> Session<S> {
    /// 独立线程中的捕获+编码循环（H.264 优先，回退 JPEG）
    pub(super) async fn capture_loop(
        mut capturer: Box<dyn ScreenCapture>,
        frame_tx: tokio_mpsc::Sender<EncodedFrame>,
        mut cmd_rx: tokio_mpsc::Receiver<CaptureCommand>,
    ) {
        use lan_desk_capture::gpu_encoder::create_best_encoder;

        let mut jpeg_encoder = FrameEncoder::new(75);
        let (w, h) = capturer.screen_size();
        let enc = create_best_encoder(w, h);
        let is_null = enc.name().contains("Null");
        let mut video_encoder: Option<Box<dyn lan_desk_capture::gpu_encoder::VideoEncoder>> =
            if is_null { None } else { Some(enc) };

        if let Some(ref enc) = video_encoder {
            info!("捕获线程使用 {} 编码 ({}x{})", enc.name(), w, h);
        } else {
            info!("无可用 H.264 编码器，仅使用 JPEG ({}x{})", w, h);
        }

        let mut idle_frames: u32 = 0;
        let mut current_fps: u32 = MAX_FPS;
        let mut seq: u64 = 0;

        loop {
            let frame_interval = Duration::from_millis(1000 / current_fps as u64);

            tokio::select! {
                _ = tokio::time::sleep(frame_interval) => {}
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(CaptureCommand::Stop) | None => {
                            debug!("捕获线程收到停止信号");
                            break;
                        }
                        Some(CaptureCommand::SwitchMonitor(index)) => {
                            info!("切换到显示器 {}", index);
                            match create_capturer_for_display(index as usize) {
                                Ok(new_capturer) => {
                                    capturer = new_capturer;
                                    let (nw, nh) = capturer.screen_size();
                                    jpeg_encoder = FrameEncoder::new(75);
                                    let new_enc = create_best_encoder(nw, nh);
                                    let enc_name = new_enc.name().to_string();
                                    video_encoder = if enc_name.contains("Null") { None } else { Some(new_enc) };
                                    info!("显示器切换成功: {}x{}, 编码器: {}", nw, nh, enc_name);
                                }
                                Err(e) => warn!("切换显示器失败: {}", e),
                            }
                            continue;
                        }
                        Some(CaptureCommand::UpdateSettings { jpeg_quality, max_fps }) => {
                            jpeg_encoder.set_quality(jpeg_quality);
                            if (MIN_FPS..=60).contains(&max_fps) {
                                // 更新帧率上限由外部自适应逻辑处理
                                info!("捕获设置已更新: 画质={}, 帧率上限={}", jpeg_quality, max_fps);
                            }
                            continue;
                        }
                    }
                }
            }

            match capturer.capture_frame() {
                Ok(frame) => {
                    let current_seq = seq;
                    let (encoded, clear_enc) = encode_and_build_frame(
                        &frame,
                        &mut video_encoder,
                        &mut jpeg_encoder,
                        current_seq,
                    );
                    if clear_enc {
                        video_encoder = None;
                        continue;
                    }

                    if let Some(encoded) = encoded {
                        seq += 1;
                        idle_frames = 0;
                        if frame_tx.send(encoded).await.is_err() {
                            debug!("独立捕获帧发送失败（接收端已关闭）");
                            break;
                        }
                    } else {
                        idle_frames += 1;
                    }
                }
                Err(e) => {
                    warn!("捕获屏幕失败: {}", e);
                }
            }

            let new_fps = adaptive_fps(
                idle_frames,
                video_encoder.is_some(),
                jpeg_encoder.last_dirty_ratio(),
            );
            if new_fps != current_fps {
                debug!("自适应帧率: {} -> {} fps", current_fps, new_fps);
                current_fps = new_fps;
            }
        }
    }

    /// 共享捕获循环（在独立线程中运行，广播帧给所有连接）
    ///
    /// `stop_signal` 为 true 时，循环将干净退出。
    pub async fn shared_capture_loop(
        frame_tx: tokio::sync::broadcast::Sender<EncodedFrame>,
        stop_signal: Arc<AtomicBool>,
    ) {
        Self::shared_capture_loop_with_cmd(frame_tx, stop_signal, None).await;
    }

    /// 支持接收命令的共享捕获循环
    pub async fn shared_capture_loop_with_cmd(
        frame_tx: tokio::sync::broadcast::Sender<EncodedFrame>,
        stop_signal: Arc<AtomicBool>,
        mut cmd_rx: Option<tokio_mpsc::Receiver<CaptureCommand>>,
    ) {
        use lan_desk_capture::gpu_encoder::create_best_encoder;

        let mut capturer = match create_capturer() {
            Ok(c) => c,
            Err(e) => {
                warn!("创建捕获器失败: {}", e);
                return;
            }
        };

        let (w, h) = capturer.screen_size();
        let mut jpeg_encoder = FrameEncoder::new(75);
        let enc = create_best_encoder(w, h);
        let is_null = enc.name().contains("Null");
        let enc_name = enc.name().to_string();
        let mut video_encoder: Option<Box<dyn lan_desk_capture::gpu_encoder::VideoEncoder>> =
            if is_null { None } else { Some(enc) };
        let mut idle_frames: u32 = 0;
        let mut current_fps: u32 = MAX_FPS;
        let mut seq: u64 = 0;

        info!("共享捕获线程启动: {}x{}, 编码器: {}", w, h, enc_name);

        loop {
            if stop_signal.load(Ordering::Acquire) {
                info!("共享捕获线程收到停止信号，退出");
                break;
            }

            let interval = Duration::from_millis(1000 / current_fps as u64);

            // 处理命令（非阻塞）
            if let Some(ref mut rx) = cmd_rx {
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        CaptureCommand::Stop => {
                            info!("共享捕获线程收到停止命令");
                            return;
                        }
                        CaptureCommand::SwitchMonitor(index) => {
                            info!("共享捕获切换到显示器 {}", index);
                            match create_capturer_for_display(index as usize) {
                                Ok(new_capturer) => {
                                    capturer = new_capturer;
                                    let (nw, nh) = capturer.screen_size();
                                    jpeg_encoder = FrameEncoder::new(75);
                                    let new_enc = create_best_encoder(nw, nh);
                                    let name = new_enc.name().to_string();
                                    video_encoder = if name.contains("Null") {
                                        None
                                    } else {
                                        Some(new_enc)
                                    };
                                    info!("显示器切换成功: {}x{}, 编码器: {}", nw, nh, name);
                                }
                                Err(e) => warn!("切换显示器失败: {}", e),
                            }
                        }
                        CaptureCommand::UpdateSettings {
                            jpeg_quality,
                            max_fps,
                        } => {
                            jpeg_encoder.set_quality(jpeg_quality);
                            if (MIN_FPS..=60).contains(&max_fps) {
                                info!("共享捕获设置更新: 画质={}, 帧率={}", jpeg_quality, max_fps);
                            }
                        }
                    }
                }
            }

            tokio::time::sleep(interval).await;

            if stop_signal.load(Ordering::Acquire) {
                break;
            }

            if frame_tx.receiver_count() == 0 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            match capturer.capture_frame() {
                Ok(frame) => {
                    let current_seq = seq;
                    let (encoded, clear_enc) = encode_and_build_frame(
                        &frame,
                        &mut video_encoder,
                        &mut jpeg_encoder,
                        current_seq,
                    );
                    if clear_enc {
                        video_encoder = None;
                        continue;
                    }

                    if let Some(encoded) = encoded {
                        seq += 1;
                        idle_frames = 0;
                        if frame_tx.send(encoded).is_err() {
                            debug!("帧广播发送失败（无接收者）");
                        }
                    } else {
                        idle_frames += 1;
                    }
                }
                Err(e) => warn!("捕获屏幕失败: {}", e),
            }

            let new_fps = adaptive_fps(
                idle_frames,
                video_encoder.is_some(),
                jpeg_encoder.last_dirty_ratio(),
            );
            if new_fps != current_fps {
                current_fps = new_fps;
            }
        }

        info!("共享捕获线程已退出");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lan_desk_capture::encoder::FrameEncoder;

    #[test]
    fn test_encode_and_build_frame_empty_data_returns_none() {
        let frame = lan_desk_capture::frame::CapturedFrame {
            width: 100,
            height: 100,
            stride: 400,
            pixel_format: lan_desk_capture::frame::PixelFormat::Bgra8,
            data: Vec::new(), // 空数据
            timestamp_ms: 12345,
        };
        let mut video_enc: Option<Box<dyn lan_desk_capture::gpu_encoder::VideoEncoder>> = None;
        let mut jpeg_enc = FrameEncoder::new(75);

        let (result, clear_enc) = encode_and_build_frame(&frame, &mut video_enc, &mut jpeg_enc, 1);
        assert!(result.is_none(), "空帧应返回 None");
        assert!(!clear_enc, "空帧不应触发编码器清除");
    }

    #[test]
    fn test_adaptive_fps_high_idle() {
        // 长时间无变化应降至最低帧率
        assert_eq!(adaptive_fps(31, false, 0.0), MIN_FPS);
        assert_eq!(adaptive_fps(50, true, 0.0), MIN_FPS);
    }

    #[test]
    fn test_adaptive_fps_active() {
        // 活跃状态（idle_frames=0）且高脏区域比应使用最大帧率
        assert_eq!(adaptive_fps(0, false, 0.6), MAX_FPS);
        assert_eq!(adaptive_fps(0, true, 0.0), MAX_FPS);
    }

    #[test]
    fn test_adaptive_fps_moderate_idle() {
        // 中等空闲（11-30帧）应降至10fps
        assert_eq!(adaptive_fps(15, false, 0.0), 10);
        assert_eq!(adaptive_fps(15, true, 0.0), 10);
    }
}
