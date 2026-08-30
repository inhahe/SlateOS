//! `<linux/hid.h>` — Human Interface Device constants.
//!
//! HID is a standard for USB keyboards, mice, game controllers,
//! and other input devices. The Linux HID subsystem parses HID
//! report descriptors and routes events to input, hidraw, and
//! driver-specific handlers.

// ---------------------------------------------------------------------------
// HID report types
// ---------------------------------------------------------------------------

/// Input report (device → host).
pub const HID_INPUT_REPORT: u8 = 0x01;
/// Output report (host → device).
pub const HID_OUTPUT_REPORT: u8 = 0x02;
/// Feature report (bidirectional).
pub const HID_FEATURE_REPORT: u8 = 0x03;

// ---------------------------------------------------------------------------
// HID usage pages
// ---------------------------------------------------------------------------

/// Generic desktop page.
pub const HID_UP_GENDESK: u32 = 0x0001_0000;
/// Simulation controls page.
pub const HID_UP_SIMULATION: u32 = 0x0002_0000;
/// VR controls page.
pub const HID_UP_VR: u32 = 0x0003_0000;
/// Sport controls page.
pub const HID_UP_SPORT: u32 = 0x0004_0000;
/// Game controls page.
pub const HID_UP_GAME: u32 = 0x0005_0000;
/// Generic device controls page.
pub const HID_UP_GENDEVCTRLS: u32 = 0x0006_0000;
/// Keyboard page.
pub const HID_UP_KEYBOARD: u32 = 0x0007_0000;
/// LED page.
pub const HID_UP_LED: u32 = 0x0008_0000;
/// Button page.
pub const HID_UP_BUTTON: u32 = 0x0009_0000;
/// Ordinal page.
pub const HID_UP_ORDINAL: u32 = 0x000A_0000;
/// Telephony page.
pub const HID_UP_TELEPHONY: u32 = 0x000B_0000;
/// Consumer page.
pub const HID_UP_CONSUMER: u32 = 0x000C_0000;
/// Digitizer page.
pub const HID_UP_DIGITIZER: u32 = 0x000D_0000;
/// Sensor page.
pub const HID_UP_SENSOR: u32 = 0x0020_0000;
/// Microsoft vendor page.
pub const HID_UP_MSVENDOR: u32 = 0xFF00_0000;

// ---------------------------------------------------------------------------
// Common HID usages (Generic Desktop)
// ---------------------------------------------------------------------------

/// Pointer.
pub const HID_GD_POINTER: u32 = 0x0001_0001;
/// Mouse.
pub const HID_GD_MOUSE: u32 = 0x0001_0002;
/// Joystick.
pub const HID_GD_JOYSTICK: u32 = 0x0001_0004;
/// Game pad.
pub const HID_GD_GAMEPAD: u32 = 0x0001_0005;
/// Keyboard.
pub const HID_GD_KEYBOARD: u32 = 0x0001_0006;
/// Keypad.
pub const HID_GD_KEYPAD: u32 = 0x0001_0007;
/// Multi-axis controller.
pub const HID_GD_MULTIAXIS: u32 = 0x0001_0008;
/// X axis.
pub const HID_GD_X: u32 = 0x0001_0030;
/// Y axis.
pub const HID_GD_Y: u32 = 0x0001_0031;
/// Z axis.
pub const HID_GD_Z: u32 = 0x0001_0032;
/// Wheel.
pub const HID_GD_WHEEL: u32 = 0x0001_0038;

// ---------------------------------------------------------------------------
// HID bus types
// ---------------------------------------------------------------------------

/// USB bus.
pub const BUS_USB: u16 = 0x03;
/// Bluetooth bus.
pub const BUS_BLUETOOTH: u16 = 0x05;
/// I2C bus.
pub const BUS_I2C: u16 = 0x18;
/// Virtual bus.
pub const BUS_VIRTUAL: u16 = 0x06;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_types_distinct() {
        let types = [HID_INPUT_REPORT, HID_OUTPUT_REPORT, HID_FEATURE_REPORT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_usage_pages_distinct() {
        let pages = [
            HID_UP_GENDESK,
            HID_UP_SIMULATION,
            HID_UP_VR,
            HID_UP_SPORT,
            HID_UP_GAME,
            HID_UP_GENDEVCTRLS,
            HID_UP_KEYBOARD,
            HID_UP_LED,
            HID_UP_BUTTON,
            HID_UP_ORDINAL,
            HID_UP_TELEPHONY,
            HID_UP_CONSUMER,
            HID_UP_DIGITIZER,
            HID_UP_SENSOR,
            HID_UP_MSVENDOR,
        ];
        for i in 0..pages.len() {
            for j in (i + 1)..pages.len() {
                assert_ne!(pages[i], pages[j]);
            }
        }
    }

    #[test]
    fn test_gd_usages_distinct() {
        let usages = [
            HID_GD_POINTER,
            HID_GD_MOUSE,
            HID_GD_JOYSTICK,
            HID_GD_GAMEPAD,
            HID_GD_KEYBOARD,
            HID_GD_KEYPAD,
            HID_GD_MULTIAXIS,
            HID_GD_X,
            HID_GD_Y,
            HID_GD_Z,
            HID_GD_WHEEL,
        ];
        for i in 0..usages.len() {
            for j in (i + 1)..usages.len() {
                assert_ne!(usages[i], usages[j]);
            }
        }
    }

    #[test]
    fn test_bus_types_distinct() {
        let buses = [BUS_USB, BUS_BLUETOOTH, BUS_I2C, BUS_VIRTUAL];
        for i in 0..buses.len() {
            for j in (i + 1)..buses.len() {
                assert_ne!(buses[i], buses[j]);
            }
        }
    }

    #[test]
    fn test_report_type_values() {
        assert_eq!(HID_INPUT_REPORT, 1);
        assert_eq!(HID_OUTPUT_REPORT, 2);
        assert_eq!(HID_FEATURE_REPORT, 3);
    }
}
