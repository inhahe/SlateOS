//! `<linux/serial.h>` — Additional serial port constants.
//!
//! Supplementary serial constants covering port types,
//! UART types, serial flags, and modem control lines.

// ---------------------------------------------------------------------------
// UART types (PORT_*)
// ---------------------------------------------------------------------------

/// Unknown UART.
pub const PORT_UNKNOWN: u32 = 0;
/// 8250 UART.
pub const PORT_8250: u32 = 1;
/// 16450 UART.
pub const PORT_16450: u32 = 2;
/// 16550 UART.
pub const PORT_16550: u32 = 3;
/// 16550A UART.
pub const PORT_16550A: u32 = 4;
/// Cirrus Logic.
pub const PORT_CIRRUS: u32 = 5;
/// 16650 UART.
pub const PORT_16650: u32 = 6;
/// 16650V2 UART.
pub const PORT_16650V2: u32 = 7;
/// 16750 UART.
pub const PORT_16750: u32 = 8;
/// Startech UART.
pub const PORT_STARTECH: u32 = 9;
/// 16C950 UART.
pub const PORT_16C950: u32 = 10;
/// 16654 UART.
pub const PORT_16654: u32 = 11;
/// 16850 UART.
pub const PORT_16850: u32 = 12;

// ---------------------------------------------------------------------------
// Serial flags (ASYNC_*)
// ---------------------------------------------------------------------------

/// Hardware flow control (CTS).
pub const ASYNC_HUP_NOTIFY: u32 = 0x0001;
/// Four-port card.
pub const ASYNC_FOURPORT: u32 = 0x0002;
/// SAK (Secure Attention Key).
pub const ASYNC_SAK: u32 = 0x0004;
/// Split termios.
pub const ASYNC_SPLIT_TERMIOS: u32 = 0x0008;
/// SPD mask.
pub const ASYNC_SPD_MASK: u32 = 0x1030;
/// SPD HI.
pub const ASYNC_SPD_HI: u32 = 0x0010;
/// SPD VHI.
pub const ASYNC_SPD_VHI: u32 = 0x0020;
/// SPD custom.
pub const ASYNC_SPD_CUST: u32 = 0x0030;
/// Skip test.
pub const ASYNC_SKIP_TEST: u32 = 0x0040;
/// Auto IRQ.
pub const ASYNC_AUTO_IRQ: u32 = 0x0080;
/// Session lockout.
pub const ASYNC_SESSION_LOCKOUT: u32 = 0x0100;
/// Pgrp lockout.
pub const ASYNC_PGRP_LOCKOUT: u32 = 0x0200;
/// Callout nohup.
pub const ASYNC_CALLOUT_NOHUP: u32 = 0x0400;
/// Low latency.
pub const ASYNC_LOW_LATENCY: u32 = 0x2000;
/// Buggy UART.
pub const ASYNC_BUGGY_UART: u32 = 0x4000;

// ---------------------------------------------------------------------------
// Modem control lines (TIOCM_*)
// ---------------------------------------------------------------------------

/// Line Enable.
pub const TIOCM_LE: u32 = 0x001;
/// Data Terminal Ready.
pub const TIOCM_DTR: u32 = 0x002;
/// Request To Send.
pub const TIOCM_RTS: u32 = 0x004;
/// Secondary Transmit.
pub const TIOCM_ST: u32 = 0x008;
/// Secondary Receive.
pub const TIOCM_SR: u32 = 0x010;
/// Clear To Send.
pub const TIOCM_CTS: u32 = 0x020;
/// Carrier Detect.
pub const TIOCM_CAR: u32 = 0x040;
/// Ring Indicator.
pub const TIOCM_RNG: u32 = 0x080;
/// Data Set Ready.
pub const TIOCM_DSR: u32 = 0x100;
/// Alias for Carrier Detect.
pub const TIOCM_CD: u32 = TIOCM_CAR;
/// Alias for Ring Indicator.
pub const TIOCM_RI: u32 = TIOCM_RNG;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_types_distinct() {
        let ports = [
            PORT_UNKNOWN,
            PORT_8250,
            PORT_16450,
            PORT_16550,
            PORT_16550A,
            PORT_CIRRUS,
            PORT_16650,
            PORT_16650V2,
            PORT_16750,
            PORT_STARTECH,
            PORT_16C950,
            PORT_16654,
            PORT_16850,
        ];
        for i in 0..ports.len() {
            for j in (i + 1)..ports.len() {
                assert_ne!(ports[i], ports[j]);
            }
        }
    }

    #[test]
    fn test_modem_lines_power_of_two() {
        let lines = [
            TIOCM_LE, TIOCM_DTR, TIOCM_RTS, TIOCM_ST, TIOCM_SR, TIOCM_CTS, TIOCM_CAR, TIOCM_RNG,
            TIOCM_DSR,
        ];
        for l in &lines {
            assert!(l.is_power_of_two(), "0x{:03x} not power of two", l);
        }
    }

    #[test]
    fn test_modem_aliases() {
        assert_eq!(TIOCM_CD, TIOCM_CAR);
        assert_eq!(TIOCM_RI, TIOCM_RNG);
    }

    #[test]
    fn test_modem_lines_no_overlap() {
        let lines = [
            TIOCM_LE, TIOCM_DTR, TIOCM_RTS, TIOCM_ST, TIOCM_SR, TIOCM_CTS, TIOCM_CAR, TIOCM_RNG,
            TIOCM_DSR,
        ];
        for i in 0..lines.len() {
            for j in (i + 1)..lines.len() {
                assert_eq!(lines[i] & lines[j], 0);
            }
        }
    }

    #[test]
    fn test_spd_values() {
        assert_eq!(ASYNC_SPD_HI, 0x0010);
        assert_eq!(ASYNC_SPD_VHI, 0x0020);
        assert_eq!(ASYNC_SPD_CUST, 0x0030);
    }
}
