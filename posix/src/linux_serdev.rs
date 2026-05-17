//! `<linux/serdev.h>` — Serial device bus constants.
//!
//! The serdev (serial device) bus framework provides a structured
//! way to attach and communicate with devices on serial/UART ports.
//! Unlike the legacy tty layer, serdev gives drivers first-class
//! bus semantics (probe/remove, power management) for Bluetooth
//! chips, GPS modules, NFC controllers, and other UART-attached
//! peripherals.

// ---------------------------------------------------------------------------
// Parity modes
// ---------------------------------------------------------------------------

/// No parity bit.
pub const SERDEV_PARITY_NONE: u8 = 0;
/// Odd parity.
pub const SERDEV_PARITY_ODD: u8 = 1;
/// Even parity.
pub const SERDEV_PARITY_EVEN: u8 = 2;

// ---------------------------------------------------------------------------
// Flow control
// ---------------------------------------------------------------------------

/// No flow control.
pub const SERDEV_FLOW_NONE: u8 = 0;
/// Hardware flow control (RTS/CTS).
pub const SERDEV_FLOW_RTSCTS: u8 = 1;
/// Software flow control (XON/XOFF).
pub const SERDEV_FLOW_XONXOFF: u8 = 2;

// ---------------------------------------------------------------------------
// Standard baud rates
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
/// 1000000 baud (1 Mbaud).
pub const SERDEV_BAUD_1000000: u32 = 1_000_000;
/// 1500000 baud (1.5 Mbaud).
pub const SERDEV_BAUD_1500000: u32 = 1_500_000;
/// 2000000 baud (2 Mbaud).
pub const SERDEV_BAUD_2000000: u32 = 2_000_000;
/// 3000000 baud (3 Mbaud).
pub const SERDEV_BAUD_3000000: u32 = 3_000_000;
/// 4000000 baud (4 Mbaud).
pub const SERDEV_BAUD_4000000: u32 = 4_000_000;

// ---------------------------------------------------------------------------
// Data bits
// ---------------------------------------------------------------------------

/// 5 data bits.
pub const SERDEV_DATABITS_5: u8 = 5;
/// 6 data bits.
pub const SERDEV_DATABITS_6: u8 = 6;
/// 7 data bits.
pub const SERDEV_DATABITS_7: u8 = 7;
/// 8 data bits (most common).
pub const SERDEV_DATABITS_8: u8 = 8;

// ---------------------------------------------------------------------------
// Stop bits
// ---------------------------------------------------------------------------

/// 1 stop bit.
pub const SERDEV_STOPBITS_1: u8 = 1;
/// 2 stop bits.
pub const SERDEV_STOPBITS_2: u8 = 2;

// ---------------------------------------------------------------------------
// Modem control lines
// ---------------------------------------------------------------------------

/// Data Terminal Ready.
pub const SERDEV_TIOCM_DTR: u32 = 1 << 0;
/// Request To Send.
pub const SERDEV_TIOCM_RTS: u32 = 1 << 1;
/// Data Set Ready.
pub const SERDEV_TIOCM_DSR: u32 = 1 << 2;
/// Clear To Send.
pub const SERDEV_TIOCM_CTS: u32 = 1 << 3;
/// Carrier Detect.
pub const SERDEV_TIOCM_CD: u32 = 1 << 4;
/// Ring Indicator.
pub const SERDEV_TIOCM_RI: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Break control
// ---------------------------------------------------------------------------

/// Break off.
pub const SERDEV_BREAK_OFF: u8 = 0;
/// Break on (send continuous spacing).
pub const SERDEV_BREAK_ON: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parity_modes_distinct() {
        let modes = [SERDEV_PARITY_NONE, SERDEV_PARITY_ODD, SERDEV_PARITY_EVEN];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_flow_control_distinct() {
        let flows = [SERDEV_FLOW_NONE, SERDEV_FLOW_RTSCTS, SERDEV_FLOW_XONXOFF];
        for i in 0..flows.len() {
            for j in (i + 1)..flows.len() {
                assert_ne!(flows[i], flows[j]);
            }
        }
    }

    #[test]
    fn test_baud_rates_increasing() {
        let bauds = [
            SERDEV_BAUD_9600, SERDEV_BAUD_19200, SERDEV_BAUD_38400,
            SERDEV_BAUD_57600, SERDEV_BAUD_115200, SERDEV_BAUD_230400,
            SERDEV_BAUD_460800, SERDEV_BAUD_921600, SERDEV_BAUD_1000000,
            SERDEV_BAUD_1500000, SERDEV_BAUD_2000000, SERDEV_BAUD_3000000,
            SERDEV_BAUD_4000000,
        ];
        for i in 1..bauds.len() {
            assert!(bauds[i] > bauds[i - 1]);
        }
    }

    #[test]
    fn test_databits_distinct() {
        let bits = [
            SERDEV_DATABITS_5, SERDEV_DATABITS_6,
            SERDEV_DATABITS_7, SERDEV_DATABITS_8,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_stopbits_distinct() {
        assert_ne!(SERDEV_STOPBITS_1, SERDEV_STOPBITS_2);
    }

    #[test]
    fn test_modem_lines_no_overlap() {
        let lines = [
            SERDEV_TIOCM_DTR, SERDEV_TIOCM_RTS, SERDEV_TIOCM_DSR,
            SERDEV_TIOCM_CTS, SERDEV_TIOCM_CD, SERDEV_TIOCM_RI,
        ];
        for i in 0..lines.len() {
            for j in (i + 1)..lines.len() {
                assert_eq!(lines[i] & lines[j], 0);
            }
        }
    }

    #[test]
    fn test_modem_lines_power_of_two() {
        let lines = [
            SERDEV_TIOCM_DTR, SERDEV_TIOCM_RTS, SERDEV_TIOCM_DSR,
            SERDEV_TIOCM_CTS, SERDEV_TIOCM_CD, SERDEV_TIOCM_RI,
        ];
        for l in &lines {
            assert!(l.is_power_of_two());
        }
    }

    #[test]
    fn test_break_control_distinct() {
        assert_ne!(SERDEV_BREAK_OFF, SERDEV_BREAK_ON);
    }
}
