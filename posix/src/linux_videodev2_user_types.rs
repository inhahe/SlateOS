//! `<linux/videodev2.h>` — top-level V4L2 ioctls + buffer / format enums.
//!
//! The V4L2 ioctl space is huge — this module covers the constants
//! that every V4L2 client touches: the magic letter, the buffer
//! types, memory models, capability bits, and the canonical pixel
//! format four-CCs (NV12, YUYV, MJPEG, JPEG).

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for V4L2 ioctls ('V').
pub const V4L2_IOC_MAGIC: u8 = b'V';

// ---------------------------------------------------------------------------
// Common ioctls
// ---------------------------------------------------------------------------

/// `VIDIOC_QUERYCAP` — query device capability bitmap.
pub const VIDIOC_QUERYCAP: u32 = 0x8068_5600;
/// `VIDIOC_ENUM_FMT` — enumerate pixel formats.
pub const VIDIOC_ENUM_FMT: u32 = 0xc040_5602;
/// `VIDIOC_G_FMT` — get current format.
pub const VIDIOC_G_FMT: u32 = 0xc0d0_5604;
/// `VIDIOC_S_FMT` — set format.
pub const VIDIOC_S_FMT: u32 = 0xc0d0_5605;
/// `VIDIOC_REQBUFS` — request kernel-side buffer pool.
pub const VIDIOC_REQBUFS: u32 = 0xc014_5608;
/// `VIDIOC_QUERYBUF` — query a buffer (offset, length).
pub const VIDIOC_QUERYBUF: u32 = 0xc058_5609;
/// `VIDIOC_QBUF` — enqueue a buffer for capture/output.
pub const VIDIOC_QBUF: u32 = 0xc058_560f;
/// `VIDIOC_DQBUF` — dequeue a filled buffer.
pub const VIDIOC_DQBUF: u32 = 0xc058_5611;
/// `VIDIOC_STREAMON` — start streaming.
pub const VIDIOC_STREAMON: u32 = 0x4004_5612;
/// `VIDIOC_STREAMOFF` — stop streaming.
pub const VIDIOC_STREAMOFF: u32 = 0x4004_5613;
/// `VIDIOC_G_INPUT` — query selected input.
pub const VIDIOC_G_INPUT: u32 = 0x8004_5626;
/// `VIDIOC_S_INPUT` — select input.
pub const VIDIOC_S_INPUT: u32 = 0xc004_5627;

// ---------------------------------------------------------------------------
// Buffer types (struct v4l2_buffer.type)
// ---------------------------------------------------------------------------

/// Single-plane video capture.
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
/// Single-plane video output.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
/// Video overlay (deprecated).
pub const V4L2_BUF_TYPE_VIDEO_OVERLAY: u32 = 3;
/// VBI capture (teletext / closed captions).
pub const V4L2_BUF_TYPE_VBI_CAPTURE: u32 = 4;
/// VBI output.
pub const V4L2_BUF_TYPE_VBI_OUTPUT: u32 = 5;
/// Multi-plane video capture (NV12 / YUV planes).
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
/// Multi-plane video output.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE: u32 = 10;
/// SDR capture.
pub const V4L2_BUF_TYPE_SDR_CAPTURE: u32 = 11;
/// Metadata capture.
pub const V4L2_BUF_TYPE_META_CAPTURE: u32 = 13;
/// Metadata output.
pub const V4L2_BUF_TYPE_META_OUTPUT: u32 = 14;

// ---------------------------------------------------------------------------
// Memory models (struct v4l2_buffer.memory)
// ---------------------------------------------------------------------------

/// MMAP — kernel-allocated buffers mapped into userspace.
pub const V4L2_MEMORY_MMAP: u32 = 1;
/// USERPTR — user-allocated pointer.
pub const V4L2_MEMORY_USERPTR: u32 = 2;
/// OVERLAY (deprecated).
pub const V4L2_MEMORY_OVERLAY: u32 = 3;
/// DMABUF — buffer is a dma-buf fd.
pub const V4L2_MEMORY_DMABUF: u32 = 4;

// ---------------------------------------------------------------------------
// Capability bits (v4l2_capability.capabilities)
// ---------------------------------------------------------------------------

/// Capture supported.
pub const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
/// Output supported.
pub const V4L2_CAP_VIDEO_OUTPUT: u32 = 0x0000_0002;
/// Multi-plane capture.
pub const V4L2_CAP_VIDEO_CAPTURE_MPLANE: u32 = 0x0000_1000;
/// Multi-plane output.
pub const V4L2_CAP_VIDEO_OUTPUT_MPLANE: u32 = 0x0000_2000;
/// streaming (MMAP/USERPTR/DMABUF) supported.
pub const V4L2_CAP_STREAMING: u32 = 0x0400_0000;
/// device_caps field is valid.
pub const V4L2_CAP_DEVICE_CAPS: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Common pixel formats (four-CC encoded as u32 little-endian)
// ---------------------------------------------------------------------------

/// `YUYV` (4:2:2 packed).
pub const V4L2_PIX_FMT_YUYV: u32 = u32::from_le_bytes(*b"YUYV");
/// `NV12` (Y plane + interleaved UV).
pub const V4L2_PIX_FMT_NV12: u32 = u32::from_le_bytes(*b"NV12");
/// `NV21` (Y plane + interleaved VU).
pub const V4L2_PIX_FMT_NV21: u32 = u32::from_le_bytes(*b"NV21");
/// `YV12` (planar Y/V/U).
pub const V4L2_PIX_FMT_YV12: u32 = u32::from_le_bytes(*b"YV12");
/// `MJPG` (Motion JPEG stream).
pub const V4L2_PIX_FMT_MJPEG: u32 = u32::from_le_bytes(*b"MJPG");
/// `JPEG` (single JPEG image).
pub const V4L2_PIX_FMT_JPEG: u32 = u32::from_le_bytes(*b"JPEG");
/// `RGB3` (packed 24-bit RGB).
pub const V4L2_PIX_FMT_RGB24: u32 = u32::from_le_bytes(*b"RGB3");
/// `BGR3` (packed 24-bit BGR).
pub const V4L2_PIX_FMT_BGR24: u32 = u32::from_le_bytes(*b"BGR3");
/// `H264` (H.264 NAL byte stream).
pub const V4L2_PIX_FMT_H264: u32 = u32::from_le_bytes(*b"H264");

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_v() {
        assert_eq!(V4L2_IOC_MAGIC, b'V');
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_v() {
        let ops = [
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
            VIDIOC_G_INPUT,
            VIDIOC_S_INPUT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'V' (0x56) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'V' as u32);
        }
    }

    #[test]
    fn test_buf_types_distinct() {
        let b = [
            V4L2_BUF_TYPE_VIDEO_CAPTURE,
            V4L2_BUF_TYPE_VIDEO_OUTPUT,
            V4L2_BUF_TYPE_VIDEO_OVERLAY,
            V4L2_BUF_TYPE_VBI_CAPTURE,
            V4L2_BUF_TYPE_VBI_OUTPUT,
            V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
            V4L2_BUF_TYPE_SDR_CAPTURE,
            V4L2_BUF_TYPE_META_CAPTURE,
            V4L2_BUF_TYPE_META_OUTPUT,
        ];
        for i in 0..b.len() {
            for j in (i + 1)..b.len() {
                assert_ne!(b[i], b[j]);
            }
        }
    }

    #[test]
    fn test_memory_models_dense_starting_from_1() {
        // The kernel rejects v4l2_buffer.memory == 0 (it's an explicit
        // out-of-range value).
        assert_eq!(V4L2_MEMORY_MMAP, 1);
        assert_eq!(V4L2_MEMORY_USERPTR, 2);
        assert_eq!(V4L2_MEMORY_OVERLAY, 3);
        assert_eq!(V4L2_MEMORY_DMABUF, 4);
    }

    #[test]
    fn test_cap_bits_distinct() {
        let c = [
            V4L2_CAP_VIDEO_CAPTURE,
            V4L2_CAP_VIDEO_OUTPUT,
            V4L2_CAP_VIDEO_CAPTURE_MPLANE,
            V4L2_CAP_VIDEO_OUTPUT_MPLANE,
            V4L2_CAP_STREAMING,
            V4L2_CAP_DEVICE_CAPS,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
            // Each capability bit is exactly one bit.
            assert!(c[i].is_power_of_two());
        }
    }

    #[test]
    fn test_pixel_format_fourccs_match_ascii() {
        // The lowest byte is the first character of the four-CC.
        assert_eq!(V4L2_PIX_FMT_YUYV & 0xff, b'Y' as u32);
        assert_eq!(V4L2_PIX_FMT_NV12 & 0xff, b'N' as u32);
        assert_eq!(V4L2_PIX_FMT_MJPEG & 0xff, b'M' as u32);
        assert_eq!(V4L2_PIX_FMT_H264 & 0xff, b'H' as u32);
        // YUYV and NV12 are the two most-common camera output formats.
        assert_ne!(V4L2_PIX_FMT_YUYV, V4L2_PIX_FMT_NV12);
    }
}
