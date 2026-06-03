//! `<linux/v4l2-controls.h>` (JPEG class) — V4L2 JPEG-codec controls.
//!
//! V4L2 exposes JPEG capture/encode parameters through the
//! `V4L2_CID_JPEG_CLASS` control class. Webcams, MJPEG sensors,
//! and hardware encoders publish the controls below for userspace
//! tools (qv4l2, gstreamer's v4l2src, libcamera) to tune.

// ---------------------------------------------------------------------------
// JPEG control class base
// ---------------------------------------------------------------------------

/// Base control ID for the JPEG control class.
pub const V4L2_CID_JPEG_CLASS_BASE: u32 = 0x009d_0900;

// ---------------------------------------------------------------------------
// JPEG controls
// ---------------------------------------------------------------------------

/// Chroma subsampling factor (see V4L2_JPEG_CHROMA_SUBSAMPLING_* below).
pub const V4L2_CID_JPEG_CHROMA_SUBSAMPLING: u32 = V4L2_CID_JPEG_CLASS_BASE + 1;
/// Restart-interval (MCUs between restart markers, 0 disables).
pub const V4L2_CID_JPEG_RESTART_INTERVAL: u32 = V4L2_CID_JPEG_CLASS_BASE + 2;
/// Compression quality (1..100).
pub const V4L2_CID_JPEG_COMPRESSION_QUALITY: u32 = V4L2_CID_JPEG_CLASS_BASE + 3;
/// Active markers bitmap (which APPn/COM/DRI markers to emit).
pub const V4L2_CID_JPEG_ACTIVE_MARKER: u32 = V4L2_CID_JPEG_CLASS_BASE + 4;

// ---------------------------------------------------------------------------
// Chroma subsampling enum values
// ---------------------------------------------------------------------------

/// 4:4:4 chroma subsampling (no subsampling).
pub const V4L2_JPEG_CHROMA_SUBSAMPLING_444: u32 = 0;
/// 4:2:2 chroma subsampling.
pub const V4L2_JPEG_CHROMA_SUBSAMPLING_422: u32 = 1;
/// 4:2:0 chroma subsampling.
pub const V4L2_JPEG_CHROMA_SUBSAMPLING_420: u32 = 2;
/// 4:1:1 chroma subsampling.
pub const V4L2_JPEG_CHROMA_SUBSAMPLING_411: u32 = 3;
/// 4:1:0 chroma subsampling.
pub const V4L2_JPEG_CHROMA_SUBSAMPLING_410: u32 = 4;
/// Grayscale (no chroma).
pub const V4L2_JPEG_CHROMA_SUBSAMPLING_GRAY: u32 = 5;

// ---------------------------------------------------------------------------
// Active-marker bits (V4L2_JPEG_ACTIVE_MARKER_*)
// ---------------------------------------------------------------------------

/// APP0 marker active (JFIF / AVI1 header).
pub const V4L2_JPEG_ACTIVE_MARKER_APP0: u32 = 1 << 0;
/// APP1 marker active (Exif).
pub const V4L2_JPEG_ACTIVE_MARKER_APP1: u32 = 1 << 1;
/// COM (comment) marker active.
pub const V4L2_JPEG_ACTIVE_MARKER_COM: u32 = 1 << 16;
/// DQT (quant-table) marker active.
pub const V4L2_JPEG_ACTIVE_MARKER_DQT: u32 = 1 << 17;
/// DHT (huffman-table) marker active.
pub const V4L2_JPEG_ACTIVE_MARKER_DHT: u32 = 1 << 18;

// ---------------------------------------------------------------------------
// Compression-quality bounds
// ---------------------------------------------------------------------------

/// Minimum compression-quality value (1 = lowest).
pub const V4L2_JPEG_QUALITY_MIN: u32 = 1;
/// Maximum compression-quality value (100 = highest).
pub const V4L2_JPEG_QUALITY_MAX: u32 = 100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controls_distinct_and_in_class_base() {
        let c = [
            V4L2_CID_JPEG_CHROMA_SUBSAMPLING,
            V4L2_CID_JPEG_RESTART_INTERVAL,
            V4L2_CID_JPEG_COMPRESSION_QUALITY,
            V4L2_CID_JPEG_ACTIVE_MARKER,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
            // Every JPEG control sits in the JPEG class base block.
            assert!(c[i] > V4L2_CID_JPEG_CLASS_BASE);
            assert!(c[i] < V4L2_CID_JPEG_CLASS_BASE + 256);
        }
    }

    #[test]
    fn test_chroma_modes_distinct() {
        let m = [
            V4L2_JPEG_CHROMA_SUBSAMPLING_444,
            V4L2_JPEG_CHROMA_SUBSAMPLING_422,
            V4L2_JPEG_CHROMA_SUBSAMPLING_420,
            V4L2_JPEG_CHROMA_SUBSAMPLING_411,
            V4L2_JPEG_CHROMA_SUBSAMPLING_410,
            V4L2_JPEG_CHROMA_SUBSAMPLING_GRAY,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
        // 4:4:4 is the default (mode 0).
        assert_eq!(V4L2_JPEG_CHROMA_SUBSAMPLING_444, 0);
    }

    #[test]
    fn test_active_marker_bits_distinct_pow2() {
        let b = [
            V4L2_JPEG_ACTIVE_MARKER_APP0,
            V4L2_JPEG_ACTIVE_MARKER_APP1,
            V4L2_JPEG_ACTIVE_MARKER_COM,
            V4L2_JPEG_ACTIVE_MARKER_DQT,
            V4L2_JPEG_ACTIVE_MARKER_DHT,
        ];
        for &m in &b {
            assert!(m.is_power_of_two());
        }
        for i in 0..b.len() {
            for j in (i + 1)..b.len() {
                assert_ne!(b[i], b[j]);
            }
        }
    }

    #[test]
    fn test_quality_bounds() {
        assert_eq!(V4L2_JPEG_QUALITY_MIN, 1);
        assert_eq!(V4L2_JPEG_QUALITY_MAX, 100);
        assert!(V4L2_JPEG_QUALITY_MIN < V4L2_JPEG_QUALITY_MAX);
    }
}
