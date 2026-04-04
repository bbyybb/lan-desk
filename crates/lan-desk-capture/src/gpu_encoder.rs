/// GPU 硬件编码器抽象层
///
/// 提供 `VideoEncoder` trait 统一不同平台的硬件/软件编码器接口，
/// 以及 `create_best_encoder` 工厂函数按优先级自动选择最优编码器。
use crate::frame::CapturedFrame;
use lan_desk_protocol::message::DirtyRegion;

/// 视频编码器统一接口
pub trait VideoEncoder: Send {
    /// 编码一帧，返回编码后的 DirtyRegion（None 表示跳帧/无变化）
    fn encode(&mut self, frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>>;

    /// 强制下一帧为关键帧
    fn force_keyframe(&mut self);

    /// 编码器名称（用于日志）
    fn name(&self) -> &str;

    /// 设置比特率/质量提示（自适应比特率调整时调用）
    fn set_bitrate_hint(&mut self, _quality: u32) {}
}

/// H264Encoder 的 VideoEncoder 适配器
pub struct H264EncoderAdapter {
    inner: crate::h264::H264Encoder,
}

impl H264EncoderAdapter {
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        Ok(Self {
            inner: crate::h264::H264Encoder::new(width, height)?,
        })
    }
}

impl VideoEncoder for H264EncoderAdapter {
    fn encode(&mut self, frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        self.inner.encode(frame)
    }

    fn force_keyframe(&mut self) {
        self.inner.force_keyframe();
    }

    fn name(&self) -> &str {
        "OpenH264 (CPU)"
    }
}

/// 空编码器（所有编码器都不可用时的回退）
pub struct NullEncoder;

impl VideoEncoder for NullEncoder {
    fn encode(&mut self, _frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        Ok(None)
    }

    fn force_keyframe(&mut self) {}

    fn name(&self) -> &str {
        "Null (disabled)"
    }
}

/// 按优先级探测并创建最优编码器（默认优先 HEVC）
///
/// 回退链：
/// 1. Windows: NVENC HEVC → NVENC H.264 → AMF → QSV → OpenH264 (CPU)
/// 2. macOS: VideoToolbox (GPU) → OpenH264 (CPU)
/// 3. Linux: VAAPI (GPU) → QSV (GPU) → OpenH264 (CPU)
/// 4. 全平台最终回退: NullEncoder
///
/// AV1 编码说明：AV1 编码需要 rav1e crate（编译时间很长且需要 nasm），
/// 或 NVENC AV1（仅 RTX 40 系列以上支持）。当前版本暂不集成 AV1 编码器，
/// 未来计划通过以下方案之一支持：
/// - NVENC AV1（NV_ENC_CODEC_AV1_GUID，需要 Ada Lovelace 或更新架构）
/// - SVT-AV1 动态加载（类似 OpenH264 的方式）
/// - rav1e 可选 feature gate
pub fn create_best_encoder(width: u32, height: u32) -> Box<dyn VideoEncoder> {
    create_best_encoder_with_preference(width, height, true)
}

/// 按优先级探测并创建最优编码器，支持 HEVC 偏好设置
pub fn create_best_encoder_with_preference(
    width: u32,
    height: u32,
    _prefer_hevc: bool,
) -> Box<dyn VideoEncoder> {
    // Windows: 尝试 NVENC HEVC → NVENC H.264 → AMF → QSV
    #[cfg(target_os = "windows")]
    {
        if _prefer_hevc {
            match super::nvenc::NvencHevcEncoder::new(width, height) {
                Ok(enc) => {
                    tracing::info!("使用 NVENC HEVC GPU 编码器 ({}x{})", width, height);
                    return Box::new(enc);
                }
                Err(e) => {
                    tracing::debug!("NVENC HEVC 不可用: {}", e);
                }
            }
        }
        match super::nvenc::NvencEncoder::new(width, height) {
            Ok(enc) => {
                tracing::info!("使用 NVENC GPU 编码器 ({}x{})", width, height);
                return Box::new(enc);
            }
            Err(e) => {
                tracing::debug!("NVENC 不可用: {}", e);
            }
        }
        match super::amf::AmfEncoder::new(width, height) {
            Ok(enc) => {
                tracing::info!("使用 AMF GPU 编码器 ({}x{})", width, height);
                return Box::new(enc);
            }
            Err(e) => {
                tracing::debug!("AMF 不可用: {}", e);
            }
        }
        match super::qsv::QsvEncoder::new(width, height) {
            Ok(enc) => {
                tracing::info!("使用 QSV GPU 编码器 ({}x{})", width, height);
                return Box::new(enc);
            }
            Err(e) => {
                tracing::debug!("QSV 不可用: {}", e);
            }
        }
    }

    // macOS: 尝试 VideoToolbox
    #[cfg(target_os = "macos")]
    {
        match super::videotoolbox::VTEncoder::new(width, height) {
            Ok(enc) => {
                tracing::info!("使用 VideoToolbox GPU 编码器 ({}x{})", width, height);
                return Box::new(enc);
            }
            Err(e) => {
                tracing::debug!("VideoToolbox 不可用: {}", e);
            }
        }
    }

    // Linux: 尝试 VAAPI → QSV
    #[cfg(target_os = "linux")]
    {
        match super::vaapi::VaapiEncoder::new(width, height) {
            Ok(enc) => {
                tracing::info!("使用 VAAPI GPU 编码器 ({}x{})", width, height);
                return Box::new(enc);
            }
            Err(e) => {
                tracing::debug!("VAAPI 不可用: {}", e);
            }
        }
        match super::qsv::QsvEncoder::new(width, height) {
            Ok(enc) => {
                tracing::info!("使用 QSV GPU 编码器 ({}x{})", width, height);
                return Box::new(enc);
            }
            Err(e) => {
                tracing::debug!("QSV 不可用: {}", e);
            }
        }
    }

    // 回退: OpenH264 软编码
    match H264EncoderAdapter::new(width, height) {
        Ok(enc) => {
            tracing::info!("使用 OpenH264 软编码器 ({}x{})", width, height);
            Box::new(enc)
        }
        Err(e) => {
            tracing::warn!("所有编码器都不可用 ({}), 仅使用 JPEG", e);
            Box::new(NullEncoder)
        }
    }
}
