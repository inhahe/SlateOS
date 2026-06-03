//! `<linux/usb/gadget.h>` — USB gadget (device-side) constants.
//!
//! The USB gadget framework lets Linux act as a USB device (function)
//! rather than a host. This is used in embedded systems, phones, and
//! development boards to present USB functions like mass storage,
//! serial ports (CDC ACM), network (RNDIS/ECM), or custom vendor
//! protocols. ConfigFS-based gadget composition allows mixing multiple
//! functions in a single composite device at runtime.

// ---------------------------------------------------------------------------
// Gadget speeds
// ---------------------------------------------------------------------------

/// Gadget doesn't know its speed yet.
pub const USB_GADGET_SPEED_UNKNOWN: u32 = 0;
/// Low speed (1.5 Mbps).
pub const USB_GADGET_SPEED_LOW: u32 = 1;
/// Full speed (12 Mbps).
pub const USB_GADGET_SPEED_FULL: u32 = 2;
/// High speed (480 Mbps).
pub const USB_GADGET_SPEED_HIGH: u32 = 3;
/// SuperSpeed (5 Gbps).
pub const USB_GADGET_SPEED_SUPER: u32 = 5;
/// SuperSpeed+ (10+ Gbps).
pub const USB_GADGET_SPEED_SUPER_PLUS: u32 = 6;

// ---------------------------------------------------------------------------
// Common gadget function types
// ---------------------------------------------------------------------------

/// Mass storage function (UMS).
pub const USB_FUNC_MASS_STORAGE: u32 = 0;
/// CDC ACM (serial port / modem).
pub const USB_FUNC_ACM: u32 = 1;
/// CDC ECM (Ethernet over USB).
pub const USB_FUNC_ECM: u32 = 2;
/// RNDIS (Windows-compatible networking).
pub const USB_FUNC_RNDIS: u32 = 3;
/// HID (keyboard/mouse emulation).
pub const USB_FUNC_HID: u32 = 4;
/// MIDI.
pub const USB_FUNC_MIDI: u32 = 5;
/// UAC (USB Audio Class).
pub const USB_FUNC_UAC: u32 = 6;
/// UVC (USB Video Class).
pub const USB_FUNC_UVC: u32 = 7;
/// Printer.
pub const USB_FUNC_PRINTER: u32 = 8;
/// Fastboot (Android bootloader protocol).
pub const USB_FUNC_FASTBOOT: u32 = 9;

// ---------------------------------------------------------------------------
// Gadget states
// ---------------------------------------------------------------------------

/// Gadget is not bound to a UDC (USB Device Controller).
pub const USB_GADGET_STATE_UNBOUND: u32 = 0;
/// Gadget is bound but not connected.
pub const USB_GADGET_STATE_NOT_ATTACHED: u32 = 1;
/// Gadget is attached (VBUS detected).
pub const USB_GADGET_STATE_ATTACHED: u32 = 2;
/// Gadget is in default state (address 0).
pub const USB_GADGET_STATE_DEFAULT: u32 = 3;
/// Gadget has been addressed (SET_ADDRESS complete).
pub const USB_GADGET_STATE_ADDRESSED: u32 = 4;
/// Gadget is configured (SET_CONFIGURATION complete).
pub const USB_GADGET_STATE_CONFIGURED: u32 = 5;
/// Gadget is suspended.
pub const USB_GADGET_STATE_SUSPENDED: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speeds_distinct() {
        let speeds = [
            USB_GADGET_SPEED_UNKNOWN,
            USB_GADGET_SPEED_LOW,
            USB_GADGET_SPEED_FULL,
            USB_GADGET_SPEED_HIGH,
            USB_GADGET_SPEED_SUPER,
            USB_GADGET_SPEED_SUPER_PLUS,
        ];
        for i in 0..speeds.len() {
            for j in (i + 1)..speeds.len() {
                assert_ne!(speeds[i], speeds[j]);
            }
        }
    }

    #[test]
    fn test_functions_distinct() {
        let funcs = [
            USB_FUNC_MASS_STORAGE,
            USB_FUNC_ACM,
            USB_FUNC_ECM,
            USB_FUNC_RNDIS,
            USB_FUNC_HID,
            USB_FUNC_MIDI,
            USB_FUNC_UAC,
            USB_FUNC_UVC,
            USB_FUNC_PRINTER,
            USB_FUNC_FASTBOOT,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            USB_GADGET_STATE_UNBOUND,
            USB_GADGET_STATE_NOT_ATTACHED,
            USB_GADGET_STATE_ATTACHED,
            USB_GADGET_STATE_DEFAULT,
            USB_GADGET_STATE_ADDRESSED,
            USB_GADGET_STATE_CONFIGURED,
            USB_GADGET_STATE_SUSPENDED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
