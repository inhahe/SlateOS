//! `<linux/videodev2.h>` — Video4Linux2 API constants.
//!
//! V4L2 is the standard Linux API for video capture (webcams, TV tuners),
//! video output, and codec devices. Used by applications like VLC, OBS,
//! FFmpeg, and GStreamer.

// ---------------------------------------------------------------------------
// V4L2 buffer types
// ---------------------------------------------------------------------------

/// Video capture buffer.
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
/// Video output buffer.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
/// Video overlay buffer.
pub const V4L2_BUF_TYPE_VIDEO_OVERLAY: u32 = 3;
/// VBI capture.
pub const V4L2_BUF_TYPE_VBI_CAPTURE: u32 = 4;
/// VBI output.
pub const V4L2_BUF_TYPE_VBI_OUTPUT: u32 = 5;
/// Multi-planar video capture.
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
/// Multi-planar video output.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE: u32 = 10;
/// Metadata capture.
pub const V4L2_BUF_TYPE_META_CAPTURE: u32 = 13;

// ---------------------------------------------------------------------------
// V4L2 memory model
// ---------------------------------------------------------------------------

/// Memory-mapped buffers.
pub const V4L2_MEMORY_MMAP: u32 = 1;
/// User-pointer buffers.
pub const V4L2_MEMORY_USERPTR: u32 = 2;
/// Overlay buffers.
pub const V4L2_MEMORY_OVERLAY: u32 = 3;
/// DMA shared buffers.
pub const V4L2_MEMORY_DMABUF: u32 = 4;

// ---------------------------------------------------------------------------
// V4L2 field order
// ---------------------------------------------------------------------------

/// Any field order.
pub const V4L2_FIELD_ANY: u32 = 0;
/// No fields (progressive).
pub const V4L2_FIELD_NONE: u32 = 1;
/// Top field only.
pub const V4L2_FIELD_TOP: u32 = 2;
/// Bottom field only.
pub const V4L2_FIELD_BOTTOM: u32 = 3;
/// Interlaced (top first).
pub const V4L2_FIELD_INTERLACED: u32 = 4;

// ---------------------------------------------------------------------------
// V4L2 ioctl commands
// ---------------------------------------------------------------------------

/// Query device capabilities.
pub const VIDIOC_QUERYCAP: u64 = 0x80685600;
/// Enumerate formats.
pub const VIDIOC_ENUM_FMT: u64 = 0xC0405602;
/// Get format.
pub const VIDIOC_G_FMT: u64 = 0xC0CC5604;
/// Set format.
pub const VIDIOC_S_FMT: u64 = 0xC0CC5605;
/// Request buffers.
pub const VIDIOC_REQBUFS: u64 = 0xC0145608;
/// Query buffer.
pub const VIDIOC_QUERYBUF: u64 = 0xC0585609;
/// Queue buffer.
pub const VIDIOC_QBUF: u64 = 0xC058560F;
/// Dequeue buffer.
pub const VIDIOC_DQBUF: u64 = 0xC0585611;
/// Start streaming.
pub const VIDIOC_STREAMON: u64 = 0x40045612;
/// Stop streaming.
pub const VIDIOC_STREAMOFF: u64 = 0x40045613;
/// Get parameters.
pub const VIDIOC_G_PARM: u64 = 0xC0CC5615;
/// Set parameters.
pub const VIDIOC_S_PARM: u64 = 0xC0CC5616;
/// Get standard.
pub const VIDIOC_G_STD: u64 = 0x80085617;
/// Set standard.
pub const VIDIOC_S_STD: u64 = 0x40085618;
/// Enumerate inputs.
pub const VIDIOC_ENUMINPUT: u64 = 0xC050561A;
/// Get control.
pub const VIDIOC_G_CTRL: u64 = 0xC008561B;
/// Set control.
pub const VIDIOC_S_CTRL: u64 = 0xC008561C;
/// Query control.
pub const VIDIOC_QUERYCTRL: u64 = 0xC0445624;
/// Get input.
pub const VIDIOC_G_INPUT: u64 = 0x80045626;
/// Set input.
pub const VIDIOC_S_INPUT: u64 = 0xC0045627;
/// Enumerate frame sizes.
pub const VIDIOC_ENUM_FRAMESIZES: u64 = 0xC02C564A;
/// Enumerate frame intervals.
pub const VIDIOC_ENUM_FRAMEINTERVALS: u64 = 0xC034564B;

// ---------------------------------------------------------------------------
// V4L2 capabilities
// ---------------------------------------------------------------------------

/// Supports video capture.
pub const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x00000001;
/// Supports video output.
pub const V4L2_CAP_VIDEO_OUTPUT: u32 = 0x00000002;
/// Supports video overlay.
pub const V4L2_CAP_VIDEO_OVERLAY: u32 = 0x00000004;
/// Supports VBI capture.
pub const V4L2_CAP_VBI_CAPTURE: u32 = 0x00000010;
/// Supports VBI output.
pub const V4L2_CAP_VBI_OUTPUT: u32 = 0x00000020;
/// Supports read/write I/O.
pub const V4L2_CAP_READWRITE: u32 = 0x01000000;
/// Supports async I/O.
pub const V4L2_CAP_ASYNCIO: u32 = 0x02000000;
/// Supports streaming I/O.
pub const V4L2_CAP_STREAMING: u32 = 0x04000000;
/// Supports multi-planar API.
pub const V4L2_CAP_VIDEO_CAPTURE_MPLANE: u32 = 0x00001000;
/// Supports multi-planar output.
pub const V4L2_CAP_VIDEO_OUTPUT_MPLANE: u32 = 0x00002000;
/// Meta capture.
pub const V4L2_CAP_META_CAPTURE: u32 = 0x00800000;
/// Device capabilities (vs. overall).
pub const V4L2_CAP_DEVICE_CAPS: u32 = 0x80000000;

// ---------------------------------------------------------------------------
// Pixel formats (fourcc)
// ---------------------------------------------------------------------------

/// Helper: build a fourcc from 4 bytes.
pub const fn v4l2_fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

/// RGB 24-bit.
pub const V4L2_PIX_FMT_RGB24: u32 = v4l2_fourcc(b'R', b'G', b'2', b'4');
/// BGR 24-bit.
pub const V4L2_PIX_FMT_BGR24: u32 = v4l2_fourcc(b'B', b'G', b'R', b'3');
/// YUYV 4:2:2.
pub const V4L2_PIX_FMT_YUYV: u32 = v4l2_fourcc(b'Y', b'U', b'Y', b'V');
/// MJPEG.
pub const V4L2_PIX_FMT_MJPEG: u32 = v4l2_fourcc(b'M', b'J', b'P', b'G');
/// JPEG.
pub const V4L2_PIX_FMT_JPEG: u32 = v4l2_fourcc(b'J', b'P', b'E', b'G');
/// H.264.
pub const V4L2_PIX_FMT_H264: u32 = v4l2_fourcc(b'H', b'2', b'6', b'4');
/// NV12 (Y/CbCr 4:2:0).
pub const V4L2_PIX_FMT_NV12: u32 = v4l2_fourcc(b'N', b'V', b'1', b'2');
/// NV21 (Y/CrCb 4:2:0).
pub const V4L2_PIX_FMT_NV21: u32 = v4l2_fourcc(b'N', b'V', b'2', b'1');
/// YUV 4:2:0 planar.
pub const V4L2_PIX_FMT_YUV420: u32 = v4l2_fourcc(b'Y', b'U', b'1', b'2');

// ---------------------------------------------------------------------------
// V4L2 control IDs
// ---------------------------------------------------------------------------

/// Control class: user controls.
pub const V4L2_CID_USER_CLASS: u32 = 0x00980000;
/// Brightness.
pub const V4L2_CID_BRIGHTNESS: u32 = V4L2_CID_USER_CLASS;
/// Contrast.
pub const V4L2_CID_CONTRAST: u32 = V4L2_CID_USER_CLASS + 1;
/// Saturation.
pub const V4L2_CID_SATURATION: u32 = V4L2_CID_USER_CLASS + 2;
/// Hue.
pub const V4L2_CID_HUE: u32 = V4L2_CID_USER_CLASS + 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buf_types_distinct() {
        let types = [
            V4L2_BUF_TYPE_VIDEO_CAPTURE,
            V4L2_BUF_TYPE_VIDEO_OUTPUT,
            V4L2_BUF_TYPE_VIDEO_OVERLAY,
            V4L2_BUF_TYPE_VBI_CAPTURE,
            V4L2_BUF_TYPE_VBI_OUTPUT,
            V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
            V4L2_BUF_TYPE_META_CAPTURE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_memory_types() {
        assert_eq!(V4L2_MEMORY_MMAP, 1);
        assert_eq!(V4L2_MEMORY_USERPTR, 2);
        assert_eq!(V4L2_MEMORY_DMABUF, 4);
    }

    #[test]
    fn test_capabilities_powers_of_two() {
        let caps = [
            V4L2_CAP_VIDEO_CAPTURE,
            V4L2_CAP_VIDEO_OUTPUT,
            V4L2_CAP_VIDEO_OVERLAY,
            V4L2_CAP_VBI_CAPTURE,
            V4L2_CAP_VBI_OUTPUT,
            V4L2_CAP_READWRITE,
            V4L2_CAP_ASYNCIO,
            V4L2_CAP_STREAMING,
            V4L2_CAP_VIDEO_CAPTURE_MPLANE,
            V4L2_CAP_VIDEO_OUTPUT_MPLANE,
            V4L2_CAP_META_CAPTURE,
            V4L2_CAP_DEVICE_CAPS,
        ];
        for c in &caps {
            assert!(c.is_power_of_two(), "cap {c:#x} not power of 2");
        }
    }

    #[test]
    fn test_fourcc() {
        // V4L2_PIX_FMT_YUYV = 'Y' | 'U'<<8 | 'Y'<<16 | 'V'<<24
        let expected =
            (b'Y' as u32) | ((b'U' as u32) << 8) | ((b'Y' as u32) << 16) | ((b'V' as u32) << 24);
        assert_eq!(V4L2_PIX_FMT_YUYV, expected);
    }

    #[test]
    fn test_pixel_formats_distinct() {
        let fmts = [
            V4L2_PIX_FMT_RGB24,
            V4L2_PIX_FMT_BGR24,
            V4L2_PIX_FMT_YUYV,
            V4L2_PIX_FMT_MJPEG,
            V4L2_PIX_FMT_JPEG,
            V4L2_PIX_FMT_H264,
            V4L2_PIX_FMT_NV12,
            V4L2_PIX_FMT_NV21,
            V4L2_PIX_FMT_YUV420,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_control_ids() {
        assert_eq!(V4L2_CID_BRIGHTNESS, 0x00980000);
        assert_eq!(V4L2_CID_CONTRAST, 0x00980001);
        assert_eq!(V4L2_CID_SATURATION, 0x00980002);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            VIDIOC_QUERYCAP,
            VIDIOC_ENUM_FMT,
            VIDIOC_G_FMT,
            VIDIOC_S_FMT,
            VIDIOC_REQBUFS,
            VIDIOC_QUERYBUF,
            VIDIOC_QBUF,
            VIDIOC_DQBUF,
            VIDIOC_STREAMON,
            VIDIOC_STREAMOFF,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
