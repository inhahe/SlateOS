//! Wayland `wl_output` — display output constants.
//!
//! `wl_output` represents a physical display connected to the
//! compositor. It reports the display's geometry (position, size,
//! physical dimensions), supported modes (resolution, refresh rate),
//! and current transform (rotation, mirroring).

// ---------------------------------------------------------------------------
// Output subpixel layout (wl_output.subpixel)
// ---------------------------------------------------------------------------

/// Unknown subpixel layout.
pub const WL_OUTPUT_SUBPIXEL_UNKNOWN: u32 = 0;
/// No subpixels (e.g., projector, CRT).
pub const WL_OUTPUT_SUBPIXEL_NONE: u32 = 1;
/// Horizontal RGB subpixels.
pub const WL_OUTPUT_SUBPIXEL_HORIZONTAL_RGB: u32 = 2;
/// Horizontal BGR subpixels.
pub const WL_OUTPUT_SUBPIXEL_HORIZONTAL_BGR: u32 = 3;
/// Vertical RGB subpixels.
pub const WL_OUTPUT_SUBPIXEL_VERTICAL_RGB: u32 = 4;
/// Vertical BGR subpixels.
pub const WL_OUTPUT_SUBPIXEL_VERTICAL_BGR: u32 = 5;

// ---------------------------------------------------------------------------
// Output transform (wl_output.transform)
// ---------------------------------------------------------------------------

/// No transform (normal orientation).
pub const WL_OUTPUT_TRANSFORM_NORMAL: u32 = 0;
/// 90 degrees clockwise.
pub const WL_OUTPUT_TRANSFORM_90: u32 = 1;
/// 180 degrees (upside down).
pub const WL_OUTPUT_TRANSFORM_180: u32 = 2;
/// 270 degrees clockwise.
pub const WL_OUTPUT_TRANSFORM_270: u32 = 3;
/// Mirrored (horizontal flip).
pub const WL_OUTPUT_TRANSFORM_FLIPPED: u32 = 4;
/// Mirrored + 90 degrees.
pub const WL_OUTPUT_TRANSFORM_FLIPPED_90: u32 = 5;
/// Mirrored + 180 degrees.
pub const WL_OUTPUT_TRANSFORM_FLIPPED_180: u32 = 6;
/// Mirrored + 270 degrees.
pub const WL_OUTPUT_TRANSFORM_FLIPPED_270: u32 = 7;

// ---------------------------------------------------------------------------
// Output mode flags (wl_output.mode.flags)
// ---------------------------------------------------------------------------

/// Mode is the current active mode.
pub const WL_OUTPUT_MODE_CURRENT: u32 = 0x1;
/// Mode is the preferred (native) mode.
pub const WL_OUTPUT_MODE_PREFERRED: u32 = 0x2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subpixel_distinct() {
        let subs = [
            WL_OUTPUT_SUBPIXEL_UNKNOWN,
            WL_OUTPUT_SUBPIXEL_NONE,
            WL_OUTPUT_SUBPIXEL_HORIZONTAL_RGB,
            WL_OUTPUT_SUBPIXEL_HORIZONTAL_BGR,
            WL_OUTPUT_SUBPIXEL_VERTICAL_RGB,
            WL_OUTPUT_SUBPIXEL_VERTICAL_BGR,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_transforms_distinct() {
        let xforms = [
            WL_OUTPUT_TRANSFORM_NORMAL,
            WL_OUTPUT_TRANSFORM_90,
            WL_OUTPUT_TRANSFORM_180,
            WL_OUTPUT_TRANSFORM_270,
            WL_OUTPUT_TRANSFORM_FLIPPED,
            WL_OUTPUT_TRANSFORM_FLIPPED_90,
            WL_OUTPUT_TRANSFORM_FLIPPED_180,
            WL_OUTPUT_TRANSFORM_FLIPPED_270,
        ];
        for i in 0..xforms.len() {
            for j in (i + 1)..xforms.len() {
                assert_ne!(xforms[i], xforms[j]);
            }
        }
    }

    #[test]
    fn test_transforms_sequential() {
        assert_eq!(WL_OUTPUT_TRANSFORM_NORMAL, 0);
        assert_eq!(WL_OUTPUT_TRANSFORM_FLIPPED_270, 7);
    }

    #[test]
    fn test_mode_flags_no_overlap() {
        assert!(WL_OUTPUT_MODE_CURRENT.is_power_of_two());
        assert!(WL_OUTPUT_MODE_PREFERRED.is_power_of_two());
        assert_eq!(WL_OUTPUT_MODE_CURRENT & WL_OUTPUT_MODE_PREFERRED, 0);
    }
}
