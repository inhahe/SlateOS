//! `<linux/input-event-codes.h>` (REL subset) — relative axis event codes.
//!
//! Relative axis events report displacement rather than absolute
//! position. Mice are the primary producers: each report describes
//! how far the pointer moved since the last sample, not where it is.
//! Scroll wheels, trackballs, and dial controllers also generate
//! relative events.

// ---------------------------------------------------------------------------
// Relative axis codes
// ---------------------------------------------------------------------------

/// Relative X movement (horizontal, positive = right).
pub const REL_X: u16 = 0x00;
/// Relative Y movement (vertical, positive = down).
pub const REL_Y: u16 = 0x01;
/// Relative Z movement (rarely used, 3D mice).
pub const REL_Z: u16 = 0x02;
/// Relative rotation around X axis.
pub const REL_RX: u16 = 0x03;
/// Relative rotation around Y axis.
pub const REL_RY: u16 = 0x04;
/// Relative rotation around Z axis.
pub const REL_RZ: u16 = 0x05;
/// Horizontal scroll wheel.
pub const REL_HWHEEL: u16 = 0x06;
/// Dial (rotary controller).
pub const REL_DIAL: u16 = 0x07;
/// Vertical scroll wheel.
pub const REL_WHEEL: u16 = 0x08;
/// Miscellaneous relative axis.
pub const REL_MISC: u16 = 0x09;
/// Reserved / unused.
pub const REL_RESERVED: u16 = 0x0A;
/// High-resolution vertical scroll wheel.
pub const REL_WHEEL_HI_RES: u16 = 0x0B;
/// High-resolution horizontal scroll wheel.
pub const REL_HWHEEL_HI_RES: u16 = 0x0C;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum relative axis code.
pub const REL_MAX: u16 = 0x0F;
/// Number of relative axis codes (REL_MAX + 1).
pub const REL_CNT: u16 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rel_axes_distinct() {
        let axes = [
            REL_X, REL_Y, REL_Z,
            REL_RX, REL_RY, REL_RZ,
            REL_HWHEEL, REL_DIAL, REL_WHEEL,
            REL_MISC, REL_RESERVED,
            REL_WHEEL_HI_RES, REL_HWHEEL_HI_RES,
        ];
        for i in 0..axes.len() {
            for j in (i + 1)..axes.len() {
                assert_ne!(axes[i], axes[j],
                    "rel axes {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_rel_xy_are_first() {
        assert_eq!(REL_X, 0);
        assert_eq!(REL_Y, 1);
    }

    #[test]
    fn test_rotation_sequential() {
        assert_eq!(REL_RX, REL_RY - 1);
        assert_eq!(REL_RY, REL_RZ - 1);
    }

    #[test]
    fn test_hires_after_standard() {
        assert!(REL_WHEEL_HI_RES > REL_WHEEL);
        assert!(REL_HWHEEL_HI_RES > REL_HWHEEL);
    }

    #[test]
    fn test_all_within_max() {
        let axes = [
            REL_X, REL_Y, REL_Z,
            REL_RX, REL_RY, REL_RZ,
            REL_HWHEEL, REL_DIAL, REL_WHEEL,
            REL_MISC, REL_RESERVED,
            REL_WHEEL_HI_RES, REL_HWHEEL_HI_RES,
        ];
        for &a in &axes {
            assert!(a <= REL_MAX, "axis {} exceeds REL_MAX", a);
        }
    }

    #[test]
    fn test_rel_cnt() {
        assert_eq!(REL_CNT, REL_MAX + 1);
    }
}
