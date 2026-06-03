//! `<linux/videodev2.h>` (pixel format subset) — V4L2 pixel format codes.
//!
//! V4L2 pixel formats are FourCC codes that describe how pixel data is
//! laid out in memory. The format determines the colour model (RGB,
//! YUV, Bayer), bit depth, plane count, and component ordering.
//! Drivers advertise supported formats via `VIDIOC_ENUM_FMT` and
//! applications select one via `VIDIOC_S_FMT`.

// ---------------------------------------------------------------------------
// Helper: v4l2_fourcc equivalent
// ---------------------------------------------------------------------------

/// Build a V4L2 FourCC code from four ASCII bytes.
const fn v4l2_fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

// ---------------------------------------------------------------------------
// RGB formats
// ---------------------------------------------------------------------------

/// 16-bit RGB 5-6-5.
pub const V4L2_PIX_FMT_RGB565: u32 = v4l2_fourcc(b'R', b'G', b'B', b'P');
/// 24-bit RGB 8-8-8.
pub const V4L2_PIX_FMT_RGB24: u32 = v4l2_fourcc(b'R', b'G', b'B', b'3');
/// 24-bit BGR 8-8-8.
pub const V4L2_PIX_FMT_BGR24: u32 = v4l2_fourcc(b'B', b'G', b'R', b'3');
/// 32-bit ARGB 8-8-8-8.
pub const V4L2_PIX_FMT_ARGB32: u32 = v4l2_fourcc(b'B', b'A', b'2', b'4');
/// 32-bit XRGB 8-8-8-8 (alpha ignored).
pub const V4L2_PIX_FMT_XRGB32: u32 = v4l2_fourcc(b'B', b'X', b'2', b'4');
/// 32-bit ABGR 8-8-8-8.
pub const V4L2_PIX_FMT_ABGR32: u32 = v4l2_fourcc(b'A', b'R', b'2', b'4');

// ---------------------------------------------------------------------------
// YUV packed formats
// ---------------------------------------------------------------------------

/// YUYV 4:2:2 (packed, 16 bpp).
pub const V4L2_PIX_FMT_YUYV: u32 = v4l2_fourcc(b'Y', b'U', b'Y', b'V');
/// UYVY 4:2:2 (packed, 16 bpp).
pub const V4L2_PIX_FMT_UYVY: u32 = v4l2_fourcc(b'U', b'Y', b'V', b'Y');
/// YVYU 4:2:2 (packed, 16 bpp).
pub const V4L2_PIX_FMT_YVYU: u32 = v4l2_fourcc(b'Y', b'V', b'Y', b'U');
/// VYUY 4:2:2 (packed, 16 bpp).
pub const V4L2_PIX_FMT_VYUY: u32 = v4l2_fourcc(b'V', b'Y', b'U', b'Y');

// ---------------------------------------------------------------------------
// YUV planar formats
// ---------------------------------------------------------------------------

/// NV12: Y plane + interleaved UV plane (4:2:0).
pub const V4L2_PIX_FMT_NV12: u32 = v4l2_fourcc(b'N', b'V', b'1', b'2');
/// NV21: Y plane + interleaved VU plane (4:2:0).
pub const V4L2_PIX_FMT_NV21: u32 = v4l2_fourcc(b'N', b'V', b'2', b'1');
/// YUV420: three separate Y, U, V planes (4:2:0).
pub const V4L2_PIX_FMT_YUV420: u32 = v4l2_fourcc(b'Y', b'U', b'1', b'2');
/// YVU420: three separate Y, V, U planes (4:2:0).
pub const V4L2_PIX_FMT_YVU420: u32 = v4l2_fourcc(b'Y', b'V', b'1', b'2');
/// YUV422P: three separate Y, U, V planes (4:2:2).
pub const V4L2_PIX_FMT_YUV422P: u32 = v4l2_fourcc(b'4', b'2', b'2', b'P');

// ---------------------------------------------------------------------------
// Compressed formats
// ---------------------------------------------------------------------------

/// MJPEG compressed.
pub const V4L2_PIX_FMT_MJPEG: u32 = v4l2_fourcc(b'M', b'J', b'P', b'G');
/// JPEG compressed (JFIF).
pub const V4L2_PIX_FMT_JPEG: u32 = v4l2_fourcc(b'J', b'P', b'E', b'G');
/// H.264 compressed.
pub const V4L2_PIX_FMT_H264: u32 = v4l2_fourcc(b'H', b'2', b'6', b'4');
/// HEVC / H.265 compressed.
pub const V4L2_PIX_FMT_HEVC: u32 = v4l2_fourcc(b'H', b'E', b'V', b'C');
/// VP8 compressed.
pub const V4L2_PIX_FMT_VP8: u32 = v4l2_fourcc(b'V', b'P', b'8', b'0');
/// VP9 compressed.
pub const V4L2_PIX_FMT_VP9: u32 = v4l2_fourcc(b'V', b'P', b'9', b'0');

// ---------------------------------------------------------------------------
// Greyscale
// ---------------------------------------------------------------------------

/// 8-bit greyscale.
pub const V4L2_PIX_FMT_GREY: u32 = v4l2_fourcc(b'G', b'R', b'E', b'Y');
/// 16-bit greyscale.
pub const V4L2_PIX_FMT_Y16: u32 = v4l2_fourcc(b'Y', b'1', b'6', b' ');

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fourcc_encoding() {
        // YUYV should be 'Y' | ('U' << 8) | ('Y' << 16) | ('V' << 24)
        let expected =
            b'Y' as u32 | ((b'U' as u32) << 8) | ((b'Y' as u32) << 16) | ((b'V' as u32) << 24);
        assert_eq!(V4L2_PIX_FMT_YUYV, expected);
    }

    #[test]
    fn test_rgb_formats_distinct() {
        let fmts = [
            V4L2_PIX_FMT_RGB565,
            V4L2_PIX_FMT_RGB24,
            V4L2_PIX_FMT_BGR24,
            V4L2_PIX_FMT_ARGB32,
            V4L2_PIX_FMT_XRGB32,
            V4L2_PIX_FMT_ABGR32,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_yuv_formats_distinct() {
        let fmts = [
            V4L2_PIX_FMT_YUYV,
            V4L2_PIX_FMT_UYVY,
            V4L2_PIX_FMT_YVYU,
            V4L2_PIX_FMT_VYUY,
            V4L2_PIX_FMT_NV12,
            V4L2_PIX_FMT_NV21,
            V4L2_PIX_FMT_YUV420,
            V4L2_PIX_FMT_YVU420,
            V4L2_PIX_FMT_YUV422P,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_compressed_formats_distinct() {
        let fmts = [
            V4L2_PIX_FMT_MJPEG,
            V4L2_PIX_FMT_JPEG,
            V4L2_PIX_FMT_H264,
            V4L2_PIX_FMT_HEVC,
            V4L2_PIX_FMT_VP8,
            V4L2_PIX_FMT_VP9,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_grey_formats_distinct() {
        assert_ne!(V4L2_PIX_FMT_GREY, V4L2_PIX_FMT_Y16);
    }

    #[test]
    fn test_all_nonzero() {
        // FourCC codes built from ASCII always produce nonzero values
        let fmts = [
            V4L2_PIX_FMT_RGB565,
            V4L2_PIX_FMT_YUYV,
            V4L2_PIX_FMT_NV12,
            V4L2_PIX_FMT_MJPEG,
            V4L2_PIX_FMT_GREY,
        ];
        for &f in &fmts {
            assert_ne!(f, 0);
        }
    }
}
