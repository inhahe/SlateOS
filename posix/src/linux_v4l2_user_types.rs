//! `<linux/videodev2.h>` — Video4Linux2 capture API.
//!
//! V4L2 is the kernel API webcams, TV tuners, and decoders speak.
//! Userspace opens `/dev/video<N>`, queries capabilities, picks a
//! pixel format with `VIDIOC_S_FMT`, enqueues buffers with
//! `VIDIOC_QBUF`, and pumps frames with `VIDIOC_DQBUF`.

// ---------------------------------------------------------------------------
// Device nodes
// ---------------------------------------------------------------------------

pub const DEV_VIDEO_PREFIX: &str = "/dev/video";
pub const DEV_VBI_PREFIX: &str = "/dev/vbi";
pub const DEV_RADIO_PREFIX: &str = "/dev/radio";
pub const DEV_V4L_SUBDEV_PREFIX: &str = "/dev/v4l-subdev";

// ---------------------------------------------------------------------------
// Buffer types (`enum v4l2_buf_type`)
// ---------------------------------------------------------------------------

pub const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
pub const V4L2_BUF_TYPE_VIDEO_OVERLAY: u32 = 3;
pub const V4L2_BUF_TYPE_VBI_CAPTURE: u32 = 4;
pub const V4L2_BUF_TYPE_VBI_OUTPUT: u32 = 5;
pub const V4L2_BUF_TYPE_SLICED_VBI_CAPTURE: u32 = 6;
pub const V4L2_BUF_TYPE_SLICED_VBI_OUTPUT: u32 = 7;
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_OVERLAY: u32 = 8;
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE: u32 = 10;
pub const V4L2_BUF_TYPE_SDR_CAPTURE: u32 = 11;
pub const V4L2_BUF_TYPE_SDR_OUTPUT: u32 = 12;
pub const V4L2_BUF_TYPE_META_CAPTURE: u32 = 13;
pub const V4L2_BUF_TYPE_META_OUTPUT: u32 = 14;

// ---------------------------------------------------------------------------
// Memory model (`enum v4l2_memory`)
// ---------------------------------------------------------------------------

pub const V4L2_MEMORY_MMAP: u32 = 1;
pub const V4L2_MEMORY_USERPTR: u32 = 2;
pub const V4L2_MEMORY_OVERLAY: u32 = 3;
pub const V4L2_MEMORY_DMABUF: u32 = 4;

// ---------------------------------------------------------------------------
// Common pixel formats (FOURCC, little-endian packing)
// ---------------------------------------------------------------------------

const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

pub const V4L2_PIX_FMT_RGB24: u32 = fourcc(b'R', b'G', b'B', b'3');
pub const V4L2_PIX_FMT_BGR24: u32 = fourcc(b'B', b'G', b'R', b'3');
pub const V4L2_PIX_FMT_YUYV: u32 = fourcc(b'Y', b'U', b'Y', b'V');
pub const V4L2_PIX_FMT_UYVY: u32 = fourcc(b'U', b'Y', b'V', b'Y');
pub const V4L2_PIX_FMT_YU12: u32 = fourcc(b'Y', b'U', b'1', b'2');
pub const V4L2_PIX_FMT_NV12: u32 = fourcc(b'N', b'V', b'1', b'2');
pub const V4L2_PIX_FMT_NV21: u32 = fourcc(b'N', b'V', b'2', b'1');
pub const V4L2_PIX_FMT_MJPEG: u32 = fourcc(b'M', b'J', b'P', b'G');
pub const V4L2_PIX_FMT_JPEG: u32 = fourcc(b'J', b'P', b'E', b'G');
pub const V4L2_PIX_FMT_H264: u32 = fourcc(b'H', b'2', b'6', b'4');

// ---------------------------------------------------------------------------
// `v4l2_capability.capabilities` flag bits
// ---------------------------------------------------------------------------

pub const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
pub const V4L2_CAP_VIDEO_OUTPUT: u32 = 0x0000_0002;
pub const V4L2_CAP_VIDEO_OVERLAY: u32 = 0x0000_0004;
pub const V4L2_CAP_VBI_CAPTURE: u32 = 0x0000_0010;
pub const V4L2_CAP_VBI_OUTPUT: u32 = 0x0000_0020;
pub const V4L2_CAP_TUNER: u32 = 0x0001_0000;
pub const V4L2_CAP_AUDIO: u32 = 0x0002_0000;
pub const V4L2_CAP_READWRITE: u32 = 0x0100_0000;
pub const V4L2_CAP_STREAMING: u32 = 0x0400_0000;
pub const V4L2_CAP_DEVICE_CAPS: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Field order (`enum v4l2_field`)
// ---------------------------------------------------------------------------

pub const V4L2_FIELD_ANY: u32 = 0;
pub const V4L2_FIELD_NONE: u32 = 1;
pub const V4L2_FIELD_TOP: u32 = 2;
pub const V4L2_FIELD_BOTTOM: u32 = 3;
pub const V4L2_FIELD_INTERLACED: u32 = 4;
pub const V4L2_FIELD_SEQ_TB: u32 = 5;
pub const V4L2_FIELD_SEQ_BT: u32 = 6;
pub const V4L2_FIELD_ALTERNATE: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_prefixes_under_dev() {
        for path in [DEV_VIDEO_PREFIX, DEV_VBI_PREFIX, DEV_RADIO_PREFIX, DEV_V4L_SUBDEV_PREFIX] {
            assert!(path.starts_with("/dev/"));
        }
    }

    #[test]
    fn test_buf_types_dense_1_to_14() {
        let b = [
            V4L2_BUF_TYPE_VIDEO_CAPTURE,
            V4L2_BUF_TYPE_VIDEO_OUTPUT,
            V4L2_BUF_TYPE_VIDEO_OVERLAY,
            V4L2_BUF_TYPE_VBI_CAPTURE,
            V4L2_BUF_TYPE_VBI_OUTPUT,
            V4L2_BUF_TYPE_SLICED_VBI_CAPTURE,
            V4L2_BUF_TYPE_SLICED_VBI_OUTPUT,
            V4L2_BUF_TYPE_VIDEO_OUTPUT_OVERLAY,
            V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
            V4L2_BUF_TYPE_SDR_CAPTURE,
            V4L2_BUF_TYPE_SDR_OUTPUT,
            V4L2_BUF_TYPE_META_CAPTURE,
            V4L2_BUF_TYPE_META_OUTPUT,
        ];
        for (i, &v) in b.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_memory_modes_dense_1_to_4() {
        assert_eq!(V4L2_MEMORY_MMAP, 1);
        assert_eq!(V4L2_MEMORY_USERPTR, 2);
        assert_eq!(V4L2_MEMORY_OVERLAY, 3);
        assert_eq!(V4L2_MEMORY_DMABUF, 4);
    }

    #[test]
    fn test_fourcc_byte_order_little_endian() {
        // FOURCC is packed little-endian: 'R'='G'='B'='3' → bytes 0..3.
        let r = V4L2_PIX_FMT_RGB24;
        assert_eq!((r & 0xFF) as u8, b'R');
        assert_eq!(((r >> 8) & 0xFF) as u8, b'G');
        assert_eq!(((r >> 16) & 0xFF) as u8, b'B');
        assert_eq!(((r >> 24) & 0xFF) as u8, b'3');
        // Common ones look like text in `lsv4l2` output.
        assert_eq!(V4L2_PIX_FMT_YUYV.to_le_bytes(), *b"YUYV");
        assert_eq!(V4L2_PIX_FMT_MJPEG.to_le_bytes(), *b"MJPG");
    }

    #[test]
    fn test_capability_bits_distinct() {
        let c = [
            V4L2_CAP_VIDEO_CAPTURE,
            V4L2_CAP_VIDEO_OUTPUT,
            V4L2_CAP_VIDEO_OVERLAY,
            V4L2_CAP_VBI_CAPTURE,
            V4L2_CAP_VBI_OUTPUT,
            V4L2_CAP_TUNER,
            V4L2_CAP_AUDIO,
            V4L2_CAP_READWRITE,
            V4L2_CAP_STREAMING,
            V4L2_CAP_DEVICE_CAPS,
        ];
        for v in c {
            assert!(v.is_power_of_two());
        }
        // DEVICE_CAPS sits at the top bit because it was a late
        // addition (kernel 3.4) when the rest of the low bits were
        // already taken.
        assert_eq!(V4L2_CAP_DEVICE_CAPS, 1 << 31);
    }

    #[test]
    fn test_field_modes_dense_0_to_7() {
        let f = [
            V4L2_FIELD_ANY,
            V4L2_FIELD_NONE,
            V4L2_FIELD_TOP,
            V4L2_FIELD_BOTTOM,
            V4L2_FIELD_INTERLACED,
            V4L2_FIELD_SEQ_TB,
            V4L2_FIELD_SEQ_BT,
            V4L2_FIELD_ALTERNATE,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_nv12_and_nv21_differ_by_chroma_swap() {
        // NV12 and NV21 are byte-swapped at the Y plane suffix —
        // they're the same words with the last two letters reversed.
        assert_ne!(V4L2_PIX_FMT_NV12, V4L2_PIX_FMT_NV21);
        let b12 = V4L2_PIX_FMT_NV12.to_le_bytes();
        let b21 = V4L2_PIX_FMT_NV21.to_le_bytes();
        assert_eq!(b12[0..2], b21[0..2]); // "NV"
        // Last 2 bytes are "12" vs "21".
        assert_eq!((b12[2], b12[3]), (b'1', b'2'));
        assert_eq!((b21[2], b21[3]), (b'2', b'1'));
    }
}
