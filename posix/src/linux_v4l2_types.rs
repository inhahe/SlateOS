//! `<linux/videodev2.h>` — Video4Linux2 (V4L2) constants.
//!
//! V4L2 is the Linux API for video capture/output devices (webcams,
//! TV tuners, video encoders/decoders). Applications open /dev/videoN,
//! negotiate format/resolution, and stream frames using memory-mapped
//! or DMA-BUF buffers. Used by every Linux camera application.

// ---------------------------------------------------------------------------
// Buffer types
// ---------------------------------------------------------------------------

/// Video capture (camera/tuner → application).
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
/// Video output (application → display/encoder).
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
/// Video overlay (direct framebuffer composite).
pub const V4L2_BUF_TYPE_VIDEO_OVERLAY: u32 = 3;
/// VBI capture (vertical blanking interval).
pub const V4L2_BUF_TYPE_VBI_CAPTURE: u32 = 4;
/// VBI output.
pub const V4L2_BUF_TYPE_VBI_OUTPUT: u32 = 5;
/// Multi-planar video capture.
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
/// Multi-planar video output.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE: u32 = 10;
/// Metadata capture.
pub const V4L2_BUF_TYPE_META_CAPTURE: u32 = 13;
/// Metadata output.
pub const V4L2_BUF_TYPE_META_OUTPUT: u32 = 14;

// ---------------------------------------------------------------------------
// Memory types (how buffers are allocated)
// ---------------------------------------------------------------------------

/// Memory-mapped buffers (kernel allocates, mmap'd to userspace).
pub const V4L2_MEMORY_MMAP: u32 = 1;
/// Userspace pointer (application provides buffer).
pub const V4L2_MEMORY_USERPTR: u32 = 2;
/// DMA-BUF file descriptor (zero-copy sharing).
pub const V4L2_MEMORY_DMABUF: u32 = 4;

// ---------------------------------------------------------------------------
// Pixel format FourCC helpers
// ---------------------------------------------------------------------------

/// YUYV 4:2:2 packed (common webcam format).
pub const V4L2_PIX_FMT_YUYV: u32 = 0x5659_5559; // 'YUYV'
/// NV12 (Y plane + interleaved UV, common for HW codecs).
pub const V4L2_PIX_FMT_NV12: u32 = 0x3231_564E; // 'NV12'
/// MJPEG compressed.
pub const V4L2_PIX_FMT_MJPEG: u32 = 0x4745_504D; // 'MJPG'
/// RGB24 (3 bytes per pixel).
pub const V4L2_PIX_FMT_RGB24: u32 = 0x3342_4752; // 'RGB3'
/// H.264 compressed video.
pub const V4L2_PIX_FMT_H264: u32 = 0x3436_3248; // 'H264'

// ---------------------------------------------------------------------------
// Capability flags
// ---------------------------------------------------------------------------

/// Device supports video capture.
pub const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
/// Device supports video output.
pub const V4L2_CAP_VIDEO_OUTPUT: u32 = 0x0000_0002;
/// Device supports streaming I/O.
pub const V4L2_CAP_STREAMING: u32 = 0x0400_0000;
/// Device supports read/write I/O.
pub const V4L2_CAP_READWRITE: u32 = 0x0100_0000;
/// Device has a tuner.
pub const V4L2_CAP_TUNER: u32 = 0x0002_0000;
/// Multi-planar API is used.
pub const V4L2_CAP_VIDEO_CAPTURE_MPLANE: u32 = 0x0000_1000;
/// Device has hardware codec.
pub const V4L2_CAP_VIDEO_M2M_MPLANE: u32 = 0x0000_4000;

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
            V4L2_BUF_TYPE_META_OUTPUT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_memory_types_distinct() {
        let mems = [V4L2_MEMORY_MMAP, V4L2_MEMORY_USERPTR, V4L2_MEMORY_DMABUF];
        for i in 0..mems.len() {
            for j in (i + 1)..mems.len() {
                assert_ne!(mems[i], mems[j]);
            }
        }
    }

    #[test]
    fn test_pixel_formats_distinct() {
        let fmts = [
            V4L2_PIX_FMT_YUYV,
            V4L2_PIX_FMT_NV12,
            V4L2_PIX_FMT_MJPEG,
            V4L2_PIX_FMT_RGB24,
            V4L2_PIX_FMT_H264,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_cap_flags_no_overlap() {
        let caps = [
            V4L2_CAP_VIDEO_CAPTURE,
            V4L2_CAP_VIDEO_OUTPUT,
            V4L2_CAP_STREAMING,
            V4L2_CAP_READWRITE,
            V4L2_CAP_TUNER,
            V4L2_CAP_VIDEO_CAPTURE_MPLANE,
            V4L2_CAP_VIDEO_M2M_MPLANE,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }
}
