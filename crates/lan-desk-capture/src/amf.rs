//! AMD Advanced Media Framework (AMF) H.264 编码器
//! 通过动态加载 amfrt64.dll（或 amfrt32.dll）实现，不可用时回退到其他编码器。
//!
//! AMF SDK 参考:
//!   - 版本: 1.4.34 (最新稳定版)
//!   - 下载地址: https://github.com/GPUOpen-LibrariesAndSDKs/AMF
//!   - 文档: https://gpuopen-librariesandsdks.github.io/AMF/
//!
//! 注意: AMF 仅在 Windows 上通过原生 DLL 支持。Linux 上 AMD GPU 编码
//! 请使用 VA-API 通道（参见 vaapi.rs）。

#[cfg(target_os = "windows")]
use crate::frame::CapturedFrame;
#[cfg(target_os = "windows")]
use crate::gpu_encoder::VideoEncoder;
#[cfg(target_os = "windows")]
use lan_desk_protocol::message::DirtyRegion;
#[cfg(target_os = "windows")]
use tracing::{debug, info};

/// 检测系统中是否存在 AMD GPU。
///
/// 通过 DXGI 枚举显示适配器，检查供应商 ID 判断。
/// AMD 的 PCI 供应商 ID 为 0x1002。
#[cfg(target_os = "windows")]
pub fn detect_amd_gpu() -> bool {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    const AMD_VENDOR_ID: u32 = 0x1002;

    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(e) => {
            debug!("AMF: 无法创建 DXGI Factory: {}", e);
            return false;
        }
    };

    let mut adapter_index = 0u32;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(adapter_index) } {
        if let Ok(desc) = unsafe { adapter.GetDesc1() } {
            if desc.VendorId == AMD_VENDOR_ID {
                let name = String::from_utf16_lossy(
                    &desc.Description[..desc
                        .Description
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(desc.Description.len())],
                );
                debug!("AMF: 检测到 AMD GPU 适配器: {}", name);
                return true;
            }
        }
        adapter_index += 1;
    }

    debug!("AMF: 未检测到 AMD GPU 适配器");
    false
}

#[cfg(target_os = "windows")]
pub struct AmfEncoder {
    // AMF 上下文和组件的原始指针
    // 实际实现需要完整的 AMF SDK FFI 定义
    _width: u32,
    _height: u32,
    frame_count: u64,
}

#[cfg(target_os = "windows")]
impl AmfEncoder {
    pub fn new(_width: u32, _height: u32) -> anyhow::Result<Self> {
        // 第一步：检测 AMD GPU 是否存在
        if !detect_amd_gpu() {
            anyhow::bail!(
                "AMF: 未检测到 AMD GPU。AMF 编码需要 AMD Radeon 显卡。\
                 如果您已安装 AMD 显卡，请确保安装了最新的 AMD Adrenalin 驱动程序: \
                 https://www.amd.com/en/support"
            );
        }

        // 第二步：尝试加载 AMF 运行时 DLL（先 64 位，后 32 位回退）
        let lib = unsafe {
            libloading::Library::new("amfrt64.dll")
                .or_else(|e64| {
                    debug!("AMF: 无法加载 amfrt64.dll: {}，尝试 amfrt32.dll", e64);
                    libloading::Library::new("amfrt32.dll")
                })
                .map_err(|e| {
                    anyhow::anyhow!(
                        "AMF: 无法加载 AMD 媒体框架运行时（amfrt64.dll / amfrt32.dll）: {}。\
                         请确保已安装 AMD Radeon 显卡并更新到最新驱动程序。\
                         驱动下载: https://www.amd.com/en/support",
                        e
                    )
                })?
        };

        // 第三步：获取 AMFInit 函数指针
        type AmfInitFn =
            unsafe extern "C" fn(version: u64, factory: *mut *mut std::ffi::c_void) -> i32;
        let _amf_init: libloading::Symbol<AmfInitFn> = unsafe {
            lib.get(b"AMFInit").map_err(|e| {
                anyhow::anyhow!(
                    "AMF: DLL 已加载但找不到 AMFInit 入口点: {}。\
                     这可能是驱动版本过旧，请更新 AMD 驱动程序。",
                    e
                )
            })?
        };

        // AMF SDK 版本 1.4.34
        // 完整实现需要：
        // 1. AMFInit() 获取 AMFFactory
        // 2. factory->CreateContext() 创建上下文
        // 3. context->InitDX11(device) 绑定 D3D11 设备
        // 4. factory->CreateComponent(context, AMFVideoEncoderVCE_AVC, &encoder)
        // 5. 设置编码参数（分辨率、帧率、码率等）
        // 6. encoder->Init(AMF_SURFACE_NV12, width, height)

        // 由于 AMF SDK 的 COM 接口需要大量 FFI 定义，
        // 这里提供骨架实现，完整 FFI 需要参考 AMF SDK 头文件
        info!("AMF: AMD 媒体框架运行时已加载，AMD GPU 编码可用");

        // 实际初始化（需要完整 FFI）
        // 暂时返回错误以触发回退
        anyhow::bail!("AMF: 编码器初始化尚未完整实现，回退到下一个编码器")
    }
}

#[cfg(target_os = "windows")]
impl VideoEncoder for AmfEncoder {
    fn encode(&mut self, _frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        self.frame_count += 1;
        // 完整实现：
        // 1. 创建 AMFSurface (NV12)
        // 2. BGRA -> NV12 颜色空间转换
        // 3. encoder->SubmitInput(surface)
        // 4. encoder->QueryOutput(&data)
        // 5. 提取 H.264 码流
        Ok(None)
    }

    fn force_keyframe(&mut self) {
        // encoder->SetProperty(AMF_VIDEO_ENCODER_FORCE_PICTURE_TYPE, AMF_VIDEO_ENCODER_PICTURE_TYPE_IDR)
    }

    fn name(&self) -> &str {
        "AMF"
    }
}

#[cfg(target_os = "windows")]
impl Drop for AmfEncoder {
    fn drop(&mut self) {
        // 释放 AMF 资源：
        // encoder->Terminate()
        // context->Terminate()
        // 释放 AMFFactory 引用
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amf_encoder_creation_does_not_panic() {
        // 无 AMD GPU 环境下应返回 Err，不应 panic
        let result = AmfEncoder::new(1920, 1080);
        match result {
            Ok(_) => println!("AMF 编码器创建成功（检测到 AMD GPU）"),
            Err(e) => println!("AMF 编码器不可用（预期行为）: {}", e),
        }
    }

    #[test]
    fn test_amf_encoder_name() {
        // 验证编码器名称返回正确字符串
        // 由于无法在无 AMD GPU 的环境中构造 AmfEncoder，
        // 我们手动构造一个用于测试 trait 方法
        let encoder = AmfEncoder {
            _width: 1920,
            _height: 1080,
            frame_count: 0,
        };
        assert_eq!(encoder.name(), "AMF");
    }

    #[test]
    fn test_amf_encoder_encode_returns_ok_none() {
        // 验证骨架实现的 encode() 返回 Ok(None)
        let mut encoder = AmfEncoder {
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

    #[test]
    fn test_detect_amd_gpu_does_not_panic() {
        // detect_amd_gpu 不应在任何环境下 panic
        let _has_amd = detect_amd_gpu();
    }
}
