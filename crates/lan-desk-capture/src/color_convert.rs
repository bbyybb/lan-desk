//! 共享色彩空间转换函数
//!
//! 提供 BGRA -> I420 和 BGRA -> NV12 两种转换，供 OpenH264 和 NVENC 编码器使用。
//! 使用 BT.601 定点整数系数优化性能，行级处理通过 rayon 并行化。

use rayon::prelude::*;

// BT.601 定点整数系数（左移 16 位精度）
const YR: i32 = 16843; // 0.257 * 65536
const YG: i32 = 33030; // 0.504 * 65536
const YB: i32 = 6423; // 0.098 * 65536
const UR: i32 = -9699; // -0.148 * 65536
const UG: i32 = -19071; // -0.291 * 65536
const UB: i32 = 28770; // 0.439 * 65536
const VR: i32 = 28770; // 0.439 * 65536
const VG: i32 = -24117; // -0.368 * 65536
const VB: i32 = -4653; // -0.071 * 65536

/// 对单行像素计算 Y 分量（使用 chunks_exact 优化内存访问模式）
#[inline]
fn compute_y_row(bgra: &[u8], stride: usize, y_row: &mut [u8], row: usize, w: usize) {
    let row_start = row * stride;
    let row_slice = &bgra[row_start..row_start + w * 4];
    for (pixel, y_out) in row_slice.chunks_exact(4).zip(y_row.iter_mut()) {
        let b = pixel[0] as i32;
        let g = pixel[1] as i32;
        let r = pixel[2] as i32;
        let y = ((YR * r + YG * g + YB * b + 32768) >> 16) + 16;
        *y_out = y.clamp(0, 255) as u8;
    }
}

/// BGRA -> I420 (平面 Y + U + V)
///
/// 输出布局：Y[w*h] + U[w*h/4] + V[w*h/4]
/// 用于 OpenH264 编码器。
///
/// `yuv_buf` 为可复用的输出缓冲区，调用方可在多帧之间保持同一 Vec 以避免重复分配。
pub fn bgra_to_i420(bgra: &[u8], stride: usize, w: usize, h: usize, yuv_buf: &mut Vec<u8>) {
    let y_size = w * h;
    let uv_w = w / 2;
    let uv_h = h / 2;
    let uv_size = uv_w * uv_h;
    let total = y_size + uv_size * 2;

    yuv_buf.clear();
    yuv_buf.resize(total, 0u8);

    let (y_plane, uv_planes) = yuv_buf.split_at_mut(y_size);
    let (u_plane, v_plane) = uv_planes.split_at_mut(uv_size);

    // 第一阶段：并行计算全部 Y 分量
    y_plane
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(row, y_row)| {
            compute_y_row(bgra, stride, y_row, row, w);
        });

    // 第二阶段：并行计算 UV 分量（每两行一组，使用 chunks_exact 优化像素访问）
    let uv_pairs: Vec<(&mut [u8], &mut [u8])> = u_plane
        .chunks_mut(uv_w)
        .zip(v_plane.chunks_mut(uv_w))
        .collect();

    uv_pairs
        .into_par_iter()
        .enumerate()
        .for_each(|(uv_row, (u_row, v_row))| {
            let row = uv_row * 2;
            let row_start = row * stride;
            let row_slice = &bgra[row_start..row_start + w * 4];
            // 每隔 2 个像素（8 字节）取一个做 UV 采样
            for (col_half, (u_out, v_out)) in u_row.iter_mut().zip(v_row.iter_mut()).enumerate() {
                let px = &row_slice[col_half * 8..col_half * 8 + 4];
                let b = px[0] as i32;
                let g = px[1] as i32;
                let r = px[2] as i32;
                let u = ((UR * r + UG * g + UB * b + 32768) >> 16) + 128;
                let v = ((VR * r + VG * g + VB * b + 32768) >> 16) + 128;
                *u_out = u.clamp(0, 255) as u8;
                *v_out = v.clamp(0, 255) as u8;
            }
        });
}

/// BGRA -> I420 便捷版本（内部分配缓冲区，兼容旧接口）
pub fn bgra_to_i420_alloc(bgra: &[u8], stride: usize, w: usize, h: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    bgra_to_i420(bgra, stride, w, h, &mut buf);
    buf
}

/// BGRA -> NV12 (平面 Y + 交错 UV)
///
/// 输出布局：Y[w*h] + UV[w*h/2]（UV 交错排列：U0 V0 U1 V1 ...）
/// 用于 NVENC GPU 编码器。
///
/// `nv12_buf` 为可复用的输出缓冲区，调用方可在多帧之间保持同一 Vec 以避免重复分配。
pub fn bgra_to_nv12(bgra: &[u8], stride: usize, w: usize, h: usize, nv12_buf: &mut Vec<u8>) {
    let y_size = w * h;
    let uv_size = w * (h / 2); // 交错 UV，宽度不减半

    nv12_buf.clear();
    nv12_buf.resize(y_size + uv_size, 0u8);

    let (y_plane, uv_plane) = nv12_buf.split_at_mut(y_size);

    // 第一阶段：并行计算全部 Y 分量
    y_plane
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(row, y_row)| {
            compute_y_row(bgra, stride, y_row, row, w);
        });

    // 第二阶段：并行计算交错 UV 分量（每两行一组，每组 w 字节，使用 chunks_exact 优化）
    uv_plane
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(uv_row, uv_chunk)| {
            let row = uv_row * 2;
            let half_w = w / 2;
            let row_start = row * stride;
            let row_slice = &bgra[row_start..row_start + w * 4];
            for (col_half, uv_pair) in uv_chunk.chunks_exact_mut(2).take(half_w).enumerate() {
                let px = &row_slice[col_half * 8..col_half * 8 + 4];
                let b = px[0] as i32;
                let g = px[1] as i32;
                let r = px[2] as i32;
                let u = ((UR * r + UG * g + UB * b + 32768) >> 16) + 128;
                let v = ((VR * r + VG * g + VB * b + 32768) >> 16) + 128;
                uv_pair[0] = u.clamp(0, 255) as u8;
                uv_pair[1] = v.clamp(0, 255) as u8;
            }
        });
}

/// BGRA -> NV12 便捷版本（内部分配缓冲区，兼容旧接口）
pub fn bgra_to_nv12_alloc(bgra: &[u8], stride: usize, w: usize, h: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    bgra_to_nv12(bgra, stride, w, h, &mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 使用 BT.601 公式手算已知 BGRA 输入的 Y 值，验证 compute_y_row 正确性
    #[test]
    fn test_compute_y_row_known_values() {
        // 纯白像素 BGRA = [255, 255, 255, 255]
        // Y = ((YR*255 + YG*255 + YB*255 + 32768) >> 16) + 16
        //   = ((16843 + 33030 + 6423) * 255 + 32768) >> 16 + 16
        //   = (56296 * 255 + 32768) >> 16 + 16
        //   = (14355480 + 32768) >> 16 + 16
        //   = 14388248 >> 16 + 16
        //   = 219 + 16 = 235
        let white_bgra = [255u8, 255, 255, 255];
        let mut y_out = [0u8; 1];
        compute_y_row(&white_bgra, 4, &mut y_out, 0, 1);
        assert_eq!(y_out[0], 235);

        // 纯黑像素 BGRA = [0, 0, 0, 255]
        // Y = ((0 + 0 + 0 + 32768) >> 16) + 16 = 0 + 16 = 16
        let black_bgra = [0u8, 0, 0, 255];
        let mut y_out = [0u8; 1];
        compute_y_row(&black_bgra, 4, &mut y_out, 0, 1);
        assert_eq!(y_out[0], 16);

        // 纯红像素 BGRA = [0, 0, 255, 255] (B=0, G=0, R=255)
        // Y = ((YR*255 + 0 + 0 + 32768) >> 16) + 16
        //   = ((16843*255 + 32768) >> 16) + 16
        //   = ((4294965 + 32768) >> 16) + 16
        //   = (4327733 >> 16) + 16
        //   = 66 + 16 = 82
        let red_bgra = [0u8, 0, 255, 255];
        let mut y_out = [0u8; 1];
        compute_y_row(&red_bgra, 4, &mut y_out, 0, 1);
        assert_eq!(y_out[0], 82);

        // 纯绿像素 BGRA = [0, 255, 0, 255] (B=0, G=255, R=0)
        // Y = ((0 + YG*255 + 0 + 32768) >> 16) + 16
        //   = ((33030*255 + 32768) >> 16) + 16
        //   = ((8422650 + 32768) >> 16) + 16
        //   = (8455418 >> 16) + 16
        //   = 129 + 16 = 145
        let green_bgra = [0u8, 255, 0, 255];
        let mut y_out = [0u8; 1];
        compute_y_row(&green_bgra, 4, &mut y_out, 0, 1);
        assert_eq!(y_out[0], 145);
    }

    /// 2x2 纯色 BGRA 转 I420，验证 Y/U/V 平面尺寸和值范围
    #[test]
    fn test_bgra_to_i420_basic() {
        let w = 2;
        let h = 2;
        let stride = w * 4;
        // 2x2 纯白像素
        let bgra = vec![255u8; w * h * 4];

        let mut yuv_buf = Vec::new();
        bgra_to_i420(&bgra, stride, w, h, &mut yuv_buf);

        let y_size = w * h; // 4
        let uv_w = w / 2; // 1
        let uv_h = h / 2; // 1
        let uv_size = uv_w * uv_h; // 1
        let total = y_size + uv_size * 2; // 4 + 1 + 1 = 6

        assert_eq!(yuv_buf.len(), total);

        // 验证 Y 平面值
        let y_plane = &yuv_buf[..y_size];
        for &y in y_plane {
            assert_eq!(y, 235, "纯白像素的 Y 值应为 235");
        }

        // 验证 U/V 平面大小正确
        let u_plane = &yuv_buf[y_size..y_size + uv_size];
        assert_eq!(u_plane.len(), uv_size, "U 平面大小应为 {}", uv_size);

        let v_plane = &yuv_buf[y_size + uv_size..];
        assert_eq!(v_plane.len(), uv_size, "V 平面大小应为 {}", uv_size);

        // 纯白像素 U 和 V 应接近 128（中性色度）
        assert!(
            (u_plane[0] as i32 - 128).unsigned_abs() <= 2,
            "纯白像素 U 值应接近 128，实际为 {}",
            u_plane[0]
        );
        assert!(
            (v_plane[0] as i32 - 128).unsigned_abs() <= 2,
            "纯白像素 V 值应接近 128，实际为 {}",
            v_plane[0]
        );
    }

    /// 调用两次 bgra_to_i420 传入同一个 yuv_buf，验证第二次不重新分配（capacity 不变）
    #[test]
    fn test_bgra_to_i420_buffer_reuse() {
        let w = 4;
        let h = 4;
        let stride = w * 4;
        let bgra = vec![128u8; w * h * 4];

        let mut yuv_buf = Vec::new();

        // 第一次调用
        bgra_to_i420(&bgra, stride, w, h, &mut yuv_buf);
        let cap_after_first = yuv_buf.capacity();
        let expected_total = w * h + (w / 2) * (h / 2) * 2;
        assert_eq!(yuv_buf.len(), expected_total);

        // 第二次调用（相同尺寸）
        bgra_to_i420(&bgra, stride, w, h, &mut yuv_buf);
        let cap_after_second = yuv_buf.capacity();

        // capacity 不应增长（复用了已有缓冲区）
        assert_eq!(
            cap_after_first, cap_after_second,
            "第二次调用不应重新分配缓冲区：第一次 capacity={}，第二次 capacity={}",
            cap_after_first, cap_after_second
        );
    }

    /// 2x2 纯色 BGRA 转 NV12，验证输出尺寸正确（w*h + w*h/2）
    #[test]
    fn test_bgra_to_nv12_basic() {
        let w = 2;
        let h = 2;
        let stride = w * 4;
        // 2x2 纯蓝像素 BGRA = [255, 0, 0, 255]
        let bgra: Vec<u8> = [255u8, 0, 0, 255]
            .iter()
            .copied()
            .cycle()
            .take(w * h * 4)
            .collect();

        let nv12 = bgra_to_nv12_alloc(&bgra, stride, w, h);

        // NV12 总大小 = w*h + w*(h/2) = w*h + w*h/2
        let expected_size = w * h + w * (h / 2);
        assert_eq!(
            nv12.len(),
            expected_size,
            "NV12 输出大小应为 w*h + w*h/2 = {}，实际为 {}",
            expected_size,
            nv12.len()
        );

        // 验证 Y 平面值在有效范围内（16..=235 为标准范围）
        let y_plane = &nv12[..w * h];
        for &y in y_plane {
            assert!(
                y >= 16 && y <= 235,
                "Y 值应在 [16, 235] 范围内，实际为 {}",
                y
            );
        }

        // 验证 UV 交错部分长度正确
        let uv_plane = &nv12[w * h..];
        assert_eq!(uv_plane.len(), w * (h / 2));
    }
}
