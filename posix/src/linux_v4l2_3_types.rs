//! `<linux/videodev2.h>` — Additional V4L2 constants (part 3).
//!
//! Supplementary V4L2 constants covering capture capabilities,
//! buffer types, and memory model types.

// ---------------------------------------------------------------------------
// V4L2 buffer types
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
/// Multi-planar video capture.
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
/// Multi-planar video output.
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
// V4L2 memory types
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
// V4L2 field types
// ---------------------------------------------------------------------------

/// Any field (driver chooses).
pub const V4L2_FIELD_ANY: u32 = 0;
/// No interlacing.
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
/// Alternate fields.
pub const V4L2_FIELD_ALTERNATE: u32 = 7;
/// Interlaced top-bottom.
pub const V4L2_FIELD_INTERLACED_TB: u32 = 8;
/// Interlaced bottom-top.
pub const V4L2_FIELD_INTERLACED_BT: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buf_types_distinct() {
        let types = [
            V4L2_BUF_TYPE_VIDEO_CAPTURE, V4L2_BUF_TYPE_VIDEO_OUTPUT,
            V4L2_BUF_TYPE_VIDEO_OVERLAY, V4L2_BUF_TYPE_VBI_CAPTURE,
            V4L2_BUF_TYPE_VBI_OUTPUT, V4L2_BUF_TYPE_SLICED_VBI_CAPTURE,
            V4L2_BUF_TYPE_SLICED_VBI_OUTPUT,
            V4L2_BUF_TYPE_VIDEO_OUTPUT_OVERLAY,
            V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
            V4L2_BUF_TYPE_SDR_CAPTURE, V4L2_BUF_TYPE_SDR_OUTPUT,
            V4L2_BUF_TYPE_META_CAPTURE, V4L2_BUF_TYPE_META_OUTPUT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_memory_types_distinct() {
        let types = [
            V4L2_MEMORY_MMAP, V4L2_MEMORY_USERPTR,
            V4L2_MEMORY_OVERLAY, V4L2_MEMORY_DMABUF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_field_types_distinct() {
        let fields = [
            V4L2_FIELD_ANY, V4L2_FIELD_NONE, V4L2_FIELD_TOP,
            V4L2_FIELD_BOTTOM, V4L2_FIELD_INTERLACED,
            V4L2_FIELD_SEQ_TB, V4L2_FIELD_SEQ_BT,
            V4L2_FIELD_ALTERNATE, V4L2_FIELD_INTERLACED_TB,
            V4L2_FIELD_INTERLACED_BT,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }
}
