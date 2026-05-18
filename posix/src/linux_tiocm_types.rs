//! `<asm-generic/ioctls.h>` — Serial/TTY modem control line constants.
//!
//! These constants represent modem control signals (DTR, RTS, CTS,
//! etc.) used with the TIOCMGET/TIOCMSET/TIOCMBIS/TIOCMBIC ioctls
//! for serial port control.

// ---------------------------------------------------------------------------
// Modem control line bits
// ---------------------------------------------------------------------------

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
/// Data Carrier Detect.
pub const TIOCM_CAR: u32 = 0x040;
/// Ring Indicator.
pub const TIOCM_RNG: u32 = 0x080;
/// Data Set Ready.
pub const TIOCM_DSR: u32 = 0x100;

/// Alias for TIOCM_CAR (Carrier Detect).
pub const TIOCM_CD: u32 = TIOCM_CAR;
/// Alias for TIOCM_RNG (Ring).
pub const TIOCM_RI: u32 = TIOCM_RNG;

// ---------------------------------------------------------------------------
// Modem control ioctls
// ---------------------------------------------------------------------------

/// Get modem control lines.
pub const TIOCMGET: u32 = 0x5415;
/// Set modem control lines.
pub const TIOCMSET: u32 = 0x5418;
/// Set bits in modem control lines (OR).
pub const TIOCMBIS: u32 = 0x5416;
/// Clear bits in modem control lines (AND NOT).
pub const TIOCMBIC: u32 = 0x5417;

// ---------------------------------------------------------------------------
// Break control ioctls
// ---------------------------------------------------------------------------

/// Send break.
pub const TCSBRK: u32 = 0x5409;
/// Send break (POSIX duration).
pub const TCSBRKP: u32 = 0x5425;
/// Turn break on.
pub const TIOCSBRK: u32 = 0x5427;
/// Turn break off.
pub const TIOCCBRK: u32 = 0x5428;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modem_lines_no_overlap() {
        let lines = [
            TIOCM_DTR, TIOCM_RTS, TIOCM_ST, TIOCM_SR,
            TIOCM_CTS, TIOCM_CAR, TIOCM_RNG, TIOCM_DSR,
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
            TIOCM_DTR, TIOCM_RTS, TIOCM_ST, TIOCM_SR,
            TIOCM_CTS, TIOCM_CAR, TIOCM_RNG, TIOCM_DSR,
        ];
        for l in &lines {
            assert!(l.is_power_of_two());
        }
    }

    #[test]
    fn test_aliases() {
        assert_eq!(TIOCM_CD, TIOCM_CAR);
        assert_eq!(TIOCM_RI, TIOCM_RNG);
    }

    #[test]
    fn test_control_ioctls_distinct() {
        let ioctls = [TIOCMGET, TIOCMSET, TIOCMBIS, TIOCMBIC];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_break_ioctls_distinct() {
        let ioctls = [TCSBRK, TCSBRKP, TIOCSBRK, TIOCCBRK];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_tiocmget() {
        assert_eq!(TIOCMGET, 0x5415);
    }
}
