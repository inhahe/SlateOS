//! `<linux/serdev.h>` — Serial device bus (serdev) constants.
//!
//! Serdev provides a proper device model for devices connected via
//! serial ports (UART). Before serdev, serial-attached devices
//! (Bluetooth HCI, GPS, touch controllers) used userspace TTY
//! access or brittle platform data. Serdev creates a bus with
//! proper probe/remove lifecycle, DT/ACPI matching, and direct
//! kernel access to the serial port without TTY layer overhead.

// ---------------------------------------------------------------------------
// Serdev device types
// ---------------------------------------------------------------------------

/// Generic serial device.
pub const SERDEV_TYPE_GENERIC: u32 = 0;
/// Bluetooth HCI UART device.
pub const SERDEV_TYPE_BT_HCI: u32 = 1;
/// GPS/GNSS device.
pub const SERDEV_TYPE_GPS: u32 = 2;
/// Touch controller.
pub const SERDEV_TYPE_TOUCH: u32 = 3;
/// NFC device.
pub const SERDEV_TYPE_NFC: u32 = 4;

// ---------------------------------------------------------------------------
// Serdev controller flags
// ---------------------------------------------------------------------------

/// Controller supports hardware flow control (RTS/CTS).
pub const SERDEV_FL_FLOW_CONTROL: u32 = 1 << 0;
/// Controller is registered.
pub const SERDEV_FL_REGISTERED: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Serdev parity
// ---------------------------------------------------------------------------

/// No parity.
pub const SERDEV_PARITY_NONE: u32 = 0;
/// Odd parity.
pub const SERDEV_PARITY_ODD: u32 = 1;
/// Even parity.
pub const SERDEV_PARITY_EVEN: u32 = 2;

// ---------------------------------------------------------------------------
// Serdev common baud rates
// ---------------------------------------------------------------------------

/// 9600 baud.
pub const SERDEV_BAUD_9600: u32 = 9600;
/// 19200 baud.
pub const SERDEV_BAUD_19200: u32 = 19200;
/// 38400 baud.
pub const SERDEV_BAUD_38400: u32 = 38400;
/// 57600 baud.
pub const SERDEV_BAUD_57600: u32 = 57600;
/// 115200 baud.
pub const SERDEV_BAUD_115200: u32 = 115200;
/// 230400 baud.
pub const SERDEV_BAUD_230400: u32 = 230400;
/// 460800 baud.
pub const SERDEV_BAUD_460800: u32 = 460800;
/// 921600 baud.
pub const SERDEV_BAUD_921600: u32 = 921600;
/// 1000000 baud (1 Mbps).
pub const SERDEV_BAUD_1000000: u32 = 1_000_000;
/// 3000000 baud (3 Mbps).
pub const SERDEV_BAUD_3000000: u32 = 3_000_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types = [
            SERDEV_TYPE_GENERIC,
            SERDEV_TYPE_BT_HCI,
            SERDEV_TYPE_GPS,
            SERDEV_TYPE_TOUCH,
            SERDEV_TYPE_NFC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_controller_flags_no_overlap() {
        let flags = [SERDEV_FL_FLOW_CONTROL, SERDEV_FL_REGISTERED];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_parity_distinct() {
        let pars = [SERDEV_PARITY_NONE, SERDEV_PARITY_ODD, SERDEV_PARITY_EVEN];
        for i in 0..pars.len() {
            for j in (i + 1)..pars.len() {
                assert_ne!(pars[i], pars[j]);
            }
        }
    }

    #[test]
    fn test_baud_rates_ordered() {
        let rates = [
            SERDEV_BAUD_9600,
            SERDEV_BAUD_19200,
            SERDEV_BAUD_38400,
            SERDEV_BAUD_57600,
            SERDEV_BAUD_115200,
            SERDEV_BAUD_230400,
            SERDEV_BAUD_460800,
            SERDEV_BAUD_921600,
            SERDEV_BAUD_1000000,
            SERDEV_BAUD_3000000,
        ];
        for i in 0..rates.len() - 1 {
            assert!(rates[i] < rates[i + 1]);
        }
    }
}
