//! Format conversion utilities for V4L2 video formats.

use image::ImageReader;
use std::io::Cursor;

/// Decode MJPEG frame to RGB buffer.
///
/// # Arguments
/// * `mjpeg_data` - Raw MJPEG frame data
/// * `rgb` - Output RGB buffer (width * height elements)
/// * `expected_width` - Expected frame width
/// * `expected_height` - Expected frame height
///
/// # Returns
/// `true` if decoding succeeded, `false` otherwise
pub fn mjpeg_to_rgb(
    mjpeg_data: &[u8],
    rgb: &mut [u32],
    expected_width: usize,
    expected_height: usize,
) -> bool {
    let cursor = Cursor::new(mjpeg_data);
    let reader = match ImageReader::new(cursor).with_guessed_format() {
        Ok(r) => r,
        Err(_) => return false,
    };

    let img = match reader.decode() {
        Ok(img) => img.to_rgb8(),
        Err(_) => return false,
    };

    let (width, height) = (img.width() as usize, img.height() as usize);

    for y in 0..height.min(expected_height) {
        for x in 0..width.min(expected_width) {
            let pixel = img.get_pixel(x as u32, y as u32);
            let rgb_idx = y * expected_width + x;
            if rgb_idx < rgb.len() {
                rgb[rgb_idx] = (pixel[0] as u32) << 16 | (pixel[1] as u32) << 8 | (pixel[2] as u32);
            }
        }
    }

    true
}

/// Convert YUYV to RGB.
///
/// # Arguments
/// * `yuyv` - Raw YUYV frame data
/// * `rgb` - Output RGB buffer (width * height elements)
/// * `width` - Frame width
/// * `height` - Frame height
pub fn yuyv_to_rgb(yuyv: &[u8], rgb: &mut [u32], width: usize, height: usize) {
    for y in 0..height {
        for x in (0..width).step_by(2) {
            let byte_offset = y * width * 2 + x * 2;
            if byte_offset + 3 >= yuyv.len() {
                break;
            }

            let y0 = yuyv[byte_offset] as i32;
            let u = yuyv[byte_offset + 1] as i32;
            let y1 = yuyv[byte_offset + 2] as i32;
            let v = yuyv[byte_offset + 3] as i32;

            let (r0, g0, b0) = yuv_to_rgb(y0, u, v);
            let rgb_idx0 = y * width + x;
            if rgb_idx0 < rgb.len() {
                rgb[rgb_idx0] = (r0 as u32) << 16 | (g0 as u32) << 8 | (b0 as u32);
            }

            if x + 1 < width {
                let (r1, g1, b1) = yuv_to_rgb(y1, u, v);
                let rgb_idx1 = y * width + x + 1;
                if rgb_idx1 < rgb.len() {
                    rgb[rgb_idx1] = (r1 as u32) << 16 | (g1 as u32) << 8 | (b1 as u32);
                }
            }
        }
    }
}

/// Convert YUV to RGB using ITU-R BT.601 conversion.
///
/// # Arguments
/// * `y` - Luma component (0-255)
/// * `u` - Chroma U component (0-255)
/// * `v` - Chroma V component (0-255)
///
/// # Returns
/// RGB tuple (r, g, b)
pub fn yuv_to_rgb(y: i32, u: i32, v: i32) -> (u8, u8, u8) {
    let c = y - 16;
    let d = u - 128;
    let e = v - 128;

    let r = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8;
    let g = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
    let b = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8;

    (r, g, b)
}
