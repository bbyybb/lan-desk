//! NVIDIA NVENC H.264 GPU 硬件编码器
//!
//! 通过 libloading 动态加载 nvEncodeAPI64.dll，
//! 使用 NVIDIA Video Codec SDK 的 NVENC API 进行 H.264 编码。
//! 无需编译时链接 NVIDIA SDK，运行时检测 GPU 可用性。

use std::ffi::c_void;
use std::ptr;

use anyhow::Context;
use tracing::{debug, info};
use windows::core::Interface;

use crate::frame::CapturedFrame;
use crate::gpu_encoder::VideoEncoder;
use lan_desk_protocol::message::{DirtyRegion, FrameEncoding};

// ─── NVENC API 常量 ───

/// NVENC API 版本号：主版本 12，次版本 2（对应 Video Codec SDK 12.2）
const NVENCAPI_VERSION: u32 = (12 << 4) | 2;

/// NV_ENCODE_API_FUNCTION_LIST 结构体版本
const NV_ENCODE_API_FUNCTION_LIST_VER: u32 =
    (2 << 16) | std::mem::size_of::<NvEncApiFunctionList>() as u32;

/// NVENC 状态码：成功
const NV_ENC_SUCCESS: u32 = 0;

/// H.264 编码器 GUID
const NV_ENC_CODEC_H264_GUID: NvEncGuid = NvEncGuid {
    data1: 0x6BC8_2762,
    data2: 0x4E63,
    data3: 0x4CA4,
    data4: [0xAA, 0x85, 0x1A, 0x4D, 0x06, 0x4D, 0xE2, 0xCC],
};

/// HEVC (H.265) 编码器 GUID
const NV_ENC_CODEC_HEVC_GUID: NvEncGuid = NvEncGuid {
    data1: 0x790C_DC88,
    data2: 0x4522,
    data3: 0x4D7B,
    data4: [0x94, 0x25, 0xBD, 0xA9, 0x97, 0x5F, 0x76, 0x03],
};

/// HEVC Main Profile GUID
const NV_ENC_HEVC_PROFILE_MAIN_GUID: NvEncGuid = NvEncGuid {
    data1: 0xB514_C39A,
    data2: 0xB55B,
    data3: 0x40FA,
    data4: [0x87, 0x8F, 0xF1, 0x25, 0x3B, 0x4D, 0xFD, 0xEC],
};

/// 低延迟高质量预设 GUID (P4)
const NV_ENC_PRESET_P4_GUID: NvEncGuid = NvEncGuid {
    data1: 0xFC0A_8D3E,
    data2: 0x45F8,
    data3: 0x4CF8,
    data4: [0x80, 0xC7, 0x29, 0x88, 0x71, 0x59, 0x0E, 0xBF],
};

/// 低延迟调优信息 GUID
const NV_ENC_TUNING_INFO_LOW_LATENCY: u32 = 2;

/// NV12 输入格式
const NV_ENC_BUFFER_FORMAT_NV12: u32 = 1;

/// 编码图片类型标志
const NV_ENC_PIC_TYPE_IDR: u32 = 4;

/// 编码图片结构
const NV_ENC_PIC_STRUCT_FRAME: u32 = 1;

/// H.264 Baseline Profile GUID
const NV_ENC_H264_PROFILE_BASELINE_GUID: NvEncGuid = NvEncGuid {
    data1: 0x0727_BCAA,
    data2: 0x78C4,
    data3: 0x4C83,
    data4: [0x8C, 0x2F, 0xEF, 0x3D, 0xFF, 0x26, 0x7C, 0x6A],
};

/// D3D11 设备类型
const NV_ENC_DEVICE_TYPE_DIRECTX: u32 = 1;

// ─── NVENC FFI 类型定义 ───

/// NVENC GUID 结构体（128 位标识符）
#[repr(C)]
#[derive(Clone, Copy)]
struct NvEncGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

/// NVENC 函数指针表
///
/// 通过 NvEncodeAPICreateInstance 填充。
/// 字段顺序必须严格匹配 NVIDIA SDK 中的 NV_ENCODE_API_FUNCTION_LIST。
#[repr(C)]
#[allow(non_snake_case)]
struct NvEncApiFunctionList {
    version: u32,
    reserved: u32,
    nvEncOpenEncodeSessionEx:
        Option<unsafe extern "C" fn(*mut NvEncOpenEncodeSessionExParams) -> u32>,
    nvEncGetEncodeGUIDs:
        Option<unsafe extern "C" fn(*mut c_void, *mut NvEncGuid, u32, *mut u32) -> u32>,
    nvEncGetEncodeProfileGUIDCount:
        Option<unsafe extern "C" fn(*mut c_void, NvEncGuid, *mut u32) -> u32>,
    nvEncGetEncodeProfileGUIDs:
        Option<unsafe extern "C" fn(*mut c_void, NvEncGuid, *mut NvEncGuid, u32, *mut u32) -> u32>,
    nvEncGetInputFormatCount: Option<unsafe extern "C" fn(*mut c_void, NvEncGuid, *mut u32) -> u32>,
    nvEncGetInputFormats:
        Option<unsafe extern "C" fn(*mut c_void, NvEncGuid, *mut u32, u32, *mut u32) -> u32>,
    nvEncGetEncodeCaps:
        Option<unsafe extern "C" fn(*mut c_void, NvEncGuid, *mut c_void, *mut i32) -> u32>,
    nvEncGetEncodePresetCount:
        Option<unsafe extern "C" fn(*mut c_void, NvEncGuid, *mut u32) -> u32>,
    nvEncGetEncodePresetGUIDs:
        Option<unsafe extern "C" fn(*mut c_void, NvEncGuid, *mut NvEncGuid, u32, *mut u32) -> u32>,
    nvEncGetEncodePresetConfigEx: Option<
        unsafe extern "C" fn(*mut c_void, NvEncGuid, NvEncGuid, u32, *mut NvEncPresetConfig) -> u32,
    >,
    nvEncInitializeEncoder:
        Option<unsafe extern "C" fn(*mut c_void, *mut NvEncInitializeParams) -> u32>,
    nvEncCreateInputBuffer:
        Option<unsafe extern "C" fn(*mut c_void, *mut NvEncCreateInputBufferParams) -> u32>,
    nvEncDestroyInputBuffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncCreateBitstreamBuffer:
        Option<unsafe extern "C" fn(*mut c_void, *mut NvEncCreateBitstreamBufferParams) -> u32>,
    nvEncDestroyBitstreamBuffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncEncodePicture: Option<unsafe extern "C" fn(*mut c_void, *mut NvEncPicParams) -> u32>,
    nvEncLockBitstream:
        Option<unsafe extern "C" fn(*mut c_void, *mut NvEncLockBitstreamParams) -> u32>,
    nvEncUnlockBitstream: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncLockInputBuffer:
        Option<unsafe extern "C" fn(*mut c_void, *mut NvEncLockInputBufferParams) -> u32>,
    nvEncUnlockInputBuffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncGetEncodeStats: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncGetSequenceParams: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncRegisterAsyncEvent: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncUnregisterAsyncEvent: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncMapInputResource: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncUnmapInputResource: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncDestroyEncoder: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
    nvEncInvalidateRefFrames: Option<unsafe extern "C" fn(*mut c_void, u64) -> u32>,
    nvEncOpenEncodeSessionEx2: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncRegisterResource: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncUnregisterResource: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncReconfigureEncoder: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    _reserved1: *mut c_void,
    nvEncCreateMVBuffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncDestroyMVBuffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncRunMotionEstimationOnly: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncGetLastErrorString: Option<unsafe extern "C" fn(*mut c_void) -> *const i8>,
    nvEncSetIOCudaStreams:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> u32>,
    nvEncGetEncodePresetConfigEx2: Option<
        unsafe extern "C" fn(*mut c_void, NvEncGuid, NvEncGuid, u32, *mut NvEncPresetConfig) -> u32,
    >,
    nvEncGetSequenceParamEx: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    nvEncLookaheadPicture: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32>,
    _reserved2: [*mut c_void; 277],
}

/// 打开编码会话的参数
#[repr(C)]
#[allow(non_snake_case)]
struct NvEncOpenEncodeSessionExParams {
    version: u32,
    deviceType: u32,
    device: *mut c_void,
    reserved: *mut c_void,
    apiVersion: u32,
    reserved1: u32,
    reserved2: [*mut c_void; 64],
}

/// 编码器初始化参数
#[repr(C)]
#[allow(non_snake_case)]
struct NvEncInitializeParams {
    version: u32,
    encodeGUID: NvEncGuid,
    presetGUID: NvEncGuid,
    encodeWidth: u32,
    encodeHeight: u32,
    darWidth: u32,
    darHeight: u32,
    frameRateNum: u32,
    frameRateDen: u32,
    enableEncodeAsync: u32,
    enablePTD: u32,
    reportSliceOffsets: u32,
    enableSubFrameWrite: u32,
    enableExternalMEHints: u32,
    enableMEOnlyMode: u32,
    enableWeightedPrediction: u32,
    enableOutputInVidmem: u32,
    reservedBitFields: u32,
    privDataSize: u32,
    privData: *mut c_void,
    encodeConfig: *mut NvEncConfig,
    maxEncodeWidth: u32,
    maxEncodeHeight: u32,
    maxMEHintCountsPerBlock: [u32; 2],
    tuningInfo: u32,
    reserved: [*mut c_void; 62],
}

/// 编码配置
#[repr(C)]
struct NvEncConfig {
    version: u32,
    profile_guid: NvEncGuid,
    gop_length: u32,
    frame_field_mode: i32,
    mv_precision: u32,
    rc_params: NvEncRcParams,
    _reserved: [u8; 768], // 简化：跳过复杂的 codec-specific 配置
}

/// 码率控制参数
#[repr(C)]
#[derive(Clone, Copy)]
struct NvEncRcParams {
    version: u32,
    rc_mode: u32,
    average_bit_rate: u32,
    max_bit_rate: u32,
    vbv_buffer_size: u32,
    vbv_initial_delay: u32,
    _reserved: [u8; 256],
}

/// 预设配置（用于查询默认配置）
#[repr(C)]
struct NvEncPresetConfig {
    version: u32,
    preset_cfg: NvEncConfig,
}

/// 创建输入缓冲区参数
#[repr(C)]
#[allow(non_snake_case)]
struct NvEncCreateInputBufferParams {
    version: u32,
    width: u32,
    height: u32,
    memoryHeap: u32,
    bufferFmt: u32,
    reserved: u32,
    inputBuffer: *mut c_void,
    pSysMemBuffer: *mut c_void,
    reserved1: [u32; 57],
    reserved2: [*mut c_void; 63],
}

/// 创建输出码流缓冲区参数
#[repr(C)]
#[allow(non_snake_case)]
struct NvEncCreateBitstreamBufferParams {
    version: u32,
    size: u32,
    memoryHeap: u32,
    reserved: u32,
    bitstreamBuffer: *mut c_void,
    bitstreamBufferPtr: *mut c_void,
    reserved1: [u32; 57],
    reserved2: [*mut c_void; 63],
}

/// 编码图片参数
#[repr(C)]
#[allow(non_snake_case)]
struct NvEncPicParams {
    version: u32,
    inputWidth: u32,
    inputHeight: u32,
    inputPitch: u32,
    encodePicFlags: u32,
    frameIdx: u32,
    inputTimeStamp: u64,
    inputDuration: u64,
    inputBuffer: *mut c_void,
    outputBitstream: *mut c_void,
    completionEvent: *mut c_void,
    bufferFmt: u32,
    pictureStruct: u32,
    pictureType: u32,
    codecPicParams: [u8; 256], // 简化 codec-specific params
    _reserved: [u32; 57],
    _reserved2: [*mut c_void; 59],
}

/// 锁定码流参数
#[repr(C)]
#[allow(non_snake_case)]
struct NvEncLockBitstreamParams {
    version: u32,
    outputBitstream: *mut c_void,
    sliceOffsets: *mut u32,
    frameIdx: u32,
    hwEncodeStatus: u32,
    numSlices: u32,
    bitstreamSizeInBytes: u32,
    outputTimeStamp: u64,
    outputDuration: u64,
    bitstreamBufferPtr: *mut c_void,
    pictureType: u32,
    pictureStruct: u32,
    frameAvgQP: u32,
    frameSatd: u32,
    ltrFrameIdx: u32,
    ltrFrameBitmap: u32,
    reserved: [u32; 13],
    intraMBCount: u32,
    interMBCount: u32,
    averageMVX: i32,
    averageMVY: i32,
    reserved1: [u32; 219],
    reserved2: [*mut c_void; 64],
}

/// 锁定输入缓冲区参数
#[repr(C)]
#[allow(non_snake_case)]
struct NvEncLockInputBufferParams {
    version: u32,
    inputBuffer: *mut c_void,
    bufferDataPtr: *mut c_void,
    pitch: u32,
    reserved1: [u32; 251],
    reserved2: [*mut c_void; 64],
}

// ─── 结构体版本宏 ───

/// 生成 NVENC 结构体版本号（版本号 | 结构体大小）
macro_rules! nvenc_struct_ver {
    ($t:ty, $ver:expr) => {
        (($ver << 16) | std::mem::size_of::<$t>() as u32)
    };
}

// ─── NVENC 编码器实现 ───

/// NVENC 编码器
///
/// 通过动态加载实现，不要求编译环境安装 NVIDIA SDK。
/// new() 阶段完成完整的初始化和测试帧验证。
pub struct NvencEncoder {
    width: u32,
    height: u32,
    _lib: libloading::Library,
    fn_list: Box<NvEncApiFunctionList>,
    encoder_ptr: *mut c_void,
    input_buffer: *mut c_void,
    output_buffer: *mut c_void,
    /// D3D11 设备指针，编码器 Drop 时需要释放
    d3d11_device: *mut c_void,
    /// 保留编码配置以确保其生命周期覆盖编码器会话
    _config: Box<NvEncConfig>,
    frame_count: u64,
    force_keyframe_flag: bool,
}

unsafe impl Send for NvencEncoder {}

impl NvencEncoder {
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        // 动态加载 NVENC DLL
        let lib = unsafe { libloading::Library::new("nvEncodeAPI64.dll") }
            .context("加载 nvEncodeAPI64.dll 失败（需要安装 NVIDIA 显卡驱动）")?;

        // 验证 API 版本
        unsafe {
            let get_version: libloading::Symbol<unsafe extern "C" fn(*mut u32) -> u32> = lib
                .get(b"NvEncodeAPIGetMaxSupportedVersion")
                .context("获取 NvEncodeAPIGetMaxSupportedVersion 失败")?;

            let mut version: u32 = 0;
            let status = get_version(&mut version);
            if status != NV_ENC_SUCCESS {
                anyhow::bail!("NvEncodeAPIGetMaxSupportedVersion 返回错误: {}", status);
            }
            let major = version >> 4;
            let minor = version & 0xF;
            info!("NVENC 最大支持 API 版本: {}.{}", major, minor);
            if major < 12 {
                anyhow::bail!("NVENC API 版本 {}.{} 过低（需要 >= 12.0）", major, minor);
            }
        }

        // 获取 NVENC 函数指针表
        let create_instance: libloading::Symbol<
            unsafe extern "C" fn(*mut NvEncApiFunctionList) -> u32,
        > = unsafe { lib.get(b"NvEncodeAPICreateInstance") }
            .context("获取 NvEncodeAPICreateInstance 失败")?;

        let mut fn_list = Box::new(unsafe { std::mem::zeroed::<NvEncApiFunctionList>() });
        fn_list.version = NV_ENCODE_API_FUNCTION_LIST_VER;

        let status = unsafe { create_instance(&mut *fn_list) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("NvEncodeAPICreateInstance 失败，状态码: {}", status);
        }
        info!("NVENC 函数表已加载");

        // 创建 D3D11 设备用于 NVENC 会话
        let d3d_device =
            unsafe { create_d3d11_device() }.context("为 NVENC 创建 D3D11 设备失败")?;

        // 打开编码会话
        let open_fn = fn_list
            .nvEncOpenEncodeSessionEx
            .context("nvEncOpenEncodeSessionEx 函数指针为空")?;

        let mut session_params: NvEncOpenEncodeSessionExParams = unsafe { std::mem::zeroed() };
        session_params.version = nvenc_struct_ver!(NvEncOpenEncodeSessionExParams, 1);
        session_params.deviceType = NV_ENC_DEVICE_TYPE_DIRECTX;
        session_params.device = d3d_device;
        session_params.apiVersion = NVENCAPI_VERSION;

        let mut encoder_ptr: *mut c_void = ptr::null_mut();
        // NVENC 将编码器句柄写入 session_params 的保留字段
        // 实际上 open_fn 返回编码器句柄通过函数返回值以外的方式：
        // nvEncOpenEncodeSessionEx 的 output 写入最后一个 reserved2 字段
        // 但根据 SDK 文档，编码器句柄通过 reserved2[0] 返回
        session_params.reserved2[0] = &mut encoder_ptr as *mut _ as *mut c_void;
        // 纠正：根据实际 SDK，编码器句柄是通过一个额外的输出参数
        // NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS 没有输出字段
        // 实际上 nvEncOpenEncodeSessionEx 签名是:
        //   NvEncOpenEncodeSessionEx(params, &encoder) -> status
        // 所以我们需要用不同的调用方式

        // SAFETY: nvEncOpenEncodeSessionEx 的真实签名接受 (params, &encoder) -> status
        // NV_ENCODE_API_FUNCTION_LIST 中的函数指针类型不完全匹配 SDK 文档签名，
        // 此处通过类型擦除调用，确保参数布局严格匹配 NVENC SDK 12.2 规范。
        // 注意：如果 SDK 版本变更导致签名不兼容，此处会产生未定义行为。
        type OpenSessionExFn =
            unsafe extern "C" fn(*mut NvEncOpenEncodeSessionExParams, *mut *mut c_void) -> u32;
        let open_session_typed: OpenSessionExFn =
            unsafe { std::mem::transmute::<_, OpenSessionExFn>(open_fn) };

        let status = unsafe { open_session_typed(&mut session_params, &mut encoder_ptr) };
        if status != NV_ENC_SUCCESS || encoder_ptr.is_null() {
            anyhow::bail!("nvEncOpenEncodeSessionEx 失败，状态码: {}", status);
        }
        info!("NVENC 编码会话已创建");

        // 获取预设配置
        let get_preset_config_fn = fn_list
            .nvEncGetEncodePresetConfigEx
            .context("nvEncGetEncodePresetConfigEx 函数指针为空")?;

        let mut preset_config: NvEncPresetConfig = unsafe { std::mem::zeroed() };
        preset_config.version = nvenc_struct_ver!(NvEncPresetConfig, 1);
        preset_config.preset_cfg.version = nvenc_struct_ver!(NvEncConfig, 1);

        let status = unsafe {
            get_preset_config_fn(
                encoder_ptr,
                NV_ENC_CODEC_H264_GUID,
                NV_ENC_PRESET_P4_GUID,
                NV_ENC_TUNING_INFO_LOW_LATENCY,
                &mut preset_config,
            )
        };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncGetEncodePresetConfigEx 失败，状态码: {}", status);
        }

        // 在预设基础上修改配置
        let mut config = Box::new(preset_config.preset_cfg);
        config.profile_guid = NV_ENC_H264_PROFILE_BASELINE_GUID;
        config.gop_length = 60; // 关键帧间隔 60 帧
                                // 配置 CBR 码率控制，目标 4 Mbps
        config.rc_params.rc_mode = 2; // NV_ENC_PARAMS_RC_CBR
        config.rc_params.average_bit_rate = 4_000_000;
        config.rc_params.max_bit_rate = 6_000_000;
        config.rc_params.vbv_buffer_size = 4_000_000;
        config.rc_params.vbv_initial_delay = 4_000_000;

        // 初始化编码器
        let init_fn = fn_list
            .nvEncInitializeEncoder
            .context("nvEncInitializeEncoder 函数指针为空")?;

        let mut init_params: NvEncInitializeParams = unsafe { std::mem::zeroed() };
        init_params.version = nvenc_struct_ver!(NvEncInitializeParams, 1);
        init_params.encodeGUID = NV_ENC_CODEC_H264_GUID;
        init_params.presetGUID = NV_ENC_PRESET_P4_GUID;
        init_params.encodeWidth = width;
        init_params.encodeHeight = height;
        init_params.darWidth = width;
        init_params.darHeight = height;
        init_params.frameRateNum = 30;
        init_params.frameRateDen = 1;
        init_params.enablePTD = 1; // 自动图片类型决策
        init_params.encodeConfig = &mut *config;
        init_params.maxEncodeWidth = width;
        init_params.maxEncodeHeight = height;
        init_params.tuningInfo = NV_ENC_TUNING_INFO_LOW_LATENCY;

        let status = unsafe { init_fn(encoder_ptr, &mut init_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncInitializeEncoder 失败，状态码: {}", status);
        }
        info!("NVENC 编码器已初始化: {}x{}", width, height);

        // 创建输入缓冲区（NV12 格式）
        let create_input_fn = fn_list
            .nvEncCreateInputBuffer
            .context("nvEncCreateInputBuffer 函数指针为空")?;

        let mut input_params: NvEncCreateInputBufferParams = unsafe { std::mem::zeroed() };
        input_params.version = nvenc_struct_ver!(NvEncCreateInputBufferParams, 1);
        input_params.width = width;
        input_params.height = height;
        input_params.bufferFmt = NV_ENC_BUFFER_FORMAT_NV12;

        let status = unsafe { create_input_fn(encoder_ptr, &mut input_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncCreateInputBuffer 失败，状态码: {}", status);
        }
        let input_buffer = input_params.inputBuffer;
        info!("NVENC 输入缓冲区已创建");

        // 创建输出码流缓冲区
        let create_output_fn = fn_list
            .nvEncCreateBitstreamBuffer
            .context("nvEncCreateBitstreamBuffer 函数指针为空")?;

        let mut output_params: NvEncCreateBitstreamBufferParams = unsafe { std::mem::zeroed() };
        output_params.version = nvenc_struct_ver!(NvEncCreateBitstreamBufferParams, 1);

        let status = unsafe { create_output_fn(encoder_ptr, &mut output_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncCreateBitstreamBuffer 失败，状态码: {}", status);
        }
        let output_buffer = output_params.bitstreamBuffer;
        info!("NVENC 输出缓冲区已创建");

        let mut encoder = Self {
            width,
            height,
            _lib: lib,
            fn_list,
            encoder_ptr,
            input_buffer,
            output_buffer,
            d3d11_device: d3d_device,
            _config: config,
            frame_count: 0,
            force_keyframe_flag: false,
        };

        // 测试帧验证：编码一个全黑帧确保编码管线完整可用
        let test_frame = CapturedFrame {
            width,
            height,
            stride: width * 4,
            pixel_format: crate::frame::PixelFormat::Bgra8,
            data: vec![0u8; (width * height * 4) as usize],
            timestamp_ms: 0,
        };

        match encoder.encode(&test_frame) {
            Ok(Some(_)) => {
                info!("NVENC 测试帧编码成功 ({}x{})", width, height);
                encoder.frame_count = 0;
                Ok(encoder)
            }
            Ok(None) => {
                anyhow::bail!("NVENC 测试帧编码无输出")
            }
            Err(e) => {
                anyhow::bail!("NVENC 测试帧编码失败: {}", e)
            }
        }
    }
}

impl VideoEncoder for NvencEncoder {
    fn encode(&mut self, frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        if frame.data.is_empty() {
            return Ok(None);
        }

        // 色彩转换：BGRA -> NV12
        let nv12_data = crate::color_convert::bgra_to_nv12_alloc(
            &frame.data,
            frame.stride as usize,
            self.width as usize,
            self.height as usize,
        );

        let is_keyframe = self.force_keyframe_flag || self.frame_count.is_multiple_of(60);
        self.force_keyframe_flag = false;

        // 锁定输入缓冲区并写入 NV12 数据
        let lock_input_fn = self
            .fn_list
            .nvEncLockInputBuffer
            .context("nvEncLockInputBuffer 函数指针为空")?;
        let unlock_input_fn = self
            .fn_list
            .nvEncUnlockInputBuffer
            .context("nvEncUnlockInputBuffer 函数指针为空")?;

        let mut lock_params: NvEncLockInputBufferParams = unsafe { std::mem::zeroed() };
        lock_params.version = nvenc_struct_ver!(NvEncLockInputBufferParams, 1);
        lock_params.inputBuffer = self.input_buffer;

        let status = unsafe { lock_input_fn(self.encoder_ptr, &mut lock_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncLockInputBuffer 失败，状态码: {}", status);
        }

        // 将 NV12 数据拷贝到锁定的缓冲区
        let dst_ptr = lock_params.bufferDataPtr as *mut u8;
        let dst_pitch = lock_params.pitch as usize;
        let w = self.width as usize;
        let h = self.height as usize;

        unsafe {
            // 拷贝 Y 平面（逐行拷贝以处理 pitch 对齐）
            for row in 0..h {
                let src_offset = row * w;
                let dst_offset = row * dst_pitch;
                ptr::copy_nonoverlapping(
                    nv12_data[src_offset..].as_ptr(),
                    dst_ptr.add(dst_offset),
                    w,
                );
            }
            // 拷贝 UV 平面
            let uv_src_start = w * h;
            for row in 0..(h / 2) {
                let src_offset = uv_src_start + row * w;
                let dst_offset = h * dst_pitch + row * dst_pitch;
                ptr::copy_nonoverlapping(
                    nv12_data[src_offset..].as_ptr(),
                    dst_ptr.add(dst_offset),
                    w,
                );
            }
        }

        let status = unsafe { unlock_input_fn(self.encoder_ptr, self.input_buffer) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncUnlockInputBuffer 失败，状态码: {}", status);
        }

        // 编码当前帧
        let encode_fn = self
            .fn_list
            .nvEncEncodePicture
            .context("nvEncEncodePicture 函数指针为空")?;

        let mut pic_params: NvEncPicParams = unsafe { std::mem::zeroed() };
        pic_params.version = nvenc_struct_ver!(NvEncPicParams, 1);
        pic_params.inputWidth = self.width;
        pic_params.inputHeight = self.height;
        pic_params.inputPitch = self.width;
        pic_params.inputBuffer = self.input_buffer;
        pic_params.outputBitstream = self.output_buffer;
        pic_params.bufferFmt = NV_ENC_BUFFER_FORMAT_NV12;
        pic_params.pictureStruct = NV_ENC_PIC_STRUCT_FRAME;
        pic_params.inputTimeStamp = self.frame_count;

        if is_keyframe {
            pic_params.encodePicFlags = 0x04; // NV_ENC_PIC_FLAG_FORCEIDR
            pic_params.pictureType = NV_ENC_PIC_TYPE_IDR;
        }

        let status = unsafe { encode_fn(self.encoder_ptr, &mut pic_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncEncodePicture 失败，状态码: {}", status);
        }

        // 锁定码流输出缓冲区读取编码后的 H.264 数据
        let lock_bitstream_fn = self
            .fn_list
            .nvEncLockBitstream
            .context("nvEncLockBitstream 函数指针为空")?;
        let unlock_bitstream_fn = self
            .fn_list
            .nvEncUnlockBitstream
            .context("nvEncUnlockBitstream 函数指针为空")?;

        let mut bs_params: NvEncLockBitstreamParams = unsafe { std::mem::zeroed() };
        bs_params.version = nvenc_struct_ver!(NvEncLockBitstreamParams, 1);
        bs_params.outputBitstream = self.output_buffer;

        let status = unsafe { lock_bitstream_fn(self.encoder_ptr, &mut bs_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncLockBitstream 失败，状态码: {}", status);
        }

        // 提取 NAL 数据
        let nal_size = bs_params.bitstreamSizeInBytes as usize;
        let nal_data = if nal_size > 0 && !bs_params.bitstreamBufferPtr.is_null() {
            let src = bs_params.bitstreamBufferPtr as *const u8;
            let mut data = vec![0u8; nal_size];
            unsafe { ptr::copy_nonoverlapping(src, data.as_mut_ptr(), nal_size) };
            data
        } else {
            Vec::new()
        };

        // 检查实际的图片类型
        let actual_keyframe =
            bs_params.pictureType == NV_ENC_PIC_TYPE_IDR || bs_params.pictureType == 0; // IDR 或 I 帧

        let status = unsafe { unlock_bitstream_fn(self.encoder_ptr, self.output_buffer) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncUnlockBitstream 失败，状态码: {}", status);
        }

        self.frame_count += 1;

        if nal_data.is_empty() {
            return Ok(None);
        }

        debug!(
            "NVENC 帧 #{}: {} bytes ({})",
            self.frame_count - 1,
            nal_data.len(),
            if actual_keyframe || is_keyframe {
                "IDR"
            } else {
                "P帧"
            }
        );

        Ok(Some(DirtyRegion {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
            encoding: FrameEncoding::H264 {
                is_keyframe: actual_keyframe || is_keyframe,
            },
            data: nal_data,
        }))
    }

    fn force_keyframe(&mut self) {
        self.force_keyframe_flag = true;
    }

    fn name(&self) -> &str {
        "NVENC (GPU)"
    }
}

impl Drop for NvencEncoder {
    fn drop(&mut self) {
        unsafe {
            // 销毁输入缓冲区
            if !self.input_buffer.is_null() {
                if let Some(destroy_fn) = self.fn_list.nvEncDestroyInputBuffer {
                    let _ = destroy_fn(self.encoder_ptr, self.input_buffer);
                }
            }
            // 销毁输出缓冲区
            if !self.output_buffer.is_null() {
                if let Some(destroy_fn) = self.fn_list.nvEncDestroyBitstreamBuffer {
                    let _ = destroy_fn(self.encoder_ptr, self.output_buffer);
                }
            }
            // 销毁编码器会话
            if !self.encoder_ptr.is_null() {
                if let Some(destroy_fn) = self.fn_list.nvEncDestroyEncoder {
                    let _ = destroy_fn(self.encoder_ptr);
                }
            }
            // 释放 D3D11 设备（编码器销毁后才能释放）
            if !self.d3d11_device.is_null() {
                use windows::core::Interface;
                use windows::Win32::Graphics::Direct3D11::ID3D11Device;
                // 通过重建 COM 对象恢复引用计数管理，使其正常 Drop
                let _ = ID3D11Device::from_raw(self.d3d11_device);
                debug!("D3D11 设备已释放");
            }
        }
        debug!("NVENC 编码器资源已释放");
    }
}

// ─── NVENC HEVC (H.265) 编码器 ───

/// NVENC HEVC 编码器
///
/// 与 NvencEncoder 结构类似，但使用 HEVC codec GUID 和 Main Profile。
/// 相比 H.264 在同等画质下码率更低，适合高分辨率场景。
pub struct NvencHevcEncoder {
    width: u32,
    height: u32,
    _lib: libloading::Library,
    fn_list: Box<NvEncApiFunctionList>,
    encoder_ptr: *mut c_void,
    input_buffer: *mut c_void,
    output_buffer: *mut c_void,
    d3d11_device: *mut c_void,
    _config: Box<NvEncConfig>,
    frame_count: u64,
    force_keyframe_flag: bool,
}

unsafe impl Send for NvencHevcEncoder {}

impl NvencHevcEncoder {
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        let lib = unsafe { libloading::Library::new("nvEncodeAPI64.dll") }
            .context("加载 nvEncodeAPI64.dll 失败（需要安装 NVIDIA 显卡驱动）")?;

        unsafe {
            let get_version: libloading::Symbol<unsafe extern "C" fn(*mut u32) -> u32> = lib
                .get(b"NvEncodeAPIGetMaxSupportedVersion")
                .context("获取 NvEncodeAPIGetMaxSupportedVersion 失败")?;
            let mut version: u32 = 0;
            let status = get_version(&mut version);
            if status != NV_ENC_SUCCESS {
                anyhow::bail!("NvEncodeAPIGetMaxSupportedVersion 返回错误: {}", status);
            }
            let major = version >> 4;
            if major < 12 {
                anyhow::bail!("NVENC API 版本过低（需要 >= 12.0）");
            }
        }

        let create_instance: libloading::Symbol<
            unsafe extern "C" fn(*mut NvEncApiFunctionList) -> u32,
        > = unsafe { lib.get(b"NvEncodeAPICreateInstance") }
            .context("获取 NvEncodeAPICreateInstance 失败")?;

        let mut fn_list = Box::new(unsafe { std::mem::zeroed::<NvEncApiFunctionList>() });
        fn_list.version = NV_ENCODE_API_FUNCTION_LIST_VER;

        let status = unsafe { create_instance(&mut *fn_list) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("NvEncodeAPICreateInstance 失败，状态码: {}", status);
        }
        info!("NVENC HEVC 函数表已加载");

        let d3d_device =
            unsafe { create_d3d11_device() }.context("为 NVENC HEVC 创建 D3D11 设备失败")?;

        let open_fn = fn_list
            .nvEncOpenEncodeSessionEx
            .context("nvEncOpenEncodeSessionEx 函数指针为空")?;

        let mut session_params: NvEncOpenEncodeSessionExParams = unsafe { std::mem::zeroed() };
        session_params.version = nvenc_struct_ver!(NvEncOpenEncodeSessionExParams, 1);
        session_params.deviceType = NV_ENC_DEVICE_TYPE_DIRECTX;
        session_params.device = d3d_device;
        session_params.apiVersion = NVENCAPI_VERSION;

        let mut encoder_ptr: *mut c_void = ptr::null_mut();
        session_params.reserved2[0] = &mut encoder_ptr as *mut _ as *mut c_void;

        type OpenSessionExFn =
            unsafe extern "C" fn(*mut NvEncOpenEncodeSessionExParams, *mut *mut c_void) -> u32;
        let open_session_typed: OpenSessionExFn =
            unsafe { std::mem::transmute::<_, OpenSessionExFn>(open_fn) };

        let status = unsafe { open_session_typed(&mut session_params, &mut encoder_ptr) };
        if status != NV_ENC_SUCCESS || encoder_ptr.is_null() {
            anyhow::bail!("nvEncOpenEncodeSessionEx (HEVC) 失败，状态码: {}", status);
        }
        info!("NVENC HEVC 编码会话已创建");

        let get_preset_config_fn = fn_list
            .nvEncGetEncodePresetConfigEx
            .context("nvEncGetEncodePresetConfigEx 函数指针为空")?;

        let mut preset_config: NvEncPresetConfig = unsafe { std::mem::zeroed() };
        preset_config.version = nvenc_struct_ver!(NvEncPresetConfig, 1);
        preset_config.preset_cfg.version = nvenc_struct_ver!(NvEncConfig, 1);

        let status = unsafe {
            get_preset_config_fn(
                encoder_ptr,
                NV_ENC_CODEC_HEVC_GUID,
                NV_ENC_PRESET_P4_GUID,
                NV_ENC_TUNING_INFO_LOW_LATENCY,
                &mut preset_config,
            )
        };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!(
                "nvEncGetEncodePresetConfigEx (HEVC) 失败，状态码: {}",
                status
            );
        }

        let mut config = Box::new(preset_config.preset_cfg);
        config.profile_guid = NV_ENC_HEVC_PROFILE_MAIN_GUID;
        config.gop_length = 60;
        config.rc_params.rc_mode = 2; // CBR
        config.rc_params.average_bit_rate = 4_000_000;
        config.rc_params.max_bit_rate = 6_000_000;
        config.rc_params.vbv_buffer_size = 4_000_000;
        config.rc_params.vbv_initial_delay = 4_000_000;

        let init_fn = fn_list
            .nvEncInitializeEncoder
            .context("nvEncInitializeEncoder 函数指针为空")?;

        let mut init_params: NvEncInitializeParams = unsafe { std::mem::zeroed() };
        init_params.version = nvenc_struct_ver!(NvEncInitializeParams, 1);
        init_params.encodeGUID = NV_ENC_CODEC_HEVC_GUID;
        init_params.presetGUID = NV_ENC_PRESET_P4_GUID;
        init_params.encodeWidth = width;
        init_params.encodeHeight = height;
        init_params.darWidth = width;
        init_params.darHeight = height;
        init_params.frameRateNum = 30;
        init_params.frameRateDen = 1;
        init_params.enablePTD = 1;
        init_params.encodeConfig = &mut *config;
        init_params.maxEncodeWidth = width;
        init_params.maxEncodeHeight = height;
        init_params.tuningInfo = NV_ENC_TUNING_INFO_LOW_LATENCY;

        let status = unsafe { init_fn(encoder_ptr, &mut init_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncInitializeEncoder (HEVC) 失败，状态码: {}", status);
        }
        info!("NVENC HEVC 编码器已初始化: {}x{}", width, height);

        let create_input_fn = fn_list
            .nvEncCreateInputBuffer
            .context("nvEncCreateInputBuffer 函数指针为空")?;
        let mut input_params: NvEncCreateInputBufferParams = unsafe { std::mem::zeroed() };
        input_params.version = nvenc_struct_ver!(NvEncCreateInputBufferParams, 1);
        input_params.width = width;
        input_params.height = height;
        input_params.bufferFmt = NV_ENC_BUFFER_FORMAT_NV12;
        let status = unsafe { create_input_fn(encoder_ptr, &mut input_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncCreateInputBuffer (HEVC) 失败，状态码: {}", status);
        }
        let input_buffer = input_params.inputBuffer;

        let create_output_fn = fn_list
            .nvEncCreateBitstreamBuffer
            .context("nvEncCreateBitstreamBuffer 函数指针为空")?;
        let mut output_params: NvEncCreateBitstreamBufferParams = unsafe { std::mem::zeroed() };
        output_params.version = nvenc_struct_ver!(NvEncCreateBitstreamBufferParams, 1);
        let status = unsafe { create_output_fn(encoder_ptr, &mut output_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncCreateBitstreamBuffer (HEVC) 失败，状态码: {}", status);
        }
        let output_buffer = output_params.bitstreamBuffer;

        let mut encoder = Self {
            width,
            height,
            _lib: lib,
            fn_list,
            encoder_ptr,
            input_buffer,
            output_buffer,
            d3d11_device: d3d_device,
            _config: config,
            frame_count: 0,
            force_keyframe_flag: false,
        };

        // 测试帧验证
        let test_frame = CapturedFrame {
            width,
            height,
            stride: width * 4,
            pixel_format: crate::frame::PixelFormat::Bgra8,
            data: vec![0u8; (width * height * 4) as usize],
            timestamp_ms: 0,
        };

        match encoder.encode(&test_frame) {
            Ok(Some(_)) => {
                info!("NVENC HEVC 测试帧编码成功 ({}x{})", width, height);
                encoder.frame_count = 0;
                Ok(encoder)
            }
            Ok(None) => anyhow::bail!("NVENC HEVC 测试帧编码无输出"),
            Err(e) => anyhow::bail!("NVENC HEVC 测试帧编码失败: {}", e),
        }
    }
}

impl VideoEncoder for NvencHevcEncoder {
    fn encode(&mut self, frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        if frame.data.is_empty() {
            return Ok(None);
        }

        let nv12_data = crate::color_convert::bgra_to_nv12_alloc(
            &frame.data,
            frame.stride as usize,
            self.width as usize,
            self.height as usize,
        );

        let is_keyframe = self.force_keyframe_flag || self.frame_count.is_multiple_of(60);
        self.force_keyframe_flag = false;

        let lock_input_fn = self
            .fn_list
            .nvEncLockInputBuffer
            .context("nvEncLockInputBuffer 函数指针为空")?;
        let unlock_input_fn = self
            .fn_list
            .nvEncUnlockInputBuffer
            .context("nvEncUnlockInputBuffer 函数指针为空")?;

        let mut lock_params: NvEncLockInputBufferParams = unsafe { std::mem::zeroed() };
        lock_params.version = nvenc_struct_ver!(NvEncLockInputBufferParams, 1);
        lock_params.inputBuffer = self.input_buffer;

        let status = unsafe { lock_input_fn(self.encoder_ptr, &mut lock_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncLockInputBuffer (HEVC) 失败，状态码: {}", status);
        }

        let dst_ptr = lock_params.bufferDataPtr as *mut u8;
        let dst_pitch = lock_params.pitch as usize;
        let w = self.width as usize;
        let h = self.height as usize;

        unsafe {
            for row in 0..h {
                ptr::copy_nonoverlapping(
                    nv12_data[row * w..].as_ptr(),
                    dst_ptr.add(row * dst_pitch),
                    w,
                );
            }
            let uv_src_start = w * h;
            for row in 0..(h / 2) {
                ptr::copy_nonoverlapping(
                    nv12_data[uv_src_start + row * w..].as_ptr(),
                    dst_ptr.add(h * dst_pitch + row * dst_pitch),
                    w,
                );
            }
        }

        let status = unsafe { unlock_input_fn(self.encoder_ptr, self.input_buffer) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncUnlockInputBuffer (HEVC) 失败，状态码: {}", status);
        }

        let encode_fn = self
            .fn_list
            .nvEncEncodePicture
            .context("nvEncEncodePicture 函数指针为空")?;

        let mut pic_params: NvEncPicParams = unsafe { std::mem::zeroed() };
        pic_params.version = nvenc_struct_ver!(NvEncPicParams, 1);
        pic_params.inputWidth = self.width;
        pic_params.inputHeight = self.height;
        pic_params.inputPitch = self.width;
        pic_params.inputBuffer = self.input_buffer;
        pic_params.outputBitstream = self.output_buffer;
        pic_params.bufferFmt = NV_ENC_BUFFER_FORMAT_NV12;
        pic_params.pictureStruct = NV_ENC_PIC_STRUCT_FRAME;
        pic_params.inputTimeStamp = self.frame_count;

        if is_keyframe {
            pic_params.encodePicFlags = 0x04;
            pic_params.pictureType = NV_ENC_PIC_TYPE_IDR;
        }

        let status = unsafe { encode_fn(self.encoder_ptr, &mut pic_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncEncodePicture (HEVC) 失败，状态码: {}", status);
        }

        let lock_bitstream_fn = self
            .fn_list
            .nvEncLockBitstream
            .context("nvEncLockBitstream 函数指针为空")?;
        let unlock_bitstream_fn = self
            .fn_list
            .nvEncUnlockBitstream
            .context("nvEncUnlockBitstream 函数指针为空")?;

        let mut bs_params: NvEncLockBitstreamParams = unsafe { std::mem::zeroed() };
        bs_params.version = nvenc_struct_ver!(NvEncLockBitstreamParams, 1);
        bs_params.outputBitstream = self.output_buffer;

        let status = unsafe { lock_bitstream_fn(self.encoder_ptr, &mut bs_params) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncLockBitstream (HEVC) 失败，状态码: {}", status);
        }

        let nal_size = bs_params.bitstreamSizeInBytes as usize;
        let nal_data = if nal_size > 0 && !bs_params.bitstreamBufferPtr.is_null() {
            let src = bs_params.bitstreamBufferPtr as *const u8;
            let mut data = vec![0u8; nal_size];
            unsafe { ptr::copy_nonoverlapping(src, data.as_mut_ptr(), nal_size) };
            data
        } else {
            Vec::new()
        };

        let actual_keyframe =
            bs_params.pictureType == NV_ENC_PIC_TYPE_IDR || bs_params.pictureType == 0;

        let status = unsafe { unlock_bitstream_fn(self.encoder_ptr, self.output_buffer) };
        if status != NV_ENC_SUCCESS {
            anyhow::bail!("nvEncUnlockBitstream (HEVC) 失败，状态码: {}", status);
        }

        self.frame_count += 1;

        if nal_data.is_empty() {
            return Ok(None);
        }

        debug!(
            "NVENC HEVC 帧 #{}: {} bytes ({})",
            self.frame_count - 1,
            nal_data.len(),
            if actual_keyframe || is_keyframe {
                "IDR"
            } else {
                "P帧"
            }
        );

        Ok(Some(DirtyRegion {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
            encoding: FrameEncoding::H265 {
                is_keyframe: actual_keyframe || is_keyframe,
            },
            data: nal_data,
        }))
    }

    fn force_keyframe(&mut self) {
        self.force_keyframe_flag = true;
    }

    fn name(&self) -> &str {
        "NVENC HEVC (GPU)"
    }
}

impl Drop for NvencHevcEncoder {
    fn drop(&mut self) {
        unsafe {
            if !self.input_buffer.is_null() {
                if let Some(destroy_fn) = self.fn_list.nvEncDestroyInputBuffer {
                    let _ = destroy_fn(self.encoder_ptr, self.input_buffer);
                }
            }
            if !self.output_buffer.is_null() {
                if let Some(destroy_fn) = self.fn_list.nvEncDestroyBitstreamBuffer {
                    let _ = destroy_fn(self.encoder_ptr, self.output_buffer);
                }
            }
            if !self.encoder_ptr.is_null() {
                if let Some(destroy_fn) = self.fn_list.nvEncDestroyEncoder {
                    let _ = destroy_fn(self.encoder_ptr);
                }
            }
            if !self.d3d11_device.is_null() {
                use windows::Win32::Graphics::Direct3D11::ID3D11Device;
                let _ = ID3D11Device::from_raw(self.d3d11_device);
                debug!("D3D11 设备已释放 (HEVC)");
            }
        }
        debug!("NVENC HEVC 编码器资源已释放");
    }
}

// ─── D3D11 设备创建辅助函数 ───

/// 创建 D3D11 设备供 NVENC 使用
///
/// NVENC 需要一个 D3D11 设备作为硬件加速上下文。
/// 创建一个仅用于编码的轻量级设备（不创建交换链）。
unsafe fn create_d3d11_device() -> anyhow::Result<*mut c_void> {
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::*;

    let mut device = None;

    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        None,
        D3D11_CREATE_DEVICE_FLAG(0),
        Some(&[]),
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    )
    .context("D3D11CreateDevice 失败（无可用的 GPU 硬件设备）")?;

    let device = device.context("D3D11 设备创建返回空指针")?;

    // 将 COM 接口指针转换为 raw pointer
    // 使用 into_raw() 而非 transmute，显式转移所有权而不泄漏引用计数。
    // NVENC 会话不负责释放 D3D11 设备，需要在编码器 Drop 时手动释放。
    // 但由于编码器整个生命周期都需要设备存活，这里用 ManuallyDrop 确保
    // 设备在编码器销毁前不被提前释放。
    let device_raw = std::mem::ManuallyDrop::new(device);
    let raw: *mut c_void = device_raw.as_raw();
    Ok(raw)
}
