//! `<linux/hid.h>` — HID usage page constants.
//!
//! HID (Human Interface Device) usage pages define namespaces for
//! input/output controls. Each usage page groups related controls
//! (e.g., keyboard keys, buttons, axes). The host uses these to
//! interpret HID report descriptors and map controls to events.

// ---------------------------------------------------------------------------
// HID usage pages (high 16 bits of 32-bit usage)
// ---------------------------------------------------------------------------

/// Undefined usage page.
pub const HID_UP_UNDEFINED: u32 = 0x0000_0000;
/// Generic Desktop usage page (mouse, keyboard, joystick).
pub const HID_UP_GENDESK: u32 = 0x0001_0000;
/// Simulation Controls (flight sim, driving).
pub const HID_UP_SIMULATION: u32 = 0x0002_0000;
/// VR Controls.
pub const HID_UP_VR: u32 = 0x0003_0000;
/// Sport Controls.
pub const HID_UP_SPORT: u32 = 0x0004_0000;
/// Game Controls.
pub const HID_UP_GAME: u32 = 0x0005_0000;
/// Generic Device Controls.
pub const HID_UP_GENDEVCTRLS: u32 = 0x0006_0000;
/// Keyboard/Keypad usage page.
pub const HID_UP_KEYBOARD: u32 = 0x0007_0000;
/// LED usage page.
pub const HID_UP_LED: u32 = 0x0008_0000;
/// Button usage page.
pub const HID_UP_BUTTON: u32 = 0x0009_0000;
/// Ordinal usage page.
pub const HID_UP_ORDINAL: u32 = 0x000A_0000;
/// Telephony usage page.
pub const HID_UP_TELEPHONY: u32 = 0x000B_0000;
/// Consumer usage page (media keys, volume, etc.).
pub const HID_UP_CONSUMER: u32 = 0x000C_0000;
/// Digitizer usage page (pen, touch).
pub const HID_UP_DIGITIZER: u32 = 0x000D_0000;
/// Physical Interface Device (force feedback).
pub const HID_UP_PID: u32 = 0x000F_0000;
/// Battery System usage page.
pub const HID_UP_BATTERY: u32 = 0x0085_0000;
/// Microsoft vendor usage page.
pub const HID_UP_MSVENDOR: u32 = 0xFF00_0000;

// ---------------------------------------------------------------------------
// Generic Desktop usages (within HID_UP_GENDESK)
// ---------------------------------------------------------------------------

/// Pointer (mouse/touchpad collection).
pub const HID_GD_POINTER: u32 = 0x0001;
/// Mouse.
pub const HID_GD_MOUSE: u32 = 0x0002;
/// Joystick.
pub const HID_GD_JOYSTICK: u32 = 0x0004;
/// Gamepad.
pub const HID_GD_GAMEPAD: u32 = 0x0005;
/// Keyboard.
pub const HID_GD_KEYBOARD: u32 = 0x0006;
/// Keypad (numeric).
pub const HID_GD_KEYPAD: u32 = 0x0007;
/// Multi-axis controller.
pub const HID_GD_MULTIAXIS: u32 = 0x0008;
/// X axis.
pub const HID_GD_X: u32 = 0x0030;
/// Y axis.
pub const HID_GD_Y: u32 = 0x0031;
/// Z axis.
pub const HID_GD_Z: u32 = 0x0032;
/// Wheel (scroll).
pub const HID_GD_WHEEL: u32 = 0x0038;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_pages_distinct() {
        let pages = [
            HID_UP_UNDEFINED, HID_UP_GENDESK, HID_UP_SIMULATION,
            HID_UP_VR, HID_UP_SPORT, HID_UP_GAME,
            HID_UP_GENDEVCTRLS, HID_UP_KEYBOARD, HID_UP_LED,
            HID_UP_BUTTON, HID_UP_ORDINAL, HID_UP_TELEPHONY,
            HID_UP_CONSUMER, HID_UP_DIGITIZER, HID_UP_PID,
            HID_UP_BATTERY, HID_UP_MSVENDOR,
        ];
        for i in 0..pages.len() {
            for j in (i + 1)..pages.len() {
                assert_ne!(pages[i], pages[j]);
            }
        }
    }

    #[test]
    fn test_gendesk_usages_distinct() {
        let usages = [
            HID_GD_POINTER, HID_GD_MOUSE, HID_GD_JOYSTICK,
            HID_GD_GAMEPAD, HID_GD_KEYBOARD, HID_GD_KEYPAD,
            HID_GD_MULTIAXIS, HID_GD_X, HID_GD_Y,
            HID_GD_Z, HID_GD_WHEEL,
        ];
        for i in 0..usages.len() {
            for j in (i + 1)..usages.len() {
                assert_ne!(usages[i], usages[j]);
            }
        }
    }

    #[test]
    fn test_pages_aligned() {
        // Usage pages should be multiples of 0x10000
        assert_eq!(HID_UP_GENDESK & 0xFFFF, 0);
        assert_eq!(HID_UP_KEYBOARD & 0xFFFF, 0);
        assert_eq!(HID_UP_CONSUMER & 0xFFFF, 0);
    }
}
