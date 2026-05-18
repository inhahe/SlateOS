//! `<linux/videodev2.h>` (field order subset) — V4L2 field interlacing modes.
//!
//! Analogue video (PAL, NTSC) transmits frames as two interlaced
//! fields (odd and even lines). V4L2 field types describe how a
//! captured buffer relates to these fields: progressive (non-
//! interlaced), top-field-first, bottom-field-first, alternating
//! fields in separate buffers, etc. Modern digital cameras are
//! always progressive but analogue capture cards need these.

// ---------------------------------------------------------------------------
// Field order codes (v4l2_field)
// ---------------------------------------------------------------------------

/// Driver decides (progressive if possible).
pub const V4L2_FIELD_ANY: u32 = 0;
/// No interlacing — progressive frame.
pub const V4L2_FIELD_NONE: u32 = 1;
/// Top (odd) field only.
pub const V4L2_FIELD_TOP: u32 = 2;
/// Bottom (even) field only.
pub const V4L2_FIELD_BOTTOM: u32 = 3;
/// Both fields interleaved in one buffer.
pub const V4L2_FIELD_INTERLACED: u32 = 4;
/// Alternating: each buffer is one field (seq: T B T B …).
pub const V4L2_FIELD_SEQ_TB: u32 = 5;
/// Alternating: each buffer is one field (seq: B T B T …).
pub const V4L2_FIELD_SEQ_BT: u32 = 6;
/// Both fields alternating per buffer.
pub const V4L2_FIELD_ALTERNATE: u32 = 7;
/// Interlaced, top field first.
pub const V4L2_FIELD_INTERLACED_TB: u32 = 8;
/// Interlaced, bottom field first.
pub const V4L2_FIELD_INTERLACED_BT: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_codes_distinct() {
        let fields = [
            V4L2_FIELD_ANY, V4L2_FIELD_NONE,
            V4L2_FIELD_TOP, V4L2_FIELD_BOTTOM,
            V4L2_FIELD_INTERLACED,
            V4L2_FIELD_SEQ_TB, V4L2_FIELD_SEQ_BT,
            V4L2_FIELD_ALTERNATE,
            V4L2_FIELD_INTERLACED_TB, V4L2_FIELD_INTERLACED_BT,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_field_codes_sequential() {
        assert_eq!(V4L2_FIELD_ANY, 0);
        assert_eq!(V4L2_FIELD_NONE, 1);
        assert_eq!(V4L2_FIELD_TOP, 2);
        assert_eq!(V4L2_FIELD_BOTTOM, 3);
        assert_eq!(V4L2_FIELD_INTERLACED, 4);
        assert_eq!(V4L2_FIELD_SEQ_TB, 5);
        assert_eq!(V4L2_FIELD_SEQ_BT, 6);
        assert_eq!(V4L2_FIELD_ALTERNATE, 7);
        assert_eq!(V4L2_FIELD_INTERLACED_TB, 8);
        assert_eq!(V4L2_FIELD_INTERLACED_BT, 9);
    }

    #[test]
    fn test_progressive_is_none() {
        // V4L2_FIELD_NONE is used for progressive (non-interlaced)
        assert_eq!(V4L2_FIELD_NONE, 1);
    }

    #[test]
    fn test_interlaced_variants() {
        // Interlaced without suffix < the TB/BT variants
        assert!(V4L2_FIELD_INTERLACED < V4L2_FIELD_INTERLACED_TB);
        assert!(V4L2_FIELD_INTERLACED_TB < V4L2_FIELD_INTERLACED_BT);
    }
}
