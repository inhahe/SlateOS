//! `<linux/gameport.h>` — Gameport (legacy joystick port) constants.
//!
//! The gameport subsystem handles legacy ISA game ports (0x201)
//! used by analog joysticks and MIDI devices. While largely
//! superseded by USB HID, gameport support remains for older
//! hardware. The driver registers a gameport device and the
//! input layer reads axis/button state via timed I/O reads.

// ---------------------------------------------------------------------------
// Gameport I/O ports
// ---------------------------------------------------------------------------

/// Standard gameport base I/O address.
pub const GAMEPORT_IO_BASE: u16 = 0x201;

// ---------------------------------------------------------------------------
// Gameport modes
// ---------------------------------------------------------------------------

/// Raw mode (direct I/O reads for analog axes).
pub const GAMEPORT_MODE_RAW: u32 = 0;
/// Cooked mode (driver provides calibrated values).
pub const GAMEPORT_MODE_COOKED: u32 = 1;

// ---------------------------------------------------------------------------
// Gameport axis/button counts
// ---------------------------------------------------------------------------

/// Maximum number of analog axes per gameport.
pub const GAMEPORT_NUM_AXES: u32 = 4;
/// Maximum number of buttons per gameport.
pub const GAMEPORT_NUM_BUTTONS: u32 = 4;

// ---------------------------------------------------------------------------
// Gameport axis indices
// ---------------------------------------------------------------------------

/// X axis (axis 0).
pub const GAMEPORT_AXIS_X: u32 = 0;
/// Y axis (axis 1).
pub const GAMEPORT_AXIS_Y: u32 = 1;
/// Rudder / throttle (axis 2).
pub const GAMEPORT_AXIS_RX: u32 = 2;
/// Throttle / Z (axis 3).
pub const GAMEPORT_AXIS_RY: u32 = 3;

// ---------------------------------------------------------------------------
// Gameport button bits (active low in hardware)
// ---------------------------------------------------------------------------

/// Button 1.
pub const GAMEPORT_BTN_1: u32 = 1 << 4;
/// Button 2.
pub const GAMEPORT_BTN_2: u32 = 1 << 5;
/// Button 3.
pub const GAMEPORT_BTN_3: u32 = 1 << 6;
/// Button 4.
pub const GAMEPORT_BTN_4: u32 = 1 << 7;

/// Button bit mask (bits 4–7 of the status byte).
pub const GAMEPORT_BTN_MASK: u32 =
    GAMEPORT_BTN_1 | GAMEPORT_BTN_2 | GAMEPORT_BTN_3 | GAMEPORT_BTN_4;

// ---------------------------------------------------------------------------
// Gameport status bits
// ---------------------------------------------------------------------------

/// Axis 0 timing bit.
pub const GAMEPORT_STATUS_AX0: u32 = 1 << 0;
/// Axis 1 timing bit.
pub const GAMEPORT_STATUS_AX1: u32 = 1 << 1;
/// Axis 2 timing bit.
pub const GAMEPORT_STATUS_AX2: u32 = 1 << 2;
/// Axis 3 timing bit.
pub const GAMEPORT_STATUS_AX3: u32 = 1 << 3;

/// Axis timing bit mask (bits 0–3).
pub const GAMEPORT_STATUS_AXES_MASK: u32 =
    GAMEPORT_STATUS_AX0 | GAMEPORT_STATUS_AX1
    | GAMEPORT_STATUS_AX2 | GAMEPORT_STATUS_AX3;

// ---------------------------------------------------------------------------
// Calibration
// ---------------------------------------------------------------------------

/// Default timeout for axis read (microseconds).
pub const GAMEPORT_CALIBRATE_TIMEOUT_US: u32 = 50_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        assert_ne!(GAMEPORT_MODE_RAW, GAMEPORT_MODE_COOKED);
    }

    #[test]
    fn test_axis_indices_distinct() {
        let axes = [
            GAMEPORT_AXIS_X, GAMEPORT_AXIS_Y,
            GAMEPORT_AXIS_RX, GAMEPORT_AXIS_RY,
        ];
        for i in 0..axes.len() {
            for j in (i + 1)..axes.len() {
                assert_ne!(axes[i], axes[j]);
            }
        }
    }

    #[test]
    fn test_axis_indices_in_range() {
        assert!(GAMEPORT_AXIS_X < GAMEPORT_NUM_AXES);
        assert!(GAMEPORT_AXIS_Y < GAMEPORT_NUM_AXES);
        assert!(GAMEPORT_AXIS_RX < GAMEPORT_NUM_AXES);
        assert!(GAMEPORT_AXIS_RY < GAMEPORT_NUM_AXES);
    }

    #[test]
    fn test_button_bits_powers_of_two() {
        let btns = [
            GAMEPORT_BTN_1, GAMEPORT_BTN_2,
            GAMEPORT_BTN_3, GAMEPORT_BTN_4,
        ];
        for btn in &btns {
            assert!(btn.is_power_of_two(), "0x{:x}", btn);
        }
    }

    #[test]
    fn test_button_bits_no_overlap() {
        let btns = [
            GAMEPORT_BTN_1, GAMEPORT_BTN_2,
            GAMEPORT_BTN_3, GAMEPORT_BTN_4,
        ];
        for i in 0..btns.len() {
            for j in (i + 1)..btns.len() {
                assert_eq!(btns[i] & btns[j], 0);
            }
        }
    }

    #[test]
    fn test_button_mask() {
        assert_eq!(GAMEPORT_BTN_MASK, 0xF0);
    }

    #[test]
    fn test_status_bits_powers_of_two() {
        let bits = [
            GAMEPORT_STATUS_AX0, GAMEPORT_STATUS_AX1,
            GAMEPORT_STATUS_AX2, GAMEPORT_STATUS_AX3,
        ];
        for bit in &bits {
            assert!(bit.is_power_of_two(), "0x{:x}", bit);
        }
    }

    #[test]
    fn test_status_bits_no_overlap() {
        let bits = [
            GAMEPORT_STATUS_AX0, GAMEPORT_STATUS_AX1,
            GAMEPORT_STATUS_AX2, GAMEPORT_STATUS_AX3,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_status_axes_mask() {
        assert_eq!(GAMEPORT_STATUS_AXES_MASK, 0x0F);
    }

    #[test]
    fn test_buttons_and_axes_no_overlap() {
        assert_eq!(GAMEPORT_BTN_MASK & GAMEPORT_STATUS_AXES_MASK, 0);
    }

    #[test]
    fn test_calibrate_timeout() {
        assert!(GAMEPORT_CALIBRATE_TIMEOUT_US > 0);
    }
}
