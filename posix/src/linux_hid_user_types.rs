//! `<linux/hid.h>` — Human Interface Device core ABI.
//!
//! HID is the USB/Bluetooth class for keyboards, mice, joysticks,
//! tablets, and any other "report-based" input device. The kernel
//! HID core (`drivers/hid/`) parses report descriptors into the
//! event values consumed by evdev (`/dev/input/event*`). The
//! constants here describe report types, request types, and the
//! topmost levels of the HID usage tree.

// ---------------------------------------------------------------------------
// Class-specific control requests (HID 1.11 §7.2)
// ---------------------------------------------------------------------------

/// `GET_REPORT`.
pub const HID_REQ_GET_REPORT: u8 = 0x01;
/// `GET_IDLE`.
pub const HID_REQ_GET_IDLE: u8 = 0x02;
/// `GET_PROTOCOL` — boot vs report.
pub const HID_REQ_GET_PROTOCOL: u8 = 0x03;
/// `SET_REPORT`.
pub const HID_REQ_SET_REPORT: u8 = 0x09;
/// `SET_IDLE`.
pub const HID_REQ_SET_IDLE: u8 = 0x0A;
/// `SET_PROTOCOL`.
pub const HID_REQ_SET_PROTOCOL: u8 = 0x0B;

// ---------------------------------------------------------------------------
// Report types
// ---------------------------------------------------------------------------

/// Input report (device → host).
pub const HID_INPUT_REPORT: u32 = 0;
/// Output report (host → device).
pub const HID_OUTPUT_REPORT: u32 = 1;
/// Feature report (configuration).
pub const HID_FEATURE_REPORT: u32 = 2;
/// Number of report types.
pub const HID_REPORT_TYPES: u32 = 3;

// ---------------------------------------------------------------------------
// Report descriptor item tag/type encoding (HID 1.11 §6.2.2)
// ---------------------------------------------------------------------------

/// Main item type.
pub const HID_ITEM_TYPE_MAIN: u8 = 0;
/// Global item type.
pub const HID_ITEM_TYPE_GLOBAL: u8 = 1;
/// Local item type.
pub const HID_ITEM_TYPE_LOCAL: u8 = 2;
/// Reserved.
pub const HID_ITEM_TYPE_RESERVED: u8 = 3;

// Main item tags (top 4 bits of the prefix byte).
/// `Input` main item.
pub const HID_MAIN_ITEM_TAG_INPUT: u8 = 8;
/// `Output` main item.
pub const HID_MAIN_ITEM_TAG_OUTPUT: u8 = 9;
/// `Feature` main item.
pub const HID_MAIN_ITEM_TAG_FEATURE: u8 = 11;
/// `Collection`.
pub const HID_MAIN_ITEM_TAG_BEGIN_COLLECTION: u8 = 10;
/// `End Collection`.
pub const HID_MAIN_ITEM_TAG_END_COLLECTION: u8 = 12;

// ---------------------------------------------------------------------------
// Usage Pages (top 16 bits of a 32-bit usage)
// ---------------------------------------------------------------------------

/// Generic Desktop Controls (mouse, keyboard, joystick, gamepad).
pub const HID_USAGE_PAGE_GENERIC_DESKTOP: u32 = 0x0001;
/// Keyboard / Keypad.
pub const HID_USAGE_PAGE_KEYBOARD: u32 = 0x0007;
/// LEDs.
pub const HID_USAGE_PAGE_LEDS: u32 = 0x0008;
/// Button.
pub const HID_USAGE_PAGE_BUTTON: u32 = 0x0009;
/// Consumer (volume, brightness, media keys).
pub const HID_USAGE_PAGE_CONSUMER: u32 = 0x000C;
/// Digitizer (tablet, touch).
pub const HID_USAGE_PAGE_DIGITIZER: u32 = 0x000D;
/// Vendor-defined.
pub const HID_USAGE_PAGE_VENDOR: u32 = 0xFF00;

// ---------------------------------------------------------------------------
// Generic Desktop usage IDs (within page 0x0001)
// ---------------------------------------------------------------------------

/// Pointer (collection).
pub const HID_USAGE_GD_POINTER: u32 = 0x01;
/// Mouse.
pub const HID_USAGE_GD_MOUSE: u32 = 0x02;
/// Joystick.
pub const HID_USAGE_GD_JOYSTICK: u32 = 0x04;
/// Gamepad.
pub const HID_USAGE_GD_GAMEPAD: u32 = 0x05;
/// Keyboard.
pub const HID_USAGE_GD_KEYBOARD: u32 = 0x06;
/// X axis.
pub const HID_USAGE_GD_X: u32 = 0x30;
/// Y axis.
pub const HID_USAGE_GD_Y: u32 = 0x31;
/// Wheel.
pub const HID_USAGE_GD_WHEEL: u32 = 0x38;

// ---------------------------------------------------------------------------
// Protocol (SET_PROTOCOL value)
// ---------------------------------------------------------------------------

/// Boot protocol (PC BIOS-compatible).
pub const HID_BOOT_PROTOCOL: u32 = 0;
/// Report protocol (full descriptor).
pub const HID_REPORT_PROTOCOL: u32 = 1;

// ---------------------------------------------------------------------------
// Misc limits
// ---------------------------------------------------------------------------

/// Maximum HID descriptor size accepted by the kernel parser.
pub const HID_MAX_DESCRIPTOR_SIZE: u32 = 4096;
/// Maximum length of an individual report (kernel-imposed).
pub const HID_MAX_BUFFER_SIZE: u32 = 16384;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_requests_distinct() {
        let r = [
            HID_REQ_GET_REPORT,
            HID_REQ_GET_IDLE,
            HID_REQ_GET_PROTOCOL,
            HID_REQ_SET_REPORT,
            HID_REQ_SET_IDLE,
            HID_REQ_SET_PROTOCOL,
        ];
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
        // SET_* are GET_* | 0x08.
        assert_eq!(HID_REQ_SET_REPORT, HID_REQ_GET_REPORT | 0x08);
        assert_eq!(HID_REQ_SET_IDLE, HID_REQ_GET_IDLE | 0x08);
        assert_eq!(HID_REQ_SET_PROTOCOL, HID_REQ_GET_PROTOCOL | 0x08);
    }

    #[test]
    fn test_report_types_dense() {
        assert_eq!(HID_INPUT_REPORT, 0);
        assert_eq!(HID_OUTPUT_REPORT, 1);
        assert_eq!(HID_FEATURE_REPORT, 2);
        assert_eq!(HID_REPORT_TYPES, 3);
    }

    #[test]
    fn test_item_types_fit_2_bits() {
        for &t in &[
            HID_ITEM_TYPE_MAIN,
            HID_ITEM_TYPE_GLOBAL,
            HID_ITEM_TYPE_LOCAL,
            HID_ITEM_TYPE_RESERVED,
        ] {
            assert!(t < 4);
        }
    }

    #[test]
    fn test_usage_pages_high_word() {
        // Generic Desktop is page 1.
        assert_eq!(HID_USAGE_PAGE_GENERIC_DESKTOP, 1);
        // Vendor-defined range starts at 0xFF00.
        assert_eq!(HID_USAGE_PAGE_VENDOR & 0xFF00, HID_USAGE_PAGE_VENDOR);
    }

    #[test]
    fn test_axes_are_consecutive() {
        // X (0x30), Y (0x31), and Wheel (0x38) live in the same usage page.
        assert_eq!(HID_USAGE_GD_Y - HID_USAGE_GD_X, 1);
        assert!(HID_USAGE_GD_WHEEL > HID_USAGE_GD_Y);
    }

    #[test]
    fn test_protocol_values_distinct() {
        assert_ne!(HID_BOOT_PROTOCOL, HID_REPORT_PROTOCOL);
        assert_eq!(HID_BOOT_PROTOCOL, 0);
        assert_eq!(HID_REPORT_PROTOCOL, 1);
    }

    #[test]
    fn test_buffer_limits_ordered() {
        assert!(HID_MAX_DESCRIPTOR_SIZE < HID_MAX_BUFFER_SIZE);
    }
}
