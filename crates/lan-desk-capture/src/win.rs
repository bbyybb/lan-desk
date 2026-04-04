use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use tracing::{debug, info, warn};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

use lan_desk_protocol::message::MonitorInfo;

use crate::frame::{CapturedFrame, PixelFormat};
use crate::ScreenCapture;

/// 枚举 Windows 显示器
pub fn list_monitors_win() -> anyhow::Result<Vec<MonitorInfo>> {
    unsafe {
        let mut device = None;
        let feature_levels = [
            windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_1,
            windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0,
            windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_10_1,
            windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_10_0,
        ];
        let result = D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        );
        if result.is_err() || device.is_none() {
            device = None;
            D3D11CreateDevice(
                None,
                windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
        }
        let device = device.context("D3D11 设备为空")?;
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter = dxgi_device.GetParent()?;

        let mut monitors = Vec::new();
        let mut i = 0u32;
        while let Ok(output) = adapter.EnumOutputs(i) {
            let output1: IDXGIOutput1 = output.cast()?;
            if let Ok(dup) = output1.DuplicateOutput(&device) {
                let desc = dup.GetDesc();
                // 通过 GetMonitorInfoW 准确判断主显示器
                let output_desc = output.GetDesc().ok();
                let (is_primary, mon_left, mon_top) = output_desc
                    .map(|od| {
                        let mut mi = MONITORINFO {
                            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                            ..Default::default()
                        };
                        if GetMonitorInfoW(od.Monitor, &mut mi).as_bool() {
                            let primary = (mi.dwFlags & MONITORINFOF_PRIMARY) != 0;
                            // 使用 MONITORINFO.rcMonitor 获取精确坐标
                            (primary, mi.rcMonitor.left, mi.rcMonitor.top)
                        } else {
                            (
                                i == 0,
                                od.DesktopCoordinates.left,
                                od.DesktopCoordinates.top,
                            )
                        }
                    })
                    .unwrap_or((i == 0, 0, 0));
                monitors.push(MonitorInfo {
                    index: i,
                    name: format!("显示器 {}", i + 1),
                    width: desc.ModeDesc.Width,
                    height: desc.ModeDesc.Height,
                    is_primary,
                    left: mon_left,
                    top: mon_top,
                });
            }
            // 某些输出可能无法复制（如离线显示器），跳过即可
            i += 1;
        }
        Ok(monitors)
    }
}

/// Windows DXGI Desktop Duplication 屏幕捕获器（带异常自动恢复）
pub struct DxgiCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    adapter: IDXGIAdapter,
    display_index: u32,
    duplication: Option<IDXGIOutputDuplication>,
    staging_texture: Option<ID3D11Texture2D>,
    width: u32,
    height: u32,
    /// 连续恢复失败次数
    recovery_failures: u32,
}

impl DxgiCapture {
    pub fn new() -> anyhow::Result<Self> {
        Self::new_for_display(0)
    }

    pub fn new_for_display(display_index: u32) -> anyhow::Result<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;

            // 指定支持的 feature level 列表，从高到低尝试
            let feature_levels = [
                windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_1,
                windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0,
                windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_10_1,
                windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_10_0,
            ];

            // 先尝试硬件驱动，失败则回退到 WARP（软件渲染）
            let result = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            );

            if result.is_err() || device.is_none() {
                tracing::warn!("D3D11 硬件设备创建失败，尝试 WARP 软件渲染回退");
                device = None;
                context = None;
                D3D11CreateDevice(
                    None,
                    windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP,
                    None,
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&feature_levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )
                .context("创建 D3D11 设备失败（硬件和 WARP 均失败）")?;
            }

            let device = device.context("D3D11 设备为空")?;
            let context = context.context("D3D11 上下文为空")?;

            let dxgi_device: IDXGIDevice = device.cast()?;
            let adapter: IDXGIAdapter = dxgi_device.GetParent()?;

            let mut capturer = Self {
                device,
                context,
                adapter,
                display_index,
                duplication: None,
                staging_texture: None,
                width: 0,
                height: 0,
                recovery_failures: 0,
            };

            capturer.init_duplication()?;
            Ok(capturer)
        }
    }

    /// 初始化或重建 Desktop Duplication
    fn init_duplication(&mut self) -> anyhow::Result<()> {
        unsafe {
            let output: IDXGIOutput = self.adapter.EnumOutputs(self.display_index)?;
            let output1: IDXGIOutput1 = output.cast()?;
            let duplication = output1.DuplicateOutput(&self.device)?;

            let dup_desc = duplication.GetDesc();
            self.width = dup_desc.ModeDesc.Width;
            self.height = dup_desc.ModeDesc.Height;
            debug!(
                "DXGI 初始化: 显示器 {}, {}x{}",
                self.display_index, self.width, self.height
            );

            // 创建 staging texture
            let tex_desc = D3D11_TEXTURE2D_DESC {
                Width: self.width,
                Height: self.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };

            let mut staging = None;
            self.device
                .CreateTexture2D(&tex_desc, None, Some(&mut staging))?;

            self.duplication = Some(duplication);
            self.staging_texture = staging;
            self.recovery_failures = 0;

            Ok(())
        }
    }

    /// 尝试从 DXGI 错误中恢复
    fn try_recover(&mut self) -> bool {
        // 释放旧资源
        self.duplication = None;
        self.staging_texture = None;

        // 等待一小段时间让桌面稳定
        std::thread::sleep(std::time::Duration::from_millis(500));

        match self.init_duplication() {
            Ok(()) => {
                info!("DXGI Desktop Duplication 恢复成功");
                true
            }
            Err(e) => {
                self.recovery_failures += 1;
                if self.recovery_failures <= 3 {
                    warn!("DXGI 恢复失败 (第 {} 次): {}", self.recovery_failures, e);
                }
                false
            }
        }
    }
}

impl ScreenCapture for DxgiCapture {
    fn capture_frame(&mut self) -> anyhow::Result<CapturedFrame> {
        let duplication = match &self.duplication {
            Some(d) => d,
            None => {
                // 正在恢复中
                if !self.try_recover() {
                    return Ok(CapturedFrame {
                        width: self.width.max(1),
                        height: self.height.max(1),
                        stride: self.width.max(1) * 4,
                        pixel_format: PixelFormat::Bgra8,
                        data: Vec::new(),
                        timestamp_ms: now_ms(),
                    });
                }
                self.duplication.as_ref().unwrap()
            }
        };

        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;

            match duplication.AcquireNextFrame(100, &mut frame_info, &mut resource) {
                Ok(()) => {}
                Err(e) => {
                    let code = e.code();
                    if code == DXGI_ERROR_WAIT_TIMEOUT {
                        return Ok(CapturedFrame {
                            width: self.width,
                            height: self.height,
                            stride: self.width * 4,
                            pixel_format: PixelFormat::Bgra8,
                            data: Vec::new(),
                            timestamp_ms: now_ms(),
                        });
                    }

                    // ACCESS_LOST: 桌面切换、锁屏、UAC 等
                    if code == DXGI_ERROR_ACCESS_LOST {
                        warn!("DXGI ACCESS_LOST（桌面切换/锁屏），尝试恢复...");
                        self.try_recover();
                        return Ok(CapturedFrame {
                            width: self.width.max(1),
                            height: self.height.max(1),
                            stride: self.width.max(1) * 4,
                            pixel_format: PixelFormat::Bgra8,
                            data: Vec::new(),
                            timestamp_ms: now_ms(),
                        });
                    }

                    // 其他 DXGI 错误也尝试恢复
                    warn!("DXGI 捕获错误: {:?}，尝试恢复...", code);
                    self.try_recover();
                    return Ok(CapturedFrame {
                        width: self.width.max(1),
                        height: self.height.max(1),
                        stride: self.width.max(1) * 4,
                        pixel_format: PixelFormat::Bgra8,
                        data: Vec::new(),
                        timestamp_ms: now_ms(),
                    });
                }
            }

            let resource = resource.context("获取帧资源失败")?;
            let texture: ID3D11Texture2D = resource.cast()?;
            let staging = self
                .staging_texture
                .as_ref()
                .context("staging texture 为空")?;

            self.context.CopyResource(staging, &texture);
            self.duplication.as_ref().unwrap().ReleaseFrame()?;

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

            let stride = mapped.RowPitch;
            let data_size = (stride * self.height) as usize;
            let data = std::slice::from_raw_parts(mapped.pData as *const u8, data_size).to_vec();

            self.context.Unmap(staging, 0);

            Ok(CapturedFrame {
                width: self.width,
                height: self.height,
                stride,
                pixel_format: PixelFormat::Bgra8,
                data,
                timestamp_ms: now_ms(),
            })
        }
    }

    fn screen_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
