//! `<linux/hid.h>` — HID (Human Interface Device) constants.
//!
//! HID is the protocol used by USB keyboards, mice, gamepads,
//! and other input devices. The Linux HID subsystem parses HID
//! report descriptors and exposes devices through the input layer.

// ---------------------------------------------------------------------------
// HID report types
// ---------------------------------------------------------------------------

/// Input report (device → host).
pub const HID_INPUT_REPORT: u8 = 1;
/// Output report (host → device).
pub const HID_OUTPUT_REPORT: u8 = 2;
/// Feature report (bidirectional, configuration).
pub const HID_FEATURE_REPORT: u8 = 3;

// ---------------------------------------------------------------------------
// HID usage pages
// ---------------------------------------------------------------------------

/// Generic Desktop page.
pub const HID_UP_GENDESK: u16 = 0x0001;
/// Simulation Controls.
pub const HID_UP_SIMULATION: u16 = 0x0002;
/// VR Controls.
pub const HID_UP_VR: u16 = 0x0003;
/// Keyboard/Keypad page.
pub const HID_UP_KEYBOARD: u16 = 0x0007;
/// LED page.
pub const HID_UP_LED: u16 = 0x0008;
/// Button page.
pub const HID_UP_BUTTON: u16 = 0x0009;
/// Consumer page (media keys).
pub const HID_UP_CONSUMER: u16 = 0x000C;
/// Digitizer page (touchscreen/stylus).
pub const HID_UP_DIGITIZER: u16 = 0x000D;
/// Sensor page.
pub const HID_UP_SENSOR: u16 = 0x0020;

// ---------------------------------------------------------------------------
// Generic Desktop usages
// ---------------------------------------------------------------------------

/// Pointer.
pub const HID_GD_POINTER: u16 = 0x01;
/// Mouse.
pub const HID_GD_MOUSE: u16 = 0x02;
/// Joystick.
pub const HID_GD_JOYSTICK: u16 = 0x04;
/// Gamepad.
pub const HID_GD_GAMEPAD: u16 = 0x05;
/// Keyboard.
pub const HID_GD_KEYBOARD: u16 = 0x06;
/// Keypad.
pub const HID_GD_KEYPAD: u16 = 0x07;
/// X axis.
pub const HID_GD_X: u16 = 0x30;
/// Y axis.
pub const HID_GD_Y: u16 = 0x31;
/// Z axis.
pub const HID_GD_Z: u16 = 0x32;
/// Wheel.
pub const HID_GD_WHEEL: u16 = 0x38;

// ---------------------------------------------------------------------------
// HID device quirk flags
// ---------------------------------------------------------------------------

/// Invert horizontal axis.
pub const HID_QUIRK_INVERT: u32 = 1 << 0;
/// No init reports.
pub const HID_QUIRK_NOTOUCH: u32 = 1 << 1;
/// Ignore device.
pub const HID_QUIRK_IGNORE: u32 = 1 << 2;
/// No auto-open on input.
pub const HID_QUIRK_NOGET: u32 = 1 << 3;
/// Always poll device.
pub const HID_QUIRK_ALWAYS_POLL: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// HID bus types
// ---------------------------------------------------------------------------

/// USB transport.
pub const BUS_USB: u16 = 0x03;
/// Bluetooth transport.
pub const BUS_BLUETOOTH: u16 = 0x05;
/// I2C transport (i2c-hid).
pub const BUS_I2C: u16 = 0x18;
/// SPI transport.
pub const BUS_SPI: u16 = 0x1C;

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
            HID_UP_GENDESK, HID_UP_SIMULATION, HID_UP_VR,
            HID_UP_KEYBOARD, HID_UP_LED, HID_UP_BUTTON,
            HID_UP_CONSUMER, HID_UP_DIGITIZER, HID_UP_SENSOR,
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
            HID_GD_POINTER, HID_GD_MOUSE, HID_GD_JOYSTICK,
            HID_GD_GAMEPAD, HID_GD_KEYBOARD, HID_GD_KEYPAD,
            HID_GD_X, HID_GD_Y, HID_GD_Z, HID_GD_WHEEL,
        ];
        for i in 0..usages.len() {
            for j in (i + 1)..usages.len() {
                assert_ne!(usages[i], usages[j]);
            }
        }
    }

    #[test]
    fn test_bus_types_distinct() {
        let buses = [BUS_USB, BUS_BLUETOOTH, BUS_I2C, BUS_SPI];
        for i in 0..buses.len() {
            for j in (i + 1)..buses.len() {
                assert_ne!(buses[i], buses[j]);
            }
        }
    }
}
