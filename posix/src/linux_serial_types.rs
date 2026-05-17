//! `<linux/serial.h>` — Serial port (UART) constants.
//!
//! Serial ports (UARTs) provide low-level byte-stream communication.
//! The Linux serial subsystem supports 8250/16550-compatible UARTs
//! and modern high-speed serial controllers. Configuration includes
//! baud rate, FIFO sizes, modem control lines, and RS-485 mode.

// ---------------------------------------------------------------------------
// Standard baud rates (Bxxx constants as speed values)
// ---------------------------------------------------------------------------

/// 0 baud (hang up).
pub const B0: u32 = 0;
/// 50 baud.
pub const B50: u32 = 1;
/// 110 baud.
pub const B110: u32 = 3;
/// 300 baud.
pub const B300: u32 = 7;
/// 1200 baud.
pub const B1200: u32 = 9;
/// 2400 baud.
pub const B2400: u32 = 11;
/// 4800 baud.
pub const B4800: u32 = 12;
/// 9600 baud.
pub const B9600: u32 = 13;
/// 19200 baud.
pub const B19200: u32 = 14;
/// 38400 baud.
pub const B38400: u32 = 15;
/// 57600 baud.
pub const B57600: u32 = 0o010001;
/// 115200 baud.
pub const B115200: u32 = 0o010002;
/// 230400 baud.
pub const B230400: u32 = 0o010003;
/// 460800 baud.
pub const B460800: u32 = 0o010004;
/// 921600 baud.
pub const B921600: u32 = 0o010007;

// ---------------------------------------------------------------------------
// Serial port type (serial_struct.type)
// ---------------------------------------------------------------------------

/// Unknown/unset port type.
pub const PORT_UNKNOWN: u32 = 0;
/// 8250 UART.
pub const PORT_8250: u32 = 1;
/// 16450 UART.
pub const PORT_16450: u32 = 2;
/// 16550 UART (with broken FIFO).
pub const PORT_16550: u32 = 3;
/// 16550A UART (working FIFO).
pub const PORT_16550A: u32 = 4;

// ---------------------------------------------------------------------------
// Modem control lines (TIOCM_* for ioctl)
// ---------------------------------------------------------------------------

/// Data Terminal Ready.
pub const TIOCM_DTR: u32 = 0x002;
/// Request To Send.
pub const TIOCM_RTS: u32 = 0x004;
/// Clear To Send.
pub const TIOCM_CTS: u32 = 0x020;
/// Carrier Detect.
pub const TIOCM_CAR: u32 = 0x040;
/// Ring Indicator.
pub const TIOCM_RNG: u32 = 0x080;
/// Data Set Ready.
pub const TIOCM_DSR: u32 = 0x100;

// ---------------------------------------------------------------------------
// RS-485 flags
// ---------------------------------------------------------------------------

/// Enable RS-485 mode.
pub const SER_RS485_ENABLED: u32 = 1 << 0;
/// Use RTS for direction control.
pub const SER_RS485_RTS_ON_SEND: u32 = 1 << 1;
/// RTS state after send.
pub const SER_RS485_RTS_AFTER_SEND: u32 = 1 << 2;
/// Enable receiving while sending.
pub const SER_RS485_RX_DURING_TX: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baud_rates_distinct() {
        let rates = [
            B0, B50, B110, B300, B1200, B2400, B4800,
            B9600, B19200, B38400, B57600, B115200,
            B230400, B460800, B921600,
        ];
        for i in 0..rates.len() {
            for j in (i + 1)..rates.len() {
                assert_ne!(rates[i], rates[j]);
            }
        }
    }

    #[test]
    fn test_port_types_distinct() {
        let types = [PORT_UNKNOWN, PORT_8250, PORT_16450, PORT_16550, PORT_16550A];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_modem_lines_distinct() {
        let lines = [TIOCM_DTR, TIOCM_RTS, TIOCM_CTS, TIOCM_CAR, TIOCM_RNG, TIOCM_DSR];
        for i in 0..lines.len() {
            for j in (i + 1)..lines.len() {
                assert_ne!(lines[i], lines[j]);
            }
        }
    }

    #[test]
    fn test_rs485_flags_no_overlap() {
        let flags = [
            SER_RS485_ENABLED, SER_RS485_RTS_ON_SEND,
            SER_RS485_RTS_AFTER_SEND, SER_RS485_RX_DURING_TX,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
