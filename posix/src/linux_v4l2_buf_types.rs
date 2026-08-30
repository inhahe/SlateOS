//! `<linux/videodev2.h>` (buffer type subset) — V4L2 buffer type codes.
//!
//! V4L2 buffers carry video frames between kernel drivers and userspace.
//! The buffer type identifies which queue (capture or output) and which
//! I/O method (single-plane or multi-plane) the buffer belongs to.
//! Applications select the buffer type when requesting buffers via
//! `VIDIOC_REQBUFS` and when queuing/dequeuing via `VIDIOC_QBUF`/`DQBUF`.

// ---------------------------------------------------------------------------
// Buffer types (v4l2_buf_type)
// ---------------------------------------------------------------------------

/// Single-plane video capture.
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
/// Single-plane video output.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
/// Video overlay (preview).
pub const V4L2_BUF_TYPE_VIDEO_OVERLAY: u32 = 3;
/// VBI capture (vertical blanking interval).
pub const V4L2_BUF_TYPE_VBI_CAPTURE: u32 = 4;
/// VBI output.
pub const V4L2_BUF_TYPE_VBI_OUTPUT: u32 = 5;
/// Sliced VBI capture.
pub const V4L2_BUF_TYPE_SLICED_VBI_CAPTURE: u32 = 6;
/// Sliced VBI output.
pub const V4L2_BUF_TYPE_SLICED_VBI_OUTPUT: u32 = 7;
/// Video output overlay.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_OVERLAY: u32 = 8;
/// Multi-plane video capture.
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
/// Multi-plane video output.
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE: u32 = 10;
/// Software-defined radio capture.
pub const V4L2_BUF_TYPE_SDR_CAPTURE: u32 = 11;
/// Software-defined radio output.
pub const V4L2_BUF_TYPE_SDR_OUTPUT: u32 = 12;
/// Metadata capture.
pub const V4L2_BUF_TYPE_META_CAPTURE: u32 = 13;
/// Metadata output.
pub const V4L2_BUF_TYPE_META_OUTPUT: u32 = 14;

// ---------------------------------------------------------------------------
// Buffer flags
// ---------------------------------------------------------------------------

/// Buffer has been mapped into userspace.
pub const V4L2_BUF_FLAG_MAPPED: u32 = 0x0001;
/// Buffer is in the driver's incoming queue.
pub const V4L2_BUF_FLAG_QUEUED: u32 = 0x0002;
/// Buffer is ready to be dequeued (filled).
pub const V4L2_BUF_FLAG_DONE: u32 = 0x0004;
/// Buffer contains a keyframe.
pub const V4L2_BUF_FLAG_KEYFRAME: u32 = 0x0008;
/// Buffer contains a P-frame.
pub const V4L2_BUF_FLAG_PFRAME: u32 = 0x0010;
/// Buffer contains a B-frame.
pub const V4L2_BUF_FLAG_BFRAME: u32 = 0x0020;
/// Buffer has encountered an error.
pub const V4L2_BUF_FLAG_ERROR: u32 = 0x0040;
/// Timestamp type: monotonic.
pub const V4L2_BUF_FLAG_TIMESTAMP_MONOTONIC: u32 = 0x2000;
/// Timestamp source: end of frame.
pub const V4L2_BUF_FLAG_TSTAMP_SRC_EOF: u32 = 0x0000;
/// Timestamp source: start of capture.
pub const V4L2_BUF_FLAG_TSTAMP_SRC_SOE: u32 = 0x0100;
/// Last buffer in a sequence (EOS).
pub const V4L2_BUF_FLAG_LAST: u32 = 0x0010_0000;

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
    fn test_buf_types_sequential() {
        assert_eq!(V4L2_BUF_TYPE_VIDEO_CAPTURE, 1);
        assert_eq!(V4L2_BUF_TYPE_VIDEO_OUTPUT, 2);
        assert_eq!(V4L2_BUF_TYPE_META_OUTPUT, 14);
    }

    #[test]
    fn test_buf_flags_no_overlap() {
        // Core status flags should not overlap
        let flags = [
            V4L2_BUF_FLAG_MAPPED,
            V4L2_BUF_FLAG_QUEUED,
            V4L2_BUF_FLAG_DONE,
            V4L2_BUF_FLAG_KEYFRAME,
            V4L2_BUF_FLAG_ERROR,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mplane_types() {
        assert!(V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE > V4L2_BUF_TYPE_VIDEO_CAPTURE);
        assert!(V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE > V4L2_BUF_TYPE_VIDEO_OUTPUT);
    }

    #[test]
    fn test_last_flag() {
        assert!(V4L2_BUF_FLAG_LAST.is_power_of_two());
    }
}
