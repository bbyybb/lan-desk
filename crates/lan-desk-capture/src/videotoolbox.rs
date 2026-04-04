//! macOS VideoToolbox H.264 GPU 硬件编码器
//!
//! 使用 Apple VideoToolbox 框架进行硬件加速 H.264 编码。
//! 支持 Apple Silicon (M1/M2/M3/M4) 和 Intel Mac 的硬件编码器。
//!
//! 编码流程：
//! 1. 创建 VTCompressionSession
//! 2. BGRA 帧数据 -> CVPixelBuffer
//! 3. VTCompressionSessionEncodeFrame 异步编码
//! 4. 输出回调中提取 CMSampleBuffer -> H.264 NAL 数据
//! 5. AVCC -> Annex-B 格式转换（4字节长度前缀 -> [00 00 00 01] start code）

use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc;

use tracing::{debug, info, warn};

use crate::frame::CapturedFrame;
use crate::gpu_encoder::VideoEncoder;
use lan_desk_protocol::message::{DirtyRegion, FrameEncoding};

// ─── CoreFoundation / CoreMedia / CoreVideo / VideoToolbox FFI ───

// CoreFoundation 类型
type CFAllocatorRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFMutableDictionaryRef = *mut c_void;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFNumberRef = *const c_void;
type CFBooleanRef = *const c_void;

// CoreMedia 类型
type CMSampleBufferRef = *const c_void;
type CMFormatDescriptionRef = *const c_void;
type CMBlockBufferRef = *const c_void;

// CoreVideo 类型
type CVPixelBufferRef = *mut c_void;
type CVReturn = i32;

// VideoToolbox 类型
type VTCompressionSessionRef = *mut c_void;

// OSStatus
type OSStatus = i32;

// CMTime 结构体
#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

impl CMTime {
    fn make(value: i64, timescale: i32) -> Self {
        CMTime {
            value,
            timescale,
            flags: 1, // kCMTimeFlags_Valid
            epoch: 0,
        }
    }

    fn invalid() -> Self {
        CMTime {
            value: 0,
            timescale: 0,
            flags: 0,
            epoch: 0,
        }
    }
}

// CFNumber 类型常量
const K_CF_NUMBER_SINT32_TYPE: i64 = 3;

// CVPixelBuffer 格式常量
const K_CV_PIXEL_FORMAT_TYPE_32BGRA: u32 = 0x42475241; // 'BGRA'

// 输出回调数据，用于在回调中将编码结果传回主线程
struct CallbackContext {
    tx: mpsc::Sender<CallbackResult>,
}

struct CallbackResult {
    data: Vec<u8>,
    is_keyframe: bool,
}

// ─── CoreFoundation / CoreMedia / CoreVideo / VideoToolbox 外部函数声明 ───

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    static kCFBooleanTrue: CFBooleanRef;
    static kCFBooleanFalse: CFBooleanRef;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;

    fn CFDictionaryCreateMutable(
        allocator: CFAllocatorRef,
        capacity: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFMutableDictionaryRef;

    fn CFDictionarySetValue(dict: CFMutableDictionaryRef, key: *const c_void, value: *const c_void);

    fn CFNumberCreate(
        allocator: CFAllocatorRef,
        the_type: i64,
        value_ptr: *const c_void,
    ) -> CFNumberRef;

    fn CFRelease(cf: CFTypeRef);
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetDataBuffer(sbuf: CMSampleBufferRef) -> CMBlockBufferRef;

    fn CMSampleBufferGetFormatDescription(sbuf: CMSampleBufferRef) -> CMFormatDescriptionRef;

    fn CMBlockBufferGetDataPointer(
        buf: CMBlockBufferRef,
        offset: usize,
        length_at_offset: *mut usize,
        total_length: *mut usize,
        data_pointer: *mut *mut u8,
    ) -> OSStatus;

    fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        format_desc: CMFormatDescriptionRef,
        parameter_set_index: usize,
        parameter_set_pointer: *mut *const u8,
        parameter_set_size: *mut usize,
        parameter_set_count: *mut usize,
        nal_unit_header_length: *mut i32,
    ) -> OSStatus;

    fn CMSampleBufferIsValid(sbuf: CMSampleBufferRef) -> u8;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    static kCVPixelBufferWidthKey: CFStringRef;
    static kCVPixelBufferHeightKey: CFStringRef;
    static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;

    fn CVPixelBufferCreateWithBytes(
        allocator: CFAllocatorRef,
        width: usize,
        height: usize,
        pixel_format_type: u32,
        base_address: *mut c_void,
        bytes_per_row: usize,
        release_callback: *const c_void,
        release_ref_con: *mut c_void,
        pixel_buffer_attributes: CFDictionaryRef,
        pixel_buffer_out: *mut CVPixelBufferRef,
    ) -> CVReturn;

    fn CVPixelBufferRelease(pixel_buffer: CVPixelBufferRef);
}

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    static kVTCompressionPropertyKey_RealTime: CFStringRef;
    static kVTCompressionPropertyKey_ProfileLevel: CFStringRef;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: CFStringRef;
    static kVTCompressionPropertyKey_AverageBitRate: CFStringRef;
    static kVTCompressionPropertyKey_AllowFrameReordering: CFStringRef;
    static kVTProfileLevel_H264_Baseline_AutoLevel: CFStringRef;
    static kVTEncodeFrameOptionKey_ForceKeyFrame: CFStringRef;

    fn VTCompressionSessionCreate(
        allocator: CFAllocatorRef,
        width: i32,
        height: i32,
        codec_type: u32,
        encoder_specification: CFDictionaryRef,
        source_image_buffer_attributes: CFDictionaryRef,
        compressed_data_allocator: CFAllocatorRef,
        output_callback: *const c_void,
        output_callback_ref_con: *mut c_void,
        compression_session_out: *mut VTCompressionSessionRef,
    ) -> OSStatus;

    fn VTSessionSetProperty(
        session: VTCompressionSessionRef,
        property_key: CFStringRef,
        property_value: CFTypeRef,
    ) -> OSStatus;

    fn VTCompressionSessionPrepareToEncodeFrames(session: VTCompressionSessionRef) -> OSStatus;

    fn VTCompressionSessionEncodeFrame(
        session: VTCompressionSessionRef,
        image_buffer: CVPixelBufferRef,
        presentation_time_stamp: CMTime,
        duration: CMTime,
        frame_properties: CFDictionaryRef,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> OSStatus;

    fn VTCompressionSessionCompleteFrames(
        session: VTCompressionSessionRef,
        complete_until_presentation_time_stamp: CMTime,
    ) -> OSStatus;

    fn VTCompressionSessionInvalidate(session: VTCompressionSessionRef);
}

// CMVideoCodecType
const K_CM_VIDEO_CODEC_TYPE_H264: u32 = 0x61766331; // 'avc1'

// ─── VideoToolbox 编码器实现 ───

/// VideoToolbox H.264 编码器
pub struct VTEncoder {
    width: u32,
    height: u32,
    frame_count: u64,
    force_keyframe_flag: bool,
    session: VTCompressionSessionRef,
    /// 回调上下文，通过 Box 持有以保证指针稳定
    _callback_ctx: Box<CallbackContext>,
    /// 接收编码输出
    output_rx: mpsc::Receiver<CallbackResult>,
}

unsafe impl Send for VTEncoder {}

impl VTEncoder {
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let callback_ctx = Box::new(CallbackContext { tx });
        let ctx_ptr = &*callback_ctx as *const CallbackContext as *mut c_void;

        let mut session: VTCompressionSessionRef = ptr::null_mut();

        // 创建 VTCompressionSession
        let status = unsafe {
            VTCompressionSessionCreate(
                kCFAllocatorDefault,
                width as i32,
                height as i32,
                K_CM_VIDEO_CODEC_TYPE_H264,
                ptr::null(),         // encoderSpecification
                ptr::null(),         // sourceImageBufferAttributes
                kCFAllocatorDefault, // compressedDataAllocator
                vt_output_callback as *const c_void,
                ctx_ptr,
                &mut session,
            )
        };

        if status != 0 || session.is_null() {
            anyhow::bail!(
                "VTCompressionSessionCreate 失败，状态码: {} (硬件编码器不可用)",
                status
            );
        }

        // 设置编码属性
        unsafe {
            // 实时编码模式
            let s = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_RealTime,
                kCFBooleanTrue as CFTypeRef,
            );
            if s != 0 {
                warn!("设置 RealTime 属性失败: {}", s);
            }

            // H.264 Baseline Profile
            let s = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_ProfileLevel,
                kVTProfileLevel_H264_Baseline_AutoLevel as CFTypeRef,
            );
            if s != 0 {
                warn!("设置 ProfileLevel 属性失败: {}", s);
            }

            // 关键帧间隔 60 帧
            let interval: i32 = 60;
            let cf_interval = CFNumberCreate(
                kCFAllocatorDefault,
                K_CF_NUMBER_SINT32_TYPE,
                &interval as *const i32 as *const c_void,
            );
            let s = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_MaxKeyFrameInterval,
                cf_interval as CFTypeRef,
            );
            CFRelease(cf_interval as CFTypeRef);
            if s != 0 {
                warn!("设置 MaxKeyFrameInterval 属性失败: {}", s);
            }

            // 平均码率 4 Mbps
            let bitrate: i32 = 4_000_000;
            let cf_bitrate = CFNumberCreate(
                kCFAllocatorDefault,
                K_CF_NUMBER_SINT32_TYPE,
                &bitrate as *const i32 as *const c_void,
            );
            let s = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_AverageBitRate,
                cf_bitrate as CFTypeRef,
            );
            CFRelease(cf_bitrate as CFTypeRef);
            if s != 0 {
                warn!("设置 AverageBitRate 属性失败: {}", s);
            }

            // 禁止帧重排序（低延迟）
            let s = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_AllowFrameReordering,
                kCFBooleanFalse as CFTypeRef,
            );
            if s != 0 {
                warn!("设置 AllowFrameReordering 属性失败: {}", s);
            }

            // 准备编码
            let s = VTCompressionSessionPrepareToEncodeFrames(session);
            if s != 0 {
                anyhow::bail!("VTCompressionSessionPrepareToEncodeFrames 失败: {}", s);
            }
        }

        info!("VideoToolbox 编码器初始化: {}x{}", width, height);

        let mut encoder = Self {
            width,
            height,
            frame_count: 0,
            force_keyframe_flag: false,
            session,
            _callback_ctx: callback_ctx,
            output_rx: rx,
        };

        // 测试帧验证：编码一个全黑帧确保编码管线可用
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
                info!("VideoToolbox 测试帧编码成功");
                encoder.frame_count = 0;
                Ok(encoder)
            }
            Ok(None) => {
                anyhow::bail!("VideoToolbox 测试帧编码无输出")
            }
            Err(e) => {
                anyhow::bail!("VideoToolbox 测试帧编码失败: {}", e)
            }
        }
    }
}

impl VideoEncoder for VTEncoder {
    fn encode(&mut self, frame: &CapturedFrame) -> anyhow::Result<Option<DirtyRegion>> {
        if frame.data.is_empty() {
            return Ok(None);
        }

        let is_keyframe = self.force_keyframe_flag || self.frame_count % 60 == 0;
        self.force_keyframe_flag = false;

        // 从 BGRA 帧数据创建 CVPixelBuffer
        let mut pixel_buffer: CVPixelBufferRef = ptr::null_mut();
        let mut frame_data = frame.data.clone();

        let cv_status = unsafe {
            CVPixelBufferCreateWithBytes(
                kCFAllocatorDefault,
                self.width as usize,
                self.height as usize,
                K_CV_PIXEL_FORMAT_TYPE_32BGRA,
                frame_data.as_mut_ptr() as *mut c_void,
                frame.stride as usize,
                ptr::null(), // releaseCallback（我们自己管理内存）
                ptr::null_mut(),
                ptr::null(), // pixelBufferAttributes
                &mut pixel_buffer,
            )
        };

        if cv_status != 0 || pixel_buffer.is_null() {
            anyhow::bail!("CVPixelBufferCreateWithBytes 失败，状态码: {}", cv_status);
        }

        // 构造帧属性（强制关键帧时设置）
        let frame_props = if is_keyframe {
            unsafe {
                let dict = CFDictionaryCreateMutable(
                    kCFAllocatorDefault,
                    1,
                    &kCFTypeDictionaryKeyCallBacks,
                    &kCFTypeDictionaryValueCallBacks,
                );
                CFDictionarySetValue(
                    dict,
                    kVTEncodeFrameOptionKey_ForceKeyFrame as *const c_void,
                    kCFBooleanTrue as *const c_void,
                );
                dict as CFDictionaryRef
            }
        } else {
            ptr::null()
        };

        // PTS 基于帧计数，时间基 30fps
        let pts = CMTime::make(self.frame_count as i64, 30);
        let duration = CMTime::make(1, 30);

        // 编码帧
        let status = unsafe {
            VTCompressionSessionEncodeFrame(
                self.session,
                pixel_buffer,
                pts,
                duration,
                frame_props,
                ptr::null_mut(), // sourceFrameRefCon
                ptr::null_mut(), // infoFlagsOut
            )
        };

        // 释放帧属性字典
        if !frame_props.is_null() {
            unsafe { CFRelease(frame_props as CFTypeRef) };
        }

        // 释放 CVPixelBuffer
        unsafe { CVPixelBufferRelease(pixel_buffer) };

        if status != 0 {
            anyhow::bail!("VTCompressionSessionEncodeFrame 失败，状态码: {}", status);
        }

        // 强制输出所有已编码的帧
        let status = unsafe { VTCompressionSessionCompleteFrames(self.session, CMTime::invalid()) };
        if status != 0 {
            anyhow::bail!(
                "VTCompressionSessionCompleteFrames 失败，状态码: {}",
                status
            );
        }

        self.frame_count += 1;

        // 从回调通道接收编码结果
        match self.output_rx.try_recv() {
            Ok(result) => {
                if result.data.is_empty() {
                    return Ok(None);
                }

                debug!(
                    "VideoToolbox 帧 #{}: {} bytes ({})",
                    self.frame_count - 1,
                    result.data.len(),
                    if result.is_keyframe { "IDR" } else { "P帧" }
                );

                Ok(Some(DirtyRegion {
                    x: 0,
                    y: 0,
                    width: self.width,
                    height: self.height,
                    encoding: FrameEncoding::H264 {
                        is_keyframe: result.is_keyframe,
                    },
                    data: result.data,
                }))
            }
            Err(mpsc::TryRecvError::Empty) => {
                // 编码器可能还没有输出（例如首帧延迟）
                Ok(None)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                anyhow::bail!("VideoToolbox 输出通道已断开")
            }
        }
    }

    fn force_keyframe(&mut self) {
        self.force_keyframe_flag = true;
    }

    fn name(&self) -> &str {
        "VideoToolbox (GPU)"
    }
}

impl Drop for VTEncoder {
    fn drop(&mut self) {
        if !self.session.is_null() {
            unsafe {
                // 等待所有帧完成编码
                let _ = VTCompressionSessionCompleteFrames(self.session, CMTime::invalid());
                // 使会话无效化
                VTCompressionSessionInvalidate(self.session);
                // 释放会话
                CFRelease(self.session as CFTypeRef);
            }
        }
        debug!("VideoToolbox 编码器资源已释放");
    }
}

// ─── VideoToolbox 输出回调 ───

/// VTCompressionSession 的异步输出回调
///
/// 当 VideoToolbox 完成一帧编码后调用此函数。
/// 从 CMSampleBuffer 中提取 H.264 NAL 数据并转换为 Annex-B 格式。
extern "C" fn vt_output_callback(
    output_callback_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: OSStatus,
    _info_flags: u32,
    sample_buffer: CMSampleBufferRef,
) {
    // 安全检查
    if status != 0 || sample_buffer.is_null() {
        warn!("VideoToolbox 输出回调收到错误状态: {}", status);
        return;
    }

    let ctx = unsafe { &*(output_callback_ref_con as *const CallbackContext) };

    // 检查是否为关键帧（通过检查 SPS/PPS 参数集的存在）
    let format_desc = unsafe { CMSampleBufferGetFormatDescription(sample_buffer) };
    let is_keyframe = if !format_desc.is_null() {
        let mut param_count: usize = 0;
        let status = unsafe {
            CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                format_desc,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut param_count,
                ptr::null_mut(),
            )
        };
        status == 0 && param_count > 0
    } else {
        false
    };

    let mut nal_output: Vec<u8> = Vec::new();

    // 关键帧：先提取 SPS 和 PPS 参数集
    if is_keyframe && !format_desc.is_null() {
        // Annex-B start code
        let start_code: [u8; 4] = [0x00, 0x00, 0x00, 0x01];

        let mut param_count: usize = 0;
        let _ = unsafe {
            CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                format_desc,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut param_count,
                ptr::null_mut(),
            )
        };

        // 提取所有参数集（通常 index 0 = SPS, index 1 = PPS）
        for i in 0..param_count {
            let mut param_ptr: *const u8 = ptr::null();
            let mut param_size: usize = 0;

            let status = unsafe {
                CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                    format_desc,
                    i,
                    &mut param_ptr,
                    &mut param_size,
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };

            if status == 0 && !param_ptr.is_null() && param_size > 0 {
                nal_output.extend_from_slice(&start_code);
                let param_data = unsafe { std::slice::from_raw_parts(param_ptr, param_size) };
                nal_output.extend_from_slice(param_data);
            }
        }
    }

    // 提取编码后的帧数据（AVCC 格式 -> Annex-B 格式）
    let data_buffer = unsafe { CMSampleBufferGetDataBuffer(sample_buffer) };
    if data_buffer.is_null() {
        warn!("VideoToolbox 输出 CMBlockBuffer 为空");
        return;
    }

    let mut total_length: usize = 0;
    let mut data_ptr: *mut u8 = ptr::null_mut();

    let status = unsafe {
        CMBlockBufferGetDataPointer(
            data_buffer,
            0,
            ptr::null_mut(),
            &mut total_length,
            &mut data_ptr,
        )
    };

    if status != 0 || data_ptr.is_null() || total_length == 0 {
        warn!("CMBlockBufferGetDataPointer 失败: {}", status);
        return;
    }

    // AVCC -> Annex-B 转换
    // AVCC 格式：[4字节长度][NAL数据][4字节长度][NAL数据]...
    // Annex-B 格式：[00 00 00 01][NAL数据][00 00 00 01][NAL数据]...
    let start_code: [u8; 4] = [0x00, 0x00, 0x00, 0x01];
    let raw_data = unsafe { std::slice::from_raw_parts(data_ptr, total_length) };

    let mut offset = 0;
    while offset + 4 <= total_length {
        // 读取 4 字节大端长度
        let nal_length = u32::from_be_bytes([
            raw_data[offset],
            raw_data[offset + 1],
            raw_data[offset + 2],
            raw_data[offset + 3],
        ]) as usize;

        offset += 4;

        if offset + nal_length > total_length {
            warn!(
                "AVCC NAL 长度越界: offset={}, nal_length={}, total={}",
                offset, nal_length, total_length
            );
            break;
        }

        nal_output.extend_from_slice(&start_code);
        nal_output.extend_from_slice(&raw_data[offset..offset + nal_length]);
        offset += nal_length;
    }

    // 通过通道将结果发送回编码器
    let _ = ctx.tx.send(CallbackResult {
        data: nal_output,
        is_keyframe,
    });
}
