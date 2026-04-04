/// 一帧原始截屏数据
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    /// 每行字节数（含对齐填充）
    pub stride: u32,
    pub pixel_format: PixelFormat,
    /// BGRA/RGBA 原始像素数据
    pub data: Vec<u8>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8,
    Rgba8,
}

impl CapturedFrame {
    /// 获取指定像素的 BGRA 值（含边界检查）
    pub fn pixel_at(&self, x: u32, y: u32) -> &[u8] {
        assert!(
            x < self.width && y < self.height,
            "pixel_at 越界: ({}, {}) 超出 {}x{} 范围",
            x,
            y,
            self.width,
            self.height
        );
        let offset = (y * self.stride + x * 4) as usize;
        &self.data[offset..offset + 4]
    }
}
