//! Wayland `wl_seat` — input seat capability constants.
//!
//! A seat represents a group of input devices (keyboard, pointer,
//! touch) that belong together — typically one per user workstation.
//! Capabilities report which device types are currently available
//! on the seat.

// ---------------------------------------------------------------------------
// Seat capabilities (wl_seat.capabilities)
// ---------------------------------------------------------------------------

/// Seat has a pointer device (mouse, trackpad).
pub const WL_SEAT_CAPABILITY_POINTER: u32 = 1;
/// Seat has a keyboard.
pub const WL_SEAT_CAPABILITY_KEYBOARD: u32 = 2;
/// Seat has a touch device (touchscreen).
pub const WL_SEAT_CAPABILITY_TOUCH: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_no_overlap() {
        let caps = [
            WL_SEAT_CAPABILITY_POINTER,
            WL_SEAT_CAPABILITY_KEYBOARD,
            WL_SEAT_CAPABILITY_TOUCH,
        ];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_capabilities_composable() {
        let full = WL_SEAT_CAPABILITY_POINTER
            | WL_SEAT_CAPABILITY_KEYBOARD
            | WL_SEAT_CAPABILITY_TOUCH;
        assert_ne!(full & WL_SEAT_CAPABILITY_POINTER, 0);
        assert_ne!(full & WL_SEAT_CAPABILITY_KEYBOARD, 0);
        assert_ne!(full & WL_SEAT_CAPABILITY_TOUCH, 0);
    }

    #[test]
    fn test_capability_values() {
        assert_eq!(WL_SEAT_CAPABILITY_POINTER, 1);
        assert_eq!(WL_SEAT_CAPABILITY_KEYBOARD, 2);
        assert_eq!(WL_SEAT_CAPABILITY_TOUCH, 4);
    }
}
