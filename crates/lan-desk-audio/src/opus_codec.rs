//! Opus 音频编解码器
//!
//! 启用 `opus` feature 时提供完整的 Opus 编码/解码能力（需要 cmake 构建 libopus）。
//! 未启用时提供 stub 实现，总是返回错误以触发 PCM 降级。

#[cfg(feature = "opus")]
mod inner {
    use audiopus::coder::{Decoder, Encoder};
    use audiopus::{Application, Channels, SampleRate};
    use tracing::{debug, warn};

    fn to_sample_rate(sr: u32) -> anyhow::Result<SampleRate> {
        match sr {
            8000 => Ok(SampleRate::Hz8000),
            12000 => Ok(SampleRate::Hz12000),
            16000 => Ok(SampleRate::Hz16000),
            24000 => Ok(SampleRate::Hz24000),
            48000 => Ok(SampleRate::Hz48000),
            _ => anyhow::bail!("Opus 不支持采样率 {}Hz（需 8k/12k/16k/24k/48k）", sr),
        }
    }

    fn to_channels(ch: u16) -> anyhow::Result<Channels> {
        match ch {
            1 => Ok(Channels::Mono),
            2 => Ok(Channels::Stereo),
            _ => anyhow::bail!("Opus 不支持 {} 声道（需 1 或 2）", ch),
        }
    }

    /// Opus 编码器：累积 PCM 样本并编码为 Opus 帧
    pub struct OpusEncoder {
        encoder: Encoder,
        frame_size: usize,
        channels: usize,
        buffer: Vec<i16>,
        output_buf: Vec<u8>,
    }

    impl OpusEncoder {
        pub fn new(sample_rate: u32, channels: u16, bitrate: u32) -> anyhow::Result<Self> {
            let mut encoder = Encoder::new(
                to_sample_rate(sample_rate)?,
                to_channels(channels)?,
                Application::LowDelay,
            )
            .map_err(|e| anyhow::anyhow!("创建 Opus 编码器失败: {}", e))?;

            encoder
                .set_bitrate(audiopus::Bitrate::BitsPerSecond(bitrate as i32))
                .map_err(|e| anyhow::anyhow!("设置 Opus 比特率失败: {}", e))?;

            let frame_size = (sample_rate * 20 / 1000) as usize;
            debug!(
                "Opus 编码器已创建: {}Hz {}ch {}kbps, 帧长 {}samples",
                sample_rate,
                channels,
                bitrate / 1000,
                frame_size
            );

            Ok(Self {
                encoder,
                frame_size,
                channels: channels as usize,
                buffer: Vec::with_capacity(frame_size * channels as usize * 2),
                output_buf: vec![0u8; 4000],
            })
        }

        /// 将 PCM 16-bit LE 字节编码为 Opus 帧
        pub fn encode_pcm(&mut self, pcm_le_bytes: &[u8]) -> Vec<Vec<u8>> {
            let mut result = Vec::new();
            // 安全地将 LE 字节转为 i16，避免 from_raw_parts 的对齐问题
            let sample_count = pcm_le_bytes.len() / 2;
            let mut samples = Vec::with_capacity(sample_count);
            for i in 0..sample_count {
                let lo = pcm_le_bytes[i * 2] as i16;
                let hi = pcm_le_bytes[i * 2 + 1] as i16;
                samples.push(lo | (hi << 8));
            }
            self.buffer.extend_from_slice(&samples);
            let samples_per_frame = self.frame_size * self.channels;
            while self.buffer.len() >= samples_per_frame {
                let frame: Vec<i16> = self.buffer.drain(..samples_per_frame).collect();
                match self.encoder.encode(&frame, &mut self.output_buf) {
                    Ok(len) => result.push(self.output_buf[..len].to_vec()),
                    Err(e) => warn!("Opus 编码失败: {}", e),
                }
            }
            result
        }
    }

    /// Opus 解码器：将 Opus 帧解码为 PCM 16-bit LE 字节
    pub struct OpusDecoder {
        decoder: Decoder,
        channels: usize,
        output_buf: Vec<i16>,
    }

    impl OpusDecoder {
        pub fn new(sample_rate: u32, channels: u16) -> anyhow::Result<Self> {
            let decoder = Decoder::new(to_sample_rate(sample_rate)?, to_channels(channels)?)
                .map_err(|e| anyhow::anyhow!("创建 Opus 解码器失败: {}", e))?;
            let frame_size = (sample_rate * 20 / 1000) as usize;
            debug!("Opus 解码器已创建: {}Hz {}ch", sample_rate, channels);
            Ok(Self {
                decoder,
                channels: channels as usize,
                output_buf: vec![0i16; frame_size * channels as usize],
            })
        }

        pub fn decode_opus(&mut self, opus_packet: &[u8]) -> anyhow::Result<Vec<u8>> {
            let decoded = self
                .decoder
                .decode(Some(opus_packet), &mut self.output_buf, false)
                .map_err(|e| anyhow::anyhow!("Opus 解码失败: {}", e))?;
            let total = decoded * self.channels;
            let mut pcm = Vec::with_capacity(total * 2);
            for &s in &self.output_buf[..total] {
                pcm.extend_from_slice(&s.to_le_bytes());
            }
            Ok(pcm)
        }
    }
}

// ─── 未启用 opus feature 时的 stub 实现 ───

#[cfg(not(feature = "opus"))]
mod inner {
    /// Opus 编码器 stub（opus feature 未启用）
    pub struct OpusEncoder;

    impl OpusEncoder {
        pub fn new(_sample_rate: u32, _channels: u16, _bitrate: u32) -> anyhow::Result<Self> {
            anyhow::bail!("Opus 编码不可用（编译时未启用 opus feature，需安装 cmake）")
        }

        pub fn encode_pcm(&mut self, _pcm_le_bytes: &[u8]) -> Vec<Vec<u8>> {
            Vec::new()
        }
    }

    /// Opus 解码器 stub（opus feature 未启用）
    pub struct OpusDecoder;

    impl OpusDecoder {
        pub fn new(_sample_rate: u32, _channels: u16) -> anyhow::Result<Self> {
            anyhow::bail!("Opus 解码不可用（编译时未启用 opus feature，需安装 cmake）")
        }

        pub fn decode_opus(&mut self, _opus_packet: &[u8]) -> anyhow::Result<Vec<u8>> {
            anyhow::bail!("Opus 解码不可用")
        }
    }
}

pub use inner::{OpusDecoder, OpusEncoder};
