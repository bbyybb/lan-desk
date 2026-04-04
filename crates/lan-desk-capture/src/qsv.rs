//! Intel Quick Sync Video (QSV) H.264 编码器
//! 通过 Intel Media SDK / oneVPL 实现。
//!
//! Intel oneVPL SDK 参考:
//!   - 仓库: https://github.com/intel/libvpl
//!   - 文档: https://intel.github.io/libvpl/
//!   - 旧版 Media SDK: https://github.com/Intel-Media-SDK/MediaSDK
//!
//! 支持平台: Windows (DLL) 和 Linux (共享库)。

use crate::frame::CapturedFrame;
use crate::gpu_encoder::VideoEncoder;
use lan_desk_protocol::message::DirtyRegion;
use tracing::{debug, info};

pub struct QsvEncoder {
    _width: u32,
    _height: u32,
    frame_count: u64,
}

impl QsvEncoder {
    pub fn new(_width: u32, _height: u32) -> anyhow::Result<Self> {
        // 尝试加载 Intel Media SDK / oneVPL 运行时
        // Windows: 优先 oneVPL (libvpl.dll)，回退到旧版 Media SDK 的 64/32 位 DLL
        // Linux: 优先 oneVPL (libvpl.so.2)，回退到旧版 (libmfx.so.1)

        #[cfg(target_os = "windows")]
        let lib_names = &[
            "libvpl.dll",
            "libmfxhw64.dll",
            "libmfx64.dll",
            "libmfxhw32.dll",
        ];
        #[cfg(target_os = "linux")]
        let lib_names = &["libvpl.so.2", "libmfx.so.1"];
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let lib_names: &[&str] = &[];

        let mut lib = None;
        for name in lib_names {
            match unsafe { libloading::Library::new(*name) } {
                Ok(l) => {
                    info!("QSV: 已成功加载 {}", name);
                    lib = Some(l);
                    break;
                }
                Err(e) => {
                    debug!("QSV: 尝试加载 {} 失败: {}", name, e);
                }
            }
        }

        let _lib = lib.ok_or_else(|| {
            anyhow::anyhow!(
                "QSV: 未找到 Intel Media SDK / oneVPL 运行时库。\
                 QSV 编码需要带有核显的 Intel CPU 以及最新的 Intel 显卡驱动程序。\
                 请确保：\n\
                 1. 您的 Intel CPU 包含集成显卡（如 Intel UHD / Iris 系列）\n\
                 2. 已安装最新的 Intel 显卡驱动: https://www.intel.com/content/www/us/en/download-center/home.html\n\
                 3. 或安装 Intel oneVPL 运行时: https://github.com/intel/libvpl"
            )
        })?;

        // 完整实现需要：
        // 1. MFXLoad() 创建加载器 (oneVPL) 或 MFXInit() (旧版 Media SDK)
        // 2. MFXCreateSession() 创建会话
        // 3. MFXVideoENCODE_Query() 查询编码能力
        // 4. 配置 mfxVideoParam（编解码器、分辨率、帧率、码率等）
        // 5. MFXVideoENCODE_Init() 初始化编码器
        // 6. 分配帧缓冲区（mfxFrameSurface1）

        // 暂时返回错误以触发回退到下一个编码器
        anyhow::bail!("QSV: 编码器初始化尚未完整实现，回退到下一个编码器")
    }
}

impl VideoEncoder for QsvEncoder {
    fn encode(&mut self, _frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        self.frame_count += 1;
        // 完整实现：
        // 1. 将 BGRA 帧数据转换为 NV12 格式
        // 2. MFXVideoENCODE_EncodeFrameAsync() 提交编码
        // 3. MFXVideoCORE_SyncOperation() 等待完成
        // 4. 从 mfxBitstream 提取 H.264 码流
        Ok(None)
    }

    fn force_keyframe(&mut self) {
        // 设置 mfxEncodeCtrl.FrameType = MFX_FRAMETYPE_I | MFX_FRAMETYPE_IDR | MFX_FRAMETYPE_REF
    }

    fn name(&self) -> &str {
        "QSV"
    }
}

impl Drop for QsvEncoder {
    fn drop(&mut self) {
        // MFXVideoENCODE_Close() 关闭编码器
        // MFXClose() 关闭会话
        // MFXUnload() 卸载加载器 (oneVPL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qsv_encoder_creation_does_not_panic() {
        // 无 Intel GPU 环境下应返回 Err，不应 panic
        let result = QsvEncoder::new(1920, 1080);
        match result {
            Ok(_) => println!("QSV 编码器创建成功（检测到 Intel GPU）"),
            Err(e) => println!("QSV 编码器不可用（预期行为）: {}", e),
        }
    }

    #[test]
    fn test_qsv_encoder_name() {
        // 验证编码器名称返回正确字符串
        let encoder = QsvEncoder {
            _width: 1920,
            _height: 1080,
            frame_count: 0,
        };
        assert_eq!(encoder.name(), "QSV");
    }

    #[test]
    fn test_qsv_encoder_encode_returns_ok_none() {
        // 验证骨架实现的 encode() 返回 Ok(None)
        let mut encoder = QsvEncoder {
            _width: 1920,
            _height: 1080,
            frame_count: 0,
        };
        let dummy_frame = crate::frame::CapturedFrame {
            width: 1920,
            height: 1080,
            stride: 1920 * 4,
            pixel_format: crate::frame::PixelFormat::Bgra8,
            data: vec![0u8; 1920 * 1080 * 4],
            timestamp_ms: 0,
        };
        let result = encoder.encode(&dummy_frame);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        assert_eq!(encoder.frame_count, 1);
    }
}
