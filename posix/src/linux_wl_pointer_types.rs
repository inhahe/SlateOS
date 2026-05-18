//! Wayland `wl_pointer` — pointer event type constants.
//!
//! The `wl_pointer` interface handles mouse/trackpad events: motion,
//! button press/release, scroll (axis), and cursor shape. Events are
//! framed between `wl_pointer.frame` boundaries to group simultaneous
//! changes atomically.

// ---------------------------------------------------------------------------
// Button state (wl_pointer.button_state)
// ---------------------------------------------------------------------------

/// Button is released.
pub const WL_POINTER_BUTTON_STATE_RELEASED: u32 = 0;
/// Button is pressed.
pub const WL_POINTER_BUTTON_STATE_PRESSED: u32 = 1;

// ---------------------------------------------------------------------------
// Axis (wl_pointer.axis)
// ---------------------------------------------------------------------------

/// Vertical scroll.
pub const WL_POINTER_AXIS_VERTICAL_SCROLL: u32 = 0;
/// Horizontal scroll.
pub const WL_POINTER_AXIS_HORIZONTAL_SCROLL: u32 = 1;

// ---------------------------------------------------------------------------
// Axis source (wl_pointer.axis_source)
// ---------------------------------------------------------------------------

/// Continuous scroll (trackpad, touchpad).
pub const WL_POINTER_AXIS_SOURCE_WHEEL: u32 = 0;
/// Finger on touchpad.
pub const WL_POINTER_AXIS_SOURCE_FINGER: u32 = 1;
/// Continuous axis (e.g. dial).
pub const WL_POINTER_AXIS_SOURCE_CONTINUOUS: u32 = 2;
/// High-resolution scroll wheel tilt.
pub const WL_POINTER_AXIS_SOURCE_WHEEL_TILT: u32 = 3;

// ---------------------------------------------------------------------------
// Axis relative direction (wl_pointer.axis_relative_direction)
// ---------------------------------------------------------------------------

/// Identical to physical motion direction.
pub const WL_POINTER_AXIS_RELATIVE_DIRECTION_IDENTICAL: u32 = 0;
/// Inverted from physical motion (natural scrolling).
pub const WL_POINTER_AXIS_RELATIVE_DIRECTION_INVERTED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_states_distinct() {
        assert_ne!(WL_POINTER_BUTTON_STATE_RELEASED,
                   WL_POINTER_BUTTON_STATE_PRESSED);
    }

    #[test]
    fn test_axes_distinct() {
        assert_ne!(WL_POINTER_AXIS_VERTICAL_SCROLL,
                   WL_POINTER_AXIS_HORIZONTAL_SCROLL);
    }

    #[test]
    fn test_axis_sources_distinct() {
        let sources = [
            WL_POINTER_AXIS_SOURCE_WHEEL,
            WL_POINTER_AXIS_SOURCE_FINGER,
            WL_POINTER_AXIS_SOURCE_CONTINUOUS,
            WL_POINTER_AXIS_SOURCE_WHEEL_TILT,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }

    #[test]
    fn test_relative_directions() {
        assert_ne!(WL_POINTER_AXIS_RELATIVE_DIRECTION_IDENTICAL,
                   WL_POINTER_AXIS_RELATIVE_DIRECTION_INVERTED);
    }

    #[test]
    fn test_values() {
        assert_eq!(WL_POINTER_BUTTON_STATE_RELEASED, 0);
        assert_eq!(WL_POINTER_BUTTON_STATE_PRESSED, 1);
    }
}
