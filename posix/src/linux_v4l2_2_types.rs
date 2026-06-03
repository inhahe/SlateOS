//! `<linux/videodev2.h>` — Additional V4L2 constants.
//!
//! Supplementary V4L2 constants covering buffer types,
//! memory types, field types, and streaming I/O flags.

// ---------------------------------------------------------------------------
// Buffer types (V4L2_BUF_TYPE_*)
// ---------------------------------------------------------------------------

/// Video capture.
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
/// Video output.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
/// Video overlay.
pub const V4L2_BUF_TYPE_VIDEO_OVERLAY: u32 = 3;
/// VBI capture.
pub const V4L2_BUF_TYPE_VBI_CAPTURE: u32 = 4;
/// VBI output.
pub const V4L2_BUF_TYPE_VBI_OUTPUT: u32 = 5;
/// Sliced VBI capture.
pub const V4L2_BUF_TYPE_SLICED_VBI_CAPTURE: u32 = 6;
/// Sliced VBI output.
pub const V4L2_BUF_TYPE_SLICED_VBI_OUTPUT: u32 = 7;
/// Video output overlay.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_OVERLAY: u32 = 8;
/// Video capture (multiplanar).
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
/// Video output (multiplanar).
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE: u32 = 10;
/// SDR capture.
pub const V4L2_BUF_TYPE_SDR_CAPTURE: u32 = 11;
/// SDR output.
pub const V4L2_BUF_TYPE_SDR_OUTPUT: u32 = 12;
/// Metadata capture.
pub const V4L2_BUF_TYPE_META_CAPTURE: u32 = 13;
/// Metadata output.
pub const V4L2_BUF_TYPE_META_OUTPUT: u32 = 14;

// ---------------------------------------------------------------------------
// Memory types (V4L2_MEMORY_*)
// ---------------------------------------------------------------------------

/// Memory mapped.
pub const V4L2_MEMORY_MMAP: u32 = 1;
/// User pointer.
pub const V4L2_MEMORY_USERPTR: u32 = 2;
/// Overlay.
pub const V4L2_MEMORY_OVERLAY: u32 = 3;
/// DMA buffer.
pub const V4L2_MEMORY_DMABUF: u32 = 4;

// ---------------------------------------------------------------------------
// Field types (V4L2_FIELD_*)
// ---------------------------------------------------------------------------

/// Any field.
pub const V4L2_FIELD_ANY: u32 = 0;
/// No field.
pub const V4L2_FIELD_NONE: u32 = 1;
/// Top field.
pub const V4L2_FIELD_TOP: u32 = 2;
/// Bottom field.
pub const V4L2_FIELD_BOTTOM: u32 = 3;
/// Interlaced.
pub const V4L2_FIELD_INTERLACED: u32 = 4;
/// Sequential top-bottom.
pub const V4L2_FIELD_SEQ_TB: u32 = 5;
/// Sequential bottom-top.
pub const V4L2_FIELD_SEQ_BT: u32 = 6;
/// Alternate.
pub const V4L2_FIELD_ALTERNATE: u32 = 7;
/// Interlaced top-bottom.
pub const V4L2_FIELD_INTERLACED_TB: u32 = 8;
/// Interlaced bottom-top.
pub const V4L2_FIELD_INTERLACED_BT: u32 = 9;

// ---------------------------------------------------------------------------
// Buffer flags (V4L2_BUF_FLAG_*)
// ---------------------------------------------------------------------------

/// Buffer is mapped.
pub const V4L2_BUF_FLAG_MAPPED: u32 = 0x00000001;
/// Buffer is queued.
pub const V4L2_BUF_FLAG_QUEUED: u32 = 0x00000002;
/// Buffer is done.
pub const V4L2_BUF_FLAG_DONE: u32 = 0x00000004;
/// Keyframe.
pub const V4L2_BUF_FLAG_KEYFRAME: u32 = 0x00000008;
/// P-frame.
pub const V4L2_BUF_FLAG_PFRAME: u32 = 0x00000010;
/// B-frame.
pub const V4L2_BUF_FLAG_BFRAME: u32 = 0x00000020;
/// Error flag.
pub const V4L2_BUF_FLAG_ERROR: u32 = 0x00000040;
/// In request.
pub const V4L2_BUF_FLAG_IN_REQUEST: u32 = 0x00000080;
/// Timestamp mono.
pub const V4L2_BUF_FLAG_TIMESTAMP_MONOTONIC: u32 = 0x00002000;
/// Last buffer.
pub const V4L2_BUF_FLAG_LAST: u32 = 0x00100000;
/// Request FD.
pub const V4L2_BUF_FLAG_REQUEST_FD: u32 = 0x00800000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buf_types_sequential() {
        assert_eq!(V4L2_BUF_TYPE_VIDEO_CAPTURE, 1);
        assert_eq!(V4L2_BUF_TYPE_VIDEO_OUTPUT, 2);
        assert_eq!(V4L2_BUF_TYPE_META_OUTPUT, 14);
    }

    #[test]
    fn test_buf_types_distinct() {
        let types = [
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
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_memory_types_sequential() {
        assert_eq!(V4L2_MEMORY_MMAP, 1);
        assert_eq!(V4L2_MEMORY_USERPTR, 2);
        assert_eq!(V4L2_MEMORY_OVERLAY, 3);
        assert_eq!(V4L2_MEMORY_DMABUF, 4);
    }

    #[test]
    fn test_field_types_sequential() {
        assert_eq!(V4L2_FIELD_ANY, 0);
        assert_eq!(V4L2_FIELD_NONE, 1);
        assert_eq!(V4L2_FIELD_INTERLACED_BT, 9);
    }

    #[test]
    fn test_buf_flags_power_of_two() {
        let flags = [
            V4L2_BUF_FLAG_MAPPED,
            V4L2_BUF_FLAG_QUEUED,
            V4L2_BUF_FLAG_DONE,
            V4L2_BUF_FLAG_KEYFRAME,
            V4L2_BUF_FLAG_PFRAME,
            V4L2_BUF_FLAG_BFRAME,
            V4L2_BUF_FLAG_ERROR,
            V4L2_BUF_FLAG_IN_REQUEST,
            V4L2_BUF_FLAG_LAST,
            V4L2_BUF_FLAG_REQUEST_FD,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }
}
