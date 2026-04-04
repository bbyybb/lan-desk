/// Linux VA-API H.264 硬件编码器
///
/// 通过 libloading 动态加载 libva.so.2 和 libva-drm.so.2，
/// 使用 VA-API 进行 H.264 硬件编码。
/// 无需编译时链接 VA-API SDK，运行时检测 GPU 可用性。
use std::ffi::c_void;
use std::ptr;

use anyhow::Context;
use tracing::{debug, info};

use crate::frame::CapturedFrame;
use crate::gpu_encoder::VideoEncoder;
use lan_desk_protocol::message::{DirtyRegion, FrameEncoding};

// ─── VA-API 类型定义 ───

type VADisplay = *mut c_void;
type VAConfigID = u32;
type VAContextID = u32;
type VASurfaceID = u32;
type VABufferID = u32;
type VAStatus = i32;
type VAEntrypoint = u32;
type VAProfile = u32;
type VAImageFormat = VAImageFormatInner;

// ─── VA-API 常量 ───

const VA_STATUS_SUCCESS: VAStatus = 0;
const VA_RT_FORMAT_YUV420: u32 = 0x00000001;
const VA_PROGRESSIVE: u32 = 0x00000001;

// H.264 Profile
const VA_PROFILE_H264_BASELINE: VAProfile = 5;
const VA_PROFILE_H264_MAIN: VAProfile = 6;
const VA_PROFILE_H264_HIGH: VAProfile = 7;
#[allow(dead_code)]
const VA_PROFILE_H264_CONSTRAINED_BASELINE: VAProfile = 13;

// H.264 Entrypoint
const VA_ENTRYPOINT_ENCSLICE: VAEntrypoint = 5;
const VA_ENTRYPOINT_ENCSLICE_LP: VAEntrypoint = 8;

// Buffer 类型
const VA_ENC_CODED_BUFFER_TYPE: u32 = 21;
const VA_ENC_SEQUENCE_PARAMETER_BUFFER_TYPE: u32 = 22;
const VA_ENC_PICTURE_PARAMETER_BUFFER_TYPE: u32 = 23;
const VA_ENC_SLICE_PARAMETER_BUFFER_TYPE: u32 = 24;
#[allow(dead_code)]
const VA_ENC_MISC_PARAMETER_BUFFER_TYPE: u32 = 27;

// 图片标志
const VA_PICTURE_H264_INVALID: u32 = 0x00000001;

// Surface attrib 类型
const VA_SURFACE_ATTRIB_PIXEL_FORMAT: u32 = 1;
const VA_SURFACE_ATTRIB_SETTABLE: u32 = 0x00000002;

// NV12 fourcc: 'N','V','1','2'
const VA_FOURCC_NV12: u32 = 0x3231564E;

// 无效 surface / buffer ID
const VA_INVALID_ID: u32 = 0xFFFFFFFF;
const VA_INVALID_SURFACE: VASurfaceID = 0xFFFFFFFF;

// ─── VA-API FFI 结构体 ───

/// VA-API 图像格式
#[repr(C)]
#[derive(Clone, Copy)]
struct VAImageFormatInner {
    fourcc: u32,
    byte_order: u32,
    bits_per_pixel: u32,
    depth: u32,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    alpha_mask: u32,
}

/// VA-API 图像
#[repr(C)]
#[derive(Clone, Copy)]
struct VAImage {
    image_id: u32,
    format: VAImageFormat,
    buf: VABufferID,
    width: u16,
    height: u16,
    num_planes: u32,
    pitches: [u32; 3],
    offsets: [u32; 3],
    data_size: u32,
    _reserved: u32,
}

/// VA surface 属性
#[repr(C)]
#[derive(Clone, Copy)]
struct VASurfaceAttrib {
    type_: u32,
    flags: u32,
    value: VAGenericValue,
}

/// VA 通用值（union 的简化版本，使用 i32 值）
#[repr(C)]
#[derive(Clone, Copy)]
struct VAGenericValue {
    type_: u32, // 0=int, 1=float, 2=pointer, 3=func
    value: i64, // 足够容纳各类值（i32/f32/pointer）
}

/// VA-API 配置属性
#[repr(C)]
#[derive(Clone, Copy)]
struct VAConfigAttrib {
    type_: u32,
    value: u32,
}

/// VA 编码输出段（coded buffer 的链表节点）
#[repr(C)]
struct VACodedBufferSegment {
    size: u32,
    bit_offset: u32,
    status: u32,
    reserved: u32,
    buf: *mut c_void,
    next: *mut VACodedBufferSegment,
}

// ─── H.264 编码参数结构体 ───

/// H.264 参考帧信息
#[repr(C)]
#[derive(Clone, Copy)]
struct VAPictureH264 {
    picture_id: VASurfaceID,
    frame_idx: u32,
    flags: u32,
    top_field_order_cnt: i32,
    bottom_field_order_cnt: i32,
}

impl Default for VAPictureH264 {
    fn default() -> Self {
        Self {
            picture_id: VA_INVALID_SURFACE,
            frame_idx: 0,
            flags: VA_PICTURE_H264_INVALID,
            top_field_order_cnt: 0,
            bottom_field_order_cnt: 0,
        }
    }
}

/// H.264 序列参数集 (SPS)
#[repr(C)]
#[derive(Clone)]
struct VAEncSequenceParameterBufferH264 {
    seq_parameter_set_id: u8,
    level_idc: u8,
    intra_period: u32,
    intra_idr_period: u32,
    ip_period: u32,
    bits_per_second: u32,
    max_num_ref_frames: u32,
    picture_width_in_mbs: u16,
    picture_height_in_mbs: u16,

    // 序列字段（使用 u32 位域简化）
    seq_fields: u32,

    bit_depth_luma_minus8: u8,
    bit_depth_chroma_minus8: u8,
    num_ref_frames_in_pic_order_cnt_cycle: u8,
    offset_for_non_ref_pic: i32,
    offset_for_top_to_bottom_field: i32,
    offset_for_ref_frame: [i32; 256],
    frame_cropping_flag: u8,
    frame_crop_left_offset: u32,
    frame_crop_right_offset: u32,
    frame_crop_top_offset: u32,
    frame_crop_bottom_offset: u32,

    vui_parameters_present_flag: u8,
    vui_fields: u32,
    aspect_ratio_idc: u8,
    sar_width: u32,
    sar_height: u32,
    num_units_in_tick: u32,
    time_scale: u32,
}

/// H.264 图片参数集 (PPS)
#[repr(C)]
#[derive(Clone)]
struct VAEncPictureParameterBufferH264 {
    curr_pic: VAPictureH264,
    reference_frames: [VAPictureH264; 16],
    coded_buf: VABufferID,

    pic_parameter_set_id: u8,
    seq_parameter_set_id: u8,
    last_picture: u8,
    frame_num: u16,

    pic_init_qp: u8,
    num_ref_idx_l0_active_minus1: u8,
    num_ref_idx_l1_active_minus1: u8,
    chroma_qp_index_offset: i8,
    second_chroma_qp_index_offset: i8,

    // 图片字段（使用 u32 位域简化）
    pic_fields: u32,
}

/// H.264 Slice 参数
#[repr(C)]
#[derive(Clone)]
struct VAEncSliceParameterBufferH264 {
    macroblock_address: u32,
    num_macroblocks: u32,
    macroblock_info: VABufferID,
    slice_type: u8,
    pic_parameter_set_id: u8,
    idr_pic_id: u16,
    pic_order_cnt_lsb: u16,
    delta_pic_order_cnt_bottom: i32,
    delta_pic_order_cnt: [i32; 2],
    direct_spatial_mv_pred_flag: u8,
    num_ref_idx_active_override_flag: u8,
    num_ref_idx_l0_active_minus1: u8,
    num_ref_idx_l1_active_minus1: u8,
    ref_pic_list_0: [VAPictureH264; 32],
    ref_pic_list_1: [VAPictureH264; 32],
    luma_log2_weight_denom: u8,
    chroma_log2_weight_denom: u8,
    luma_weight_l0_flag: u8,
    luma_weight_l0: [i16; 32],
    luma_offset_l0: [i16; 32],
    chroma_weight_l0_flag: u8,
    chroma_weight_l0: [[i16; 2]; 32],
    chroma_offset_l0: [[i16; 2]; 32],
    luma_weight_l1_flag: u8,
    luma_weight_l1: [i16; 32],
    luma_offset_l1: [i16; 32],
    chroma_weight_l1_flag: u8,
    chroma_weight_l1: [[i16; 2]; 32],
    chroma_offset_l1: [[i16; 2]; 32],
    cabac_init_idc: u8,
    slice_qp_delta: i8,
    disable_deblocking_filter_idc: u8,
    slice_alpha_c0_offset_div2: i8,
    slice_beta_offset_div2: i8,
}

// ─── VA-API 函数指针表 ───

#[allow(non_snake_case)]
struct VaApiFunctions {
    vaInitialize: unsafe extern "C" fn(VADisplay, *mut i32, *mut i32) -> VAStatus,
    vaTerminate: unsafe extern "C" fn(VADisplay) -> VAStatus,
    vaCreateConfig: unsafe extern "C" fn(
        VADisplay,
        VAProfile,
        VAEntrypoint,
        *mut VAConfigAttrib,
        i32,
        *mut VAConfigID,
    ) -> VAStatus,
    vaDestroyConfig: unsafe extern "C" fn(VADisplay, VAConfigID) -> VAStatus,
    vaCreateContext: unsafe extern "C" fn(
        VADisplay,
        VAConfigID,
        i32,
        i32,
        i32,
        *mut VASurfaceID,
        i32,
        *mut VAContextID,
    ) -> VAStatus,
    vaDestroyContext: unsafe extern "C" fn(VADisplay, VAContextID) -> VAStatus,
    vaCreateSurfaces: unsafe extern "C" fn(
        VADisplay,
        u32,
        u32,
        u32,
        *mut VASurfaceID,
        u32,
        *mut VASurfaceAttrib,
        u32,
    ) -> VAStatus,
    vaDestroySurfaces: unsafe extern "C" fn(VADisplay, *mut VASurfaceID, i32) -> VAStatus,
    vaCreateBuffer: unsafe extern "C" fn(
        VADisplay,
        VAContextID,
        u32,
        u32,
        u32,
        *mut c_void,
        *mut VABufferID,
    ) -> VAStatus,
    vaDestroyBuffer: unsafe extern "C" fn(VADisplay, VABufferID) -> VAStatus,
    vaMapBuffer: unsafe extern "C" fn(VADisplay, VABufferID, *mut *mut c_void) -> VAStatus,
    vaUnmapBuffer: unsafe extern "C" fn(VADisplay, VABufferID) -> VAStatus,
    vaBeginPicture: unsafe extern "C" fn(VADisplay, VAContextID, VASurfaceID) -> VAStatus,
    vaRenderPicture: unsafe extern "C" fn(VADisplay, VAContextID, *mut VABufferID, i32) -> VAStatus,
    vaEndPicture: unsafe extern "C" fn(VADisplay, VAContextID) -> VAStatus,
    vaSyncSurface: unsafe extern "C" fn(VADisplay, VASurfaceID) -> VAStatus,
    vaGetDisplayDRM: unsafe extern "C" fn(i32) -> VADisplay,
    vaDeriveImage: unsafe extern "C" fn(VADisplay, VASurfaceID, *mut VAImage) -> VAStatus,
    vaDestroyImage: unsafe extern "C" fn(VADisplay, u32) -> VAStatus,
    vaQueryConfigEntrypoints:
        unsafe extern "C" fn(VADisplay, VAProfile, *mut VAEntrypoint, *mut i32) -> VAStatus,
}

/// 检查 VA-API 调用返回值
fn va_check(status: VAStatus, op: &str) -> anyhow::Result<()> {
    if status != VA_STATUS_SUCCESS {
        anyhow::bail!("VA-API {} 失败，状态码: {}", op, status);
    }
    Ok(())
}

// ─── 编码器实现 ───

/// VA-API H.264 硬件编码器
pub struct VaapiEncoder {
    width: u32,
    height: u32,
    _lib_va: libloading::Library,
    _lib_va_drm: libloading::Library,
    funcs: VaApiFunctions,
    display: VADisplay,
    config_id: VAConfigID,
    context_id: VAContextID,
    /// surfaces[0] = 参考帧, surfaces[1] = 当前编码帧
    surfaces: Vec<VASurfaceID>,
    coded_buf: VABufferID,
    frame_count: u64,
    keyframe_interval: u64,
    current_surface: usize,
    drm_fd: i32,
    force_keyframe_flag: bool,
    /// 低功耗入口点（某些驱动仅支持 LP）
    #[allow(dead_code)]
    use_low_power: bool,
}

// VA-API 句柄本身是线程安全的（同一时刻单线程使用编码器即可）
unsafe impl Send for VaapiEncoder {}

/// 加载 VA-API 函数指针表
unsafe fn load_va_functions(
    lib_va: &libloading::Library,
    lib_va_drm: &libloading::Library,
) -> anyhow::Result<VaApiFunctions> {
    macro_rules! load_fn {
        ($lib:expr, $name:expr) => {{
            let sym: libloading::Symbol<*const c_void> = $lib
                .get($name)
                .with_context(|| format!("加载函数 {} 失败", String::from_utf8_lossy($name)))?;
            if sym.is_null() {
                anyhow::bail!("函数 {} 为空指针", String::from_utf8_lossy($name));
            }
            std::mem::transmute(*sym)
        }};
    }

    Ok(VaApiFunctions {
        vaInitialize: load_fn!(lib_va, b"vaInitialize\0"),
        vaTerminate: load_fn!(lib_va, b"vaTerminate\0"),
        vaCreateConfig: load_fn!(lib_va, b"vaCreateConfig\0"),
        vaDestroyConfig: load_fn!(lib_va, b"vaDestroyConfig\0"),
        vaCreateContext: load_fn!(lib_va, b"vaCreateContext\0"),
        vaDestroyContext: load_fn!(lib_va, b"vaDestroyContext\0"),
        vaCreateSurfaces: load_fn!(lib_va, b"vaCreateSurfaces\0"),
        vaDestroySurfaces: load_fn!(lib_va, b"vaDestroySurfaces\0"),
        vaCreateBuffer: load_fn!(lib_va, b"vaCreateBuffer\0"),
        vaDestroyBuffer: load_fn!(lib_va, b"vaDestroyBuffer\0"),
        vaMapBuffer: load_fn!(lib_va, b"vaMapBuffer\0"),
        vaUnmapBuffer: load_fn!(lib_va, b"vaUnmapBuffer\0"),
        vaBeginPicture: load_fn!(lib_va, b"vaBeginPicture\0"),
        vaRenderPicture: load_fn!(lib_va, b"vaRenderPicture\0"),
        vaEndPicture: load_fn!(lib_va, b"vaEndPicture\0"),
        vaSyncSurface: load_fn!(lib_va, b"vaSyncSurface\0"),
        vaDeriveImage: load_fn!(lib_va, b"vaDeriveImage\0"),
        vaDestroyImage: load_fn!(lib_va, b"vaDestroyImage\0"),
        vaQueryConfigEntrypoints: load_fn!(lib_va, b"vaQueryConfigEntrypoints\0"),
        vaGetDisplayDRM: load_fn!(lib_va_drm, b"vaGetDisplayDRM\0"),
    })
}

impl VaapiEncoder {
    /// 创建 VA-API 编码器实例
    ///
    /// 如果系统不支持 VA-API（libva 缺失、无 DRM 渲染节点或不支持 H.264 编码），返回错误。
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        // 1. 枚举 /dev/dri/renderD* 设备（128-191），找到第一个可用的
        let mut drm_fd = -1i32;
        for i in 128..=191 {
            let path = format!("/dev/dri/renderD{}\0", i);
            let fd = unsafe { libc::open(path.as_ptr() as *const libc::c_char, libc::O_RDWR) };
            if fd >= 0 {
                drm_fd = fd;
                tracing::info!("VA-API: 使用 DRM render node /dev/dri/renderD{}", i);
                break;
            }
        }
        if drm_fd < 0 {
            anyhow::bail!("VA-API: 未找到可用的 DRM render node (/dev/dri/renderD128-191)");
        }

        // 2. 加载动态库
        let lib_va = unsafe {
            libloading::Library::new("libva.so.2")
                .or_else(|_| libloading::Library::new("libva.so"))
                .map_err(|e| anyhow::anyhow!("VA-API: libva.so 未找到 ({})", e))?
        };
        let lib_va_drm = unsafe {
            libloading::Library::new("libva-drm.so.2")
                .or_else(|_| libloading::Library::new("libva-drm.so"))
                .map_err(|e| anyhow::anyhow!("VA-API: libva-drm.so 未找到 ({})", e))?
        };

        // 3. 加载函数指针
        let funcs = unsafe { load_va_functions(&lib_va, &lib_va_drm)? };

        // 4. 获取 VA display
        let display = unsafe { (funcs.vaGetDisplayDRM)(drm_fd) };
        if display.is_null() {
            unsafe {
                libc::close(drm_fd);
            }
            anyhow::bail!("VA-API: vaGetDisplayDRM 返回空指针");
        }

        // 5. 初始化 VA-API
        let mut major_ver: i32 = 0;
        let mut minor_ver: i32 = 0;
        let status = unsafe { (funcs.vaInitialize)(display, &mut major_ver, &mut minor_ver) };
        if status != VA_STATUS_SUCCESS {
            unsafe {
                libc::close(drm_fd);
            }
            anyhow::bail!("VA-API: vaInitialize 失败，状态码: {}", status);
        }
        info!("VA-API 版本: {}.{}", major_ver, minor_ver);

        // 6. 查询 H.264 编码支持
        // 尝试多个 profile: High > Main > Baseline
        let profiles_to_try = [
            (VA_PROFILE_H264_HIGH, "High"),
            (VA_PROFILE_H264_MAIN, "Main"),
            (VA_PROFILE_H264_BASELINE, "Baseline"),
        ];

        let mut selected_profile: Option<VAProfile> = None;
        let mut use_low_power = false;

        for &(profile, profile_name) in &profiles_to_try {
            let mut entrypoints = [0u32; 32];
            let mut num_entrypoints: i32 = 0;
            let status = unsafe {
                (funcs.vaQueryConfigEntrypoints)(
                    display,
                    profile,
                    entrypoints.as_mut_ptr(),
                    &mut num_entrypoints,
                )
            };
            if status != VA_STATUS_SUCCESS {
                continue;
            }

            let ep_slice = &entrypoints[..num_entrypoints as usize];

            // 优先使用标准入口点，回退到低功耗入口点
            if ep_slice.contains(&VA_ENTRYPOINT_ENCSLICE) {
                info!("VA-API: 使用 H.264 {} + VAEntrypointEncSlice", profile_name);
                selected_profile = Some(profile);
                use_low_power = false;
                break;
            } else if ep_slice.contains(&VA_ENTRYPOINT_ENCSLICE_LP) {
                info!(
                    "VA-API: 使用 H.264 {} + VAEntrypointEncSliceLP (低功耗)",
                    profile_name
                );
                selected_profile = Some(profile);
                use_low_power = true;
                break;
            }
        }

        let profile = selected_profile.ok_or_else(|| {
            unsafe {
                (funcs.vaTerminate)(display);
                libc::close(drm_fd);
            }
            anyhow::anyhow!("VA-API: 该 GPU 不支持 H.264 编码")
        })?;
        let entrypoint = if use_low_power {
            VA_ENTRYPOINT_ENCSLICE_LP
        } else {
            VA_ENTRYPOINT_ENCSLICE
        };

        // 7. 创建编码配置
        // 配置属性：RT 格式 YUV420
        let mut attrib = VAConfigAttrib {
            type_: 1, // VAConfigAttribRTFormat
            value: VA_RT_FORMAT_YUV420,
        };

        let mut config_id: VAConfigID = 0;
        let status = unsafe {
            (funcs.vaCreateConfig)(display, profile, entrypoint, &mut attrib, 1, &mut config_id)
        };
        if status != VA_STATUS_SUCCESS {
            unsafe {
                (funcs.vaTerminate)(display);
                libc::close(drm_fd);
            }
            anyhow::bail!("VA-API: vaCreateConfig 失败，状态码: {}", status);
        }

        // 8. 创建 NV12 表面（2 个：参考帧 + 当前帧）
        let num_surfaces: u32 = 2;
        let mut surfaces = vec![VA_INVALID_SURFACE; num_surfaces as usize];

        // 设置 surface 属性为 NV12 像素格式
        let mut surf_attrib = VASurfaceAttrib {
            type_: VA_SURFACE_ATTRIB_PIXEL_FORMAT,
            flags: VA_SURFACE_ATTRIB_SETTABLE,
            value: VAGenericValue {
                type_: 0, // VAGenericValueTypeInteger
                value: VA_FOURCC_NV12 as i64,
            },
        };

        let status = unsafe {
            (funcs.vaCreateSurfaces)(
                display,
                VA_RT_FORMAT_YUV420,
                width,
                height,
                surfaces.as_mut_ptr(),
                num_surfaces,
                &mut surf_attrib,
                1,
            )
        };
        if status != VA_STATUS_SUCCESS {
            unsafe {
                (funcs.vaDestroyConfig)(display, config_id);
                (funcs.vaTerminate)(display);
                libc::close(drm_fd);
            }
            anyhow::bail!("VA-API: vaCreateSurfaces 失败，状态码: {}", status);
        }
        info!(
            "VA-API: 已创建 {} 个 NV12 表面 ({}x{})",
            num_surfaces, width, height
        );

        // 9. 创建编码上下文
        let mut context_id: VAContextID = 0;
        let status = unsafe {
            (funcs.vaCreateContext)(
                display,
                config_id,
                width as i32,
                height as i32,
                VA_PROGRESSIVE as i32,
                surfaces.as_mut_ptr(),
                num_surfaces as i32,
                &mut context_id,
            )
        };
        if status != VA_STATUS_SUCCESS {
            unsafe {
                (funcs.vaDestroySurfaces)(display, surfaces.as_mut_ptr(), num_surfaces as i32);
                (funcs.vaDestroyConfig)(display, config_id);
                (funcs.vaTerminate)(display);
                libc::close(drm_fd);
            }
            anyhow::bail!("VA-API: vaCreateContext 失败，状态码: {}", status);
        }

        // 10. 创建编码输出缓冲区 (coded buffer)
        // 大小估算：宽*高 用于存储编码后的码流数据
        let coded_buf_size = (width * height) as u32;
        let mut coded_buf: VABufferID = VA_INVALID_ID;
        let status = unsafe {
            (funcs.vaCreateBuffer)(
                display,
                context_id,
                VA_ENC_CODED_BUFFER_TYPE,
                coded_buf_size,
                1,
                ptr::null_mut(),
                &mut coded_buf,
            )
        };
        if status != VA_STATUS_SUCCESS {
            unsafe {
                (funcs.vaDestroyContext)(display, context_id);
                (funcs.vaDestroySurfaces)(display, surfaces.as_mut_ptr(), num_surfaces as i32);
                (funcs.vaDestroyConfig)(display, config_id);
                (funcs.vaTerminate)(display);
                libc::close(drm_fd);
            }
            anyhow::bail!("VA-API: vaCreateBuffer(coded) 失败，状态码: {}", status);
        }

        info!(
            "VA-API 编码器初始化完成: {}x{}, coded_buf_size={}",
            width, height, coded_buf_size
        );

        let mut encoder = Self {
            width,
            height,
            _lib_va: lib_va,
            _lib_va_drm: lib_va_drm,
            funcs,
            display,
            config_id,
            context_id,
            surfaces,
            coded_buf,
            frame_count: 0,
            keyframe_interval: 60,
            current_surface: 0,
            drm_fd,
            force_keyframe_flag: false,
            use_low_power,
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
                info!("VA-API 测试帧编码成功 ({}x{})", width, height);
                encoder.frame_count = 0;
                Ok(encoder)
            }
            Ok(None) => {
                anyhow::bail!("VA-API 测试帧编码无输出")
            }
            Err(e) => {
                anyhow::bail!("VA-API 测试帧编码失败: {}", e)
            }
        }
    }

    /// 上传 NV12 数据到 VA surface
    fn upload_nv12_to_surface(&self, surface: VASurfaceID, nv12_data: &[u8]) -> anyhow::Result<()> {
        let w = self.width as usize;
        let h = self.height as usize;

        // 使用 vaDeriveImage 获取 surface 的内存映射
        let mut image: VAImage = unsafe { std::mem::zeroed() };
        let status = unsafe { (self.funcs.vaDeriveImage)(self.display, surface, &mut image) };
        va_check(status, "vaDeriveImage")?;

        // 映射 buffer 到用户空间内存
        let mut buf_ptr: *mut c_void = ptr::null_mut();
        let status = unsafe { (self.funcs.vaMapBuffer)(self.display, image.buf, &mut buf_ptr) };
        if status != VA_STATUS_SUCCESS {
            unsafe {
                (self.funcs.vaDestroyImage)(self.display, image.image_id);
            }
            anyhow::bail!("VA-API: vaMapBuffer 失败，状态码: {}", status);
        }

        let dst = buf_ptr as *mut u8;

        unsafe {
            // 拷贝 Y 平面（逐行拷贝以处理 pitch 对齐）
            let y_pitch = image.pitches[0] as usize;
            let y_offset = image.offsets[0] as usize;
            for row in 0..h {
                let src_off = row * w;
                let dst_off = y_offset + row * y_pitch;
                ptr::copy_nonoverlapping(nv12_data[src_off..].as_ptr(), dst.add(dst_off), w);
            }

            // 拷贝 UV 平面（交错 UV）
            let uv_pitch = image.pitches[1] as usize;
            let uv_offset = image.offsets[1] as usize;
            let uv_src_start = w * h;
            for row in 0..(h / 2) {
                let src_off = uv_src_start + row * w;
                let dst_off = uv_offset + row * uv_pitch;
                ptr::copy_nonoverlapping(nv12_data[src_off..].as_ptr(), dst.add(dst_off), w);
            }
        }

        // 取消映射并销毁 image
        unsafe {
            (self.funcs.vaUnmapBuffer)(self.display, image.buf);
            (self.funcs.vaDestroyImage)(self.display, image.image_id);
        }

        Ok(())
    }

    /// 构建 SPS (序列参数集) 缓冲区
    fn create_seq_param_buffer(&self, is_keyframe: bool) -> anyhow::Result<VABufferID> {
        let width_in_mbs = (self.width + 15) / 16;
        let height_in_mbs = (self.height + 15) / 16;

        // seq_fields 位域：
        // bit 0: chroma_format_idc (1 = 4:2:0) — 使用低 2 位
        // bit 2: residual_colour_transform_flag = 0
        // bit 3: gaps_in_frame_num_value_allowed_flag = 0
        // bit 4: frame_mbs_only_flag = 1 (逐行扫描)
        // bit 5: mb_adaptive_frame_field_flag = 0
        // bit 6: direct_8x8_inference_flag = 0
        // bit 7: MinLumaBiPredSize8x8 = 0
        // bit 8: log2_max_frame_num_minus4 = 4 (max_frame_num = 256)
        // bit 12: pic_order_cnt_type = 0
        // bit 14: log2_max_pic_order_cnt_lsb_minus4 = 2 (max_poc_lsb = 64)
        // bit 18: delta_pic_order_always_zero_flag = 0
        let seq_fields: u32 =
            1           // chroma_format_idc = 1 (4:2:0)
            | (1 << 4)  // frame_mbs_only_flag = 1
            | (4 << 8)  // log2_max_frame_num_minus4 = 4
            | (0 << 12) // pic_order_cnt_type = 0
            | (2 << 14) // log2_max_pic_order_cnt_lsb_minus4 = 2
        ;

        let mut sps = VAEncSequenceParameterBufferH264 {
            seq_parameter_set_id: 0,
            level_idc: 41, // Level 4.1
            intra_period: self.keyframe_interval as u32,
            intra_idr_period: self.keyframe_interval as u32,
            ip_period: 1,               // 无 B 帧：每个 P 帧紧跟 I 帧
            bits_per_second: 4_000_000, // 4 Mbps CBR
            max_num_ref_frames: 1,
            picture_width_in_mbs: width_in_mbs as u16,
            picture_height_in_mbs: height_in_mbs as u16,
            seq_fields,
            bit_depth_luma_minus8: 0,
            bit_depth_chroma_minus8: 0,
            num_ref_frames_in_pic_order_cnt_cycle: 0,
            offset_for_non_ref_pic: 0,
            offset_for_top_to_bottom_field: 0,
            offset_for_ref_frame: [0i32; 256],
            frame_cropping_flag: 0,
            frame_crop_left_offset: 0,
            frame_crop_right_offset: 0,
            frame_crop_top_offset: 0,
            frame_crop_bottom_offset: 0,
            vui_parameters_present_flag: 1,
            vui_fields: 0, // 简化：不设置 VUI 位域
            aspect_ratio_idc: 0,
            sar_width: 0,
            sar_height: 0,
            num_units_in_tick: 1,
            time_scale: 60, // 30fps → time_scale=60, num_units_in_tick=1
        };

        // 处理非 16 像素对齐的分辨率裁剪
        let aligned_width = width_in_mbs as u32 * 16;
        let aligned_height = height_in_mbs as u32 * 16;
        if aligned_width != self.width || aligned_height != self.height {
            sps.frame_cropping_flag = 1;
            sps.frame_crop_right_offset = (aligned_width - self.width) / 2;
            sps.frame_crop_bottom_offset = (aligned_height - self.height) / 2;
        }

        let _ = is_keyframe; // SPS 每帧都一样

        let mut buf_id: VABufferID = VA_INVALID_ID;
        let status = unsafe {
            (self.funcs.vaCreateBuffer)(
                self.display,
                self.context_id,
                VA_ENC_SEQUENCE_PARAMETER_BUFFER_TYPE,
                std::mem::size_of::<VAEncSequenceParameterBufferH264>() as u32,
                1,
                &mut sps as *mut _ as *mut c_void,
                &mut buf_id,
            )
        };
        va_check(status, "vaCreateBuffer(SPS)")?;
        Ok(buf_id)
    }

    /// 构建 PPS (图片参数集) 缓冲区
    fn create_pic_param_buffer(
        &self,
        current_surface: VASurfaceID,
        ref_surface: VASurfaceID,
        is_keyframe: bool,
    ) -> anyhow::Result<VABufferID> {
        let frame_num = if is_keyframe {
            0
        } else {
            (self.frame_count % 256) as u16
        };

        // 当前帧的 picture 信息
        let curr_pic = VAPictureH264 {
            picture_id: current_surface,
            frame_idx: frame_num as u32,
            flags: 0,
            top_field_order_cnt: (self.frame_count * 2) as i32,
            bottom_field_order_cnt: (self.frame_count * 2) as i32,
        };

        // 参考帧列表
        let mut reference_frames = [VAPictureH264::default(); 16];
        if !is_keyframe {
            reference_frames[0] = VAPictureH264 {
                picture_id: ref_surface,
                frame_idx: ((self.frame_count - 1) % 256) as u32,
                flags: 0,
                top_field_order_cnt: ((self.frame_count - 1) * 2) as i32,
                bottom_field_order_cnt: ((self.frame_count - 1) * 2) as i32,
            };
        }

        // pic_fields 位域：
        // bit 0: idr_pic_flag
        // bit 1: reference_pic_flag = 1 (所有帧都作为参考帧)
        // bit 2: entropy_coding_mode_flag = 0 (CAVLC for Baseline)
        // bit 3: weighted_pred_flag = 0
        // bit 5: weighted_bipred_idc = 0
        // bit 7: constrained_intra_pred_flag = 0
        // bit 8: transform_8x8_mode_flag = 0
        // bit 9: deblocking_filter_control_present_flag = 1
        // bit 10: redundant_pic_cnt_present_flag = 0
        // bit 11: pic_order_present_flag = 0
        // bit 12: pic_scaling_matrix_present_flag = 0
        let pic_fields: u32 =
            (if is_keyframe { 1 } else { 0 })  // idr_pic_flag
            | (1 << 1)   // reference_pic_flag
            | (1 << 9)   // deblocking_filter_control_present_flag
        ;

        let pps = VAEncPictureParameterBufferH264 {
            curr_pic,
            reference_frames,
            coded_buf: self.coded_buf,
            pic_parameter_set_id: 0,
            seq_parameter_set_id: 0,
            last_picture: 0,
            frame_num,
            pic_init_qp: 26,
            num_ref_idx_l0_active_minus1: 0,
            num_ref_idx_l1_active_minus1: 0,
            chroma_qp_index_offset: 0,
            second_chroma_qp_index_offset: 0,
            pic_fields,
        };

        let mut buf_id: VABufferID = VA_INVALID_ID;
        let status = unsafe {
            (self.funcs.vaCreateBuffer)(
                self.display,
                self.context_id,
                VA_ENC_PICTURE_PARAMETER_BUFFER_TYPE,
                std::mem::size_of::<VAEncPictureParameterBufferH264>() as u32,
                1,
                &pps as *const _ as *mut c_void,
                &mut buf_id,
            )
        };
        va_check(status, "vaCreateBuffer(PPS)")?;
        Ok(buf_id)
    }

    /// 构建 Slice 参数缓冲区
    fn create_slice_param_buffer(
        &self,
        ref_surface: VASurfaceID,
        is_keyframe: bool,
    ) -> anyhow::Result<VABufferID> {
        let width_in_mbs = (self.width + 15) / 16;
        let height_in_mbs = (self.height + 15) / 16;
        let total_mbs = width_in_mbs * height_in_mbs;

        let slice_type: u8 = if is_keyframe { 2 } else { 0 }; // 2=I, 0=P

        // 参考帧列表
        let mut ref_pic_list_0 = [VAPictureH264::default(); 32];
        if !is_keyframe {
            ref_pic_list_0[0] = VAPictureH264 {
                picture_id: ref_surface,
                frame_idx: ((self.frame_count - 1) % 256) as u32,
                flags: 0,
                top_field_order_cnt: ((self.frame_count - 1) * 2) as i32,
                bottom_field_order_cnt: ((self.frame_count - 1) * 2) as i32,
            };
        }

        let slice = VAEncSliceParameterBufferH264 {
            macroblock_address: 0,
            num_macroblocks: total_mbs,
            macroblock_info: VA_INVALID_ID,
            slice_type,
            pic_parameter_set_id: 0,
            idr_pic_id: 0,
            pic_order_cnt_lsb: ((self.frame_count * 2) % 64) as u16,
            delta_pic_order_cnt_bottom: 0,
            delta_pic_order_cnt: [0; 2],
            direct_spatial_mv_pred_flag: 0,
            num_ref_idx_active_override_flag: if is_keyframe { 0 } else { 1 },
            num_ref_idx_l0_active_minus1: 0,
            num_ref_idx_l1_active_minus1: 0,
            ref_pic_list_0,
            ref_pic_list_1: [VAPictureH264::default(); 32],
            luma_log2_weight_denom: 0,
            chroma_log2_weight_denom: 0,
            luma_weight_l0_flag: 0,
            luma_weight_l0: [0i16; 32],
            luma_offset_l0: [0i16; 32],
            chroma_weight_l0_flag: 0,
            chroma_weight_l0: [[0i16; 2]; 32],
            chroma_offset_l0: [[0i16; 2]; 32],
            luma_weight_l1_flag: 0,
            luma_weight_l1: [0i16; 32],
            luma_offset_l1: [0i16; 32],
            chroma_weight_l1_flag: 0,
            chroma_weight_l1: [[0i16; 2]; 32],
            chroma_offset_l1: [[0i16; 2]; 32],
            cabac_init_idc: 0,
            slice_qp_delta: 0,
            disable_deblocking_filter_idc: 0,
            slice_alpha_c0_offset_div2: 0,
            slice_beta_offset_div2: 0,
        };

        let mut buf_id: VABufferID = VA_INVALID_ID;
        let status = unsafe {
            (self.funcs.vaCreateBuffer)(
                self.display,
                self.context_id,
                VA_ENC_SLICE_PARAMETER_BUFFER_TYPE,
                std::mem::size_of::<VAEncSliceParameterBufferH264>() as u32,
                1,
                &slice as *const _ as *mut c_void,
                &mut buf_id,
            )
        };
        va_check(status, "vaCreateBuffer(Slice)")?;
        Ok(buf_id)
    }
}

impl VideoEncoder for VaapiEncoder {
    fn encode(&mut self, frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        if frame.data.is_empty() {
            return Ok(None);
        }

        // 1. 色彩转换 BGRA → NV12
        let nv12_data = crate::color_convert::bgra_to_nv12_alloc(
            &frame.data,
            frame.stride as usize,
            self.width as usize,
            self.height as usize,
        );

        let is_keyframe =
            self.force_keyframe_flag || self.frame_count % self.keyframe_interval == 0;
        self.force_keyframe_flag = false;

        // surface 索引：交替使用 2 个 surface
        let cur_idx = self.current_surface;
        let ref_idx = 1 - cur_idx;
        let current_surface = self.surfaces[cur_idx];
        let ref_surface = self.surfaces[ref_idx];

        // 2. 上传 NV12 数据到当前 surface
        self.upload_nv12_to_surface(current_surface, &nv12_data)?;

        // 3. 创建参数缓冲区
        let sps_buf = self.create_seq_param_buffer(is_keyframe)?;
        let pps_buf = self.create_pic_param_buffer(current_surface, ref_surface, is_keyframe)?;
        let slice_buf = self.create_slice_param_buffer(ref_surface, is_keyframe)?;

        let mut buffers = [sps_buf, pps_buf, slice_buf];

        // 4. 编码管线：BeginPicture → RenderPicture → EndPicture
        let status =
            unsafe { (self.funcs.vaBeginPicture)(self.display, self.context_id, current_surface) };
        if status != VA_STATUS_SUCCESS {
            // 清理缓冲区
            for &buf in &buffers {
                unsafe {
                    (self.funcs.vaDestroyBuffer)(self.display, buf);
                }
            }
            anyhow::bail!("VA-API: vaBeginPicture 失败，状态码: {}", status);
        }

        let status = unsafe {
            (self.funcs.vaRenderPicture)(
                self.display,
                self.context_id,
                buffers.as_mut_ptr(),
                buffers.len() as i32,
            )
        };
        if status != VA_STATUS_SUCCESS {
            // EndPicture 即使 RenderPicture 失败也应该调用以保持状态一致
            unsafe {
                (self.funcs.vaEndPicture)(self.display, self.context_id);
            }
            anyhow::bail!("VA-API: vaRenderPicture 失败，状态码: {}", status);
        }

        let status = unsafe { (self.funcs.vaEndPicture)(self.display, self.context_id) };
        va_check(status, "vaEndPicture")?;

        // 5. 同步等待编码完成
        let status = unsafe { (self.funcs.vaSyncSurface)(self.display, current_surface) };
        va_check(status, "vaSyncSurface")?;

        // 6. 读取编码后的 H.264 NAL 数据
        let mut coded_buf_ptr: *mut c_void = ptr::null_mut();
        let status =
            unsafe { (self.funcs.vaMapBuffer)(self.display, self.coded_buf, &mut coded_buf_ptr) };
        va_check(status, "vaMapBuffer(coded)")?;

        // 读取 VACodedBufferSegment 链表，收集所有编码数据段
        let nal_data = unsafe {
            let mut result = Vec::new();
            let mut segment = coded_buf_ptr as *const VACodedBufferSegment;
            while !segment.is_null() {
                let seg = &*segment;
                if seg.size > 0 && !seg.buf.is_null() {
                    let src = seg.buf as *const u8;
                    let prev_len = result.len();
                    result.resize(prev_len + seg.size as usize, 0);
                    ptr::copy_nonoverlapping(
                        src,
                        result[prev_len..].as_mut_ptr(),
                        seg.size as usize,
                    );
                }
                segment = seg.next;
            }
            result
        };

        unsafe {
            (self.funcs.vaUnmapBuffer)(self.display, self.coded_buf);
        }

        // 更新状态
        self.frame_count += 1;
        self.current_surface = 1 - self.current_surface;

        if nal_data.is_empty() {
            return Ok(None);
        }

        debug!(
            "VA-API 帧 #{}: {} bytes ({})",
            self.frame_count - 1,
            nal_data.len(),
            if is_keyframe { "IDR" } else { "P帧" }
        );

        Ok(Some(DirtyRegion {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
            encoding: FrameEncoding::H264 { is_keyframe },
            data: nal_data,
        }))
    }

    fn force_keyframe(&mut self) {
        self.force_keyframe_flag = true;
    }

    fn name(&self) -> &str {
        "VAAPI (GPU)"
    }
}

impl Drop for VaapiEncoder {
    fn drop(&mut self) {
        unsafe {
            // 按顺序释放资源：buffers → context → surfaces → config → terminate → close fd

            // 1. 销毁 coded buffer
            if self.coded_buf != VA_INVALID_ID {
                (self.funcs.vaDestroyBuffer)(self.display, self.coded_buf);
            }

            // 2. 销毁编码上下文
            if self.context_id != 0 {
                (self.funcs.vaDestroyContext)(self.display, self.context_id);
            }

            // 3. 销毁 surface
            if !self.surfaces.is_empty() {
                (self.funcs.vaDestroySurfaces)(
                    self.display,
                    self.surfaces.as_mut_ptr(),
                    self.surfaces.len() as i32,
                );
            }

            // 4. 销毁配置
            if self.config_id != 0 {
                (self.funcs.vaDestroyConfig)(self.display, self.config_id);
            }

            // 5. 终止 VA display
            if !self.display.is_null() {
                (self.funcs.vaTerminate)(self.display);
            }

            // 6. 关闭 DRM 文件描述符
            if self.drm_fd >= 0 {
                libc::close(self.drm_fd);
            }
        }
        debug!("VA-API 编码器资源已释放");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vaapi_encoder_creation() {
        // VAAPI 编码器创建取决于系统是否有 libva.so 和 GPU
        // 此测试仅验证构造函数不会 panic
        let result = VaapiEncoder::new(1920, 1080);
        // 在没有 GPU 的环境（Windows/CI）中会返回 Err，这是预期行为
        match result {
            Ok(enc) => assert_eq!(enc.name(), "VAAPI (GPU)"),
            Err(_) => {} // 没有 VAAPI 支持也是正常的
        }
    }
}
