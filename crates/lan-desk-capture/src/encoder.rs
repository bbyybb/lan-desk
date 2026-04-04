use std::io::Cursor;
use std::time::Instant;

use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, ImageEncoder};
use lan_desk_protocol::message::{DirtyRegion, FrameEncoding};
use rayon::prelude::*;
use tracing::debug;

use crate::frame::CapturedFrame;

/// 帧编码器：对比前后帧，输出变化区域的 JPEG 编码
pub struct FrameEncoder {
    prev_frame: Option<Vec<u8>>,
    prev_width: u32,
    prev_height: u32,
    prev_stride: u32,
    jpeg_quality: u8,
    /// 脏检测分块大小
    block_size: u32,
    /// 帧序号
    seq: u64,
    /// 自适应画质：上次编码耗时
    last_encode_ms: u64,
    /// 上次脏区域比例 (0.0~1.0)
    last_dirty_ratio: f32,
    /// 可复用的 RGB 像素缓冲区，避免每次编码重新分配
    rgb_buf: Vec<u8>,
    /// 可复用的 JPEG 编码输出缓冲区
    jpeg_buf: Vec<u8>,
}

impl FrameEncoder {
    pub fn new(jpeg_quality: u8) -> Self {
        Self {
            prev_frame: None,
            prev_width: 0,
            prev_height: 0,
            prev_stride: 0,
            jpeg_quality,
            block_size: 64,
            seq: 0,
            last_encode_ms: 0,
            last_dirty_ratio: 0.0,
            rgb_buf: Vec::new(),
            jpeg_buf: Vec::new(),
        }
    }

    pub fn next_seq(&mut self) -> u64 {
        let s = self.seq;
        self.seq += 1;
        s
    }

    /// 获取上次脏区域比例，用于外部自适应帧率
    pub fn last_dirty_ratio(&self) -> f32 {
        self.last_dirty_ratio
    }

    /// 动态调整 JPEG 画质
    pub fn set_quality(&mut self, quality: u8) {
        self.jpeg_quality = quality.clamp(20, 95);
    }

    /// 获取当前画质
    pub fn quality(&self) -> u8 {
        self.jpeg_quality
    }

    /// 编码一帧，返回变化的脏矩形区域列表
    pub fn encode(&mut self, frame: &CapturedFrame) -> anyhow::Result<Vec<DirtyRegion>> {
        let start = Instant::now();

        let full_frame = self.prev_frame.is_none()
            || self.prev_width != frame.width
            || self.prev_height != frame.height;

        let total_blocks;
        let dirty_blocks;

        let regions = if full_frame {
            total_blocks = 1;
            dirty_blocks = 1;
            vec![self.encode_region_jpeg(frame, 0, 0, frame.width, frame.height)?]
        } else {
            let (regions, tb, db) = self.encode_dirty_regions(frame)?;
            total_blocks = tb;
            dirty_blocks = db;
            regions
        };

        // 更新统计
        self.last_encode_ms = start.elapsed().as_millis() as u64;
        self.last_dirty_ratio = if total_blocks > 0 {
            dirty_blocks as f32 / total_blocks as f32
        } else {
            0.0
        };

        // 自适应画质：脏区域多时降低质量保帧率
        self.adapt_quality();

        // 保存当前帧（双缓冲：大小不变时避免重新分配）
        if let Some(ref mut prev) = self.prev_frame {
            if prev.len() == frame.data.len() {
                prev.copy_from_slice(&frame.data);
            } else {
                *prev = frame.data.clone();
            }
        } else {
            self.prev_frame = Some(frame.data.clone());
        }
        self.prev_width = frame.width;
        self.prev_height = frame.height;
        self.prev_stride = frame.stride;

        Ok(regions)
    }

    /// 根据脏区域比例和编码耗时自动调节画质
    fn adapt_quality(&mut self) {
        if self.last_dirty_ratio > 0.6 {
            // 大面积变化：降低画质保帧率
            let new_q = (self.jpeg_quality as i32 - 5).max(30) as u8;
            if new_q != self.jpeg_quality {
                debug!(
                    "自适应画质: {} -> {} (脏比例 {:.1}%)",
                    self.jpeg_quality,
                    new_q,
                    self.last_dirty_ratio * 100.0
                );
                self.jpeg_quality = new_q;
            }
        } else if self.last_dirty_ratio < 0.1 && self.jpeg_quality < 85 {
            // 小面积变化：提高画质
            let new_q = (self.jpeg_quality + 2).min(85);
            self.jpeg_quality = new_q;
        }
    }

    /// 检测脏块并编码，返回 (regions, total_blocks, dirty_blocks)
    fn encode_dirty_regions(
        &mut self,
        frame: &CapturedFrame,
    ) -> anyhow::Result<(Vec<DirtyRegion>, u32, u32)> {
        let prev = self.prev_frame.as_ref().unwrap();
        let mut dirty_mask = Vec::new();

        let cols = frame.width.div_ceil(self.block_size);
        let rows = frame.height.div_ceil(self.block_size);
        let total_blocks = cols * rows;

        // 第一遍：标记脏块
        for by in 0..rows {
            for bx in 0..cols {
                let x = bx * self.block_size;
                let y = by * self.block_size;
                let w = (frame.width - x).min(self.block_size);
                let h = (frame.height - y).min(self.block_size);
                dirty_mask.push(self.block_changed(prev, &frame.data, frame.stride, x, y, w, h));
            }
        }

        let dirty_count = dirty_mask.iter().filter(|&&d| d).count() as u32;

        // 第二遍：合并相邻脏块为更大的矩形（行级合并），收集坐标
        let mut dirty_rects: Vec<(u32, u32, u32, u32)> = Vec::new();
        for by in 0..rows {
            let mut run_start: Option<u32> = None;
            for bx in 0..cols {
                let idx = (by * cols + bx) as usize;
                if dirty_mask[idx] {
                    if run_start.is_none() {
                        run_start = Some(bx);
                    }
                } else if let Some(start) = run_start.take() {
                    let x = start * self.block_size;
                    let y = by * self.block_size;
                    let w = (bx * self.block_size - x).min(frame.width - x);
                    let h = self.block_size.min(frame.height - y);
                    dirty_rects.push((x, y, w, h));
                }
            }
            if let Some(start) = run_start {
                let x = start * self.block_size;
                let y = by * self.block_size;
                let w = frame.width - x;
                let h = self.block_size.min(frame.height - y);
                dirty_rects.push((x, y, w, h));
            }
        }

        // 并行编码各脏区域（各区域独立，使用独立缓冲区）
        let quality = self.jpeg_quality;
        let regions: Vec<DirtyRegion> = dirty_rects
            .par_iter()
            .filter_map(|&(x, y, w, h)| {
                encode_region_jpeg_parallel(frame, x, y, w, h, quality).ok()
            })
            .collect();

        Ok((regions, total_blocks, dirty_count))
    }

    /// 检查一个块是否有变化（采样对比）
    #[allow(clippy::too_many_arguments)]
    fn block_changed(
        &self,
        prev: &[u8],
        curr: &[u8],
        stride: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> bool {
        let step = if h > 8 { 4 } else { 1 };
        for row in (0..h).step_by(step) {
            let offset = ((y + row) * stride + x * 4) as usize;
            let len = (w * 4) as usize;
            if offset + len > prev.len() || offset + len > curr.len() {
                return true;
            }
            if prev[offset..offset + len] != curr[offset..offset + len] {
                return true;
            }
        }
        false
    }

    /// 将指定区域编码为 JPEG（用于全帧编码，复用内部缓冲区）
    fn encode_region_jpeg(
        &mut self,
        frame: &CapturedFrame,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> anyhow::Result<DirtyRegion> {
        // 提取区域像素 BGRA -> RGB，复用 rgb_buf
        self.rgb_buf.clear();
        let rgb_len = (w * h * 3) as usize;
        self.rgb_buf
            .reserve(rgb_len.saturating_sub(self.rgb_buf.capacity()));
        bgra_region_to_rgb(&frame.data, frame.stride, x, y, w, h, &mut self.rgb_buf);

        // JPEG 编码，编码后直接取走数据，避免不必要的 clone
        self.jpeg_buf.clear();
        let mut cursor = Cursor::new(std::mem::take(&mut self.jpeg_buf));
        let encoder = JpegEncoder::new_with_quality(&mut cursor, self.jpeg_quality);
        encoder.write_image(&self.rgb_buf, w, h, ColorType::Rgb8.into())?;
        let result_data = cursor.into_inner();
        // 为下次编码预分配新缓冲区，容量与本次输出接近
        self.jpeg_buf = Vec::with_capacity(result_data.len());

        Ok(DirtyRegion {
            x,
            y,
            width: w,
            height: h,
            encoding: FrameEncoding::Jpeg {
                quality: self.jpeg_quality,
            },
            data: result_data,
        })
    }
}

/// BGRA 区域提取为 RGB（行级批处理，利用 chunks_exact 优化向量化）
fn bgra_region_to_rgb(
    data: &[u8],
    stride: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    rgb_buf: &mut Vec<u8>,
) {
    for row in 0..h {
        let row_start = ((y + row) * stride + x * 4) as usize;
        let row_end = row_start + (w * 4) as usize;
        if row_end <= data.len() {
            for pixel in data[row_start..row_end].chunks_exact(4) {
                rgb_buf.push(pixel[2]); // R
                rgb_buf.push(pixel[1]); // G
                rgb_buf.push(pixel[0]); // B
            }
        } else {
            rgb_buf.extend(std::iter::repeat_n(0u8, (w * 3) as usize));
        }
    }
}

/// 并行编码用的无状态 JPEG 编码函数（每次独立分配缓冲区）
fn encode_region_jpeg_parallel(
    frame: &CapturedFrame,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    quality: u8,
) -> anyhow::Result<DirtyRegion> {
    let rgb_len = (w * h * 3) as usize;
    let mut rgb_buf = Vec::with_capacity(rgb_len);
    bgra_region_to_rgb(&frame.data, frame.stride, x, y, w, h, &mut rgb_buf);

    let mut jpeg_buf = Vec::with_capacity(rgb_len / 4);
    {
        let mut cursor = Cursor::new(&mut jpeg_buf);
        let encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
        encoder.write_image(&rgb_buf, w, h, ColorType::Rgb8.into())?;
    }

    Ok(DirtyRegion {
        x,
        y,
        width: w,
        height: h,
        encoding: FrameEncoding::Jpeg { quality },
        data: jpeg_buf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::PixelFormat;

    fn make_frame(width: u32, height: u32, color: [u8; 4]) -> CapturedFrame {
        let stride = width * 4;
        let data = color.repeat((width * height) as usize);
        CapturedFrame {
            width,
            height,
            stride,
            pixel_format: PixelFormat::Bgra8,
            data,
            timestamp_ms: 0,
        }
    }

    #[test]
    fn test_first_frame_full_encode() {
        let mut encoder = FrameEncoder::new(75);
        let frame = make_frame(128, 128, [0, 0, 255, 255]); // 蓝色
        let regions = encoder.encode(&frame).unwrap();
        // 第一帧应该生成一个覆盖整屏的区域
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].x, 0);
        assert_eq!(regions[0].y, 0);
        assert_eq!(regions[0].width, 128);
        assert_eq!(regions[0].height, 128);
        assert!(!regions[0].data.is_empty());
    }

    #[test]
    fn test_identical_frame_no_regions() {
        let mut encoder = FrameEncoder::new(75);
        let frame = make_frame(64, 64, [100, 200, 50, 255]);
        encoder.encode(&frame).unwrap();
        // 第二帧相同，不应有脏区域
        let regions = encoder.encode(&frame).unwrap();
        assert!(regions.is_empty(), "相同帧不应产生脏区域");
    }

    #[test]
    fn test_changed_frame_has_regions() {
        let mut encoder = FrameEncoder::new(75);
        let frame1 = make_frame(128, 128, [0, 0, 0, 255]);
        encoder.encode(&frame1).unwrap();

        let frame2 = make_frame(128, 128, [255, 255, 255, 255]);
        let regions = encoder.encode(&frame2).unwrap();
        assert!(!regions.is_empty(), "变化帧应有脏区域");
    }

    #[test]
    fn test_seq_increments() {
        let mut encoder = FrameEncoder::new(75);
        assert_eq!(encoder.next_seq(), 0);
        assert_eq!(encoder.next_seq(), 1);
        assert_eq!(encoder.next_seq(), 2);
    }

    #[test]
    fn test_quality_clamp() {
        let mut encoder = FrameEncoder::new(50);
        encoder.set_quality(10); // 低于最小值
        assert_eq!(encoder.quality(), 20);
        encoder.set_quality(100); // 高于最大值
        assert_eq!(encoder.quality(), 95);
        encoder.set_quality(60);
        assert_eq!(encoder.quality(), 60);
    }

    #[test]
    fn test_resolution_change_resets() {
        let mut encoder = FrameEncoder::new(75);
        let frame1 = make_frame(64, 64, [0, 0, 0, 255]);
        encoder.encode(&frame1).unwrap();

        // 分辨率变化应当作全帧处理
        let frame2 = make_frame(128, 128, [0, 0, 0, 255]);
        let regions = encoder.encode(&frame2).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].width, 128);
    }

    #[test]
    fn test_bgra_region_to_rgb_color_order() {
        // BGRA: B=10, G=20, R=30, A=255
        let data: Vec<u8> = vec![10, 20, 30, 255, 50, 60, 70, 255];
        let mut rgb = Vec::new();
        super::bgra_region_to_rgb(&data, 8, 0, 0, 2, 1, &mut rgb);
        // RGB output should be: R=30,G=20,B=10, R=70,G=60,B=50
        assert_eq!(rgb, vec![30, 20, 10, 70, 60, 50]);
    }

    #[test]
    fn test_encode_region_jpeg_parallel_produces_valid_jpeg() {
        let frame = make_frame(64, 64, [0, 128, 255, 255]);
        let region = super::encode_region_jpeg_parallel(&frame, 0, 0, 64, 64, 75).unwrap();
        assert_eq!(region.x, 0);
        assert_eq!(region.width, 64);
        // JPEG 数据应以 FFD8 (SOI) 开头
        assert!(region.data.len() > 2);
        assert_eq!(region.data[0], 0xFF);
        assert_eq!(region.data[1], 0xD8);
    }
}
