use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use tracing::{debug, info};

use lan_desk_protocol::message::{DirtyRegion, FrameEncoding};

use crate::frame::CapturedFrame;

/// H.264 编码器封装
pub struct H264Encoder {
    encoder: Encoder,
    width: usize,
    height: usize,
    frame_count: u64,
    keyframe_interval: u64,
}

impl H264Encoder {
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        // 确保宽高为偶数
        let w = (width as usize) & !1;
        let h = (height as usize) & !1;

        let api = OpenH264API::from_source();
        let config = EncoderConfig::new();
        let encoder = Encoder::with_api_config(api, config)?;

        info!("H.264 编码器初始化: {}x{}", w, h);

        Ok(Self {
            encoder,
            width: w,
            height: h,
            frame_count: 0,
            keyframe_interval: 60,
        })
    }

    /// 编码一帧 BGRA 为 H.264
    pub fn encode(&mut self, frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        if frame.data.is_empty() {
            return Ok(None);
        }

        let is_keyframe = self.frame_count.is_multiple_of(self.keyframe_interval);
        if is_keyframe {
            self.encoder.force_intra_frame();
        }

        // BGRA -> YUV420（使用共享色彩转换模块）
        let yuv_data = crate::color_convert::bgra_to_i420_alloc(
            &frame.data,
            frame.stride as usize,
            self.width,
            self.height,
        );
        let yuv = YUVBuffer::from_vec(yuv_data, self.width, self.height);

        // 编码
        let bitstream = self.encoder.encode(&yuv)?;
        let nal_data = bitstream.to_vec();

        self.frame_count += 1;

        if nal_data.is_empty() {
            return Ok(None);
        }

        debug!(
            "H.264 帧 #{}: {} bytes ({})",
            self.frame_count - 1,
            nal_data.len(),
            if is_keyframe { "I帧" } else { "P帧" }
        );

        Ok(Some(DirtyRegion {
            x: 0,
            y: 0,
            width: self.width as u32,
            height: self.height as u32,
            encoding: FrameEncoding::H264 { is_keyframe },
            data: nal_data,
        }))
    }

    pub fn force_keyframe(&mut self) {
        self.frame_count = 0;
    }
}
