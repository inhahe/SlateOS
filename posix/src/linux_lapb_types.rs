//! `<linux/lapb.h>` — LAPB (Link Access Procedure, Balanced) constants.
//!
//! LAPB is a data link layer protocol (ISO 7776) used by
//! X.25 networks.  These constants define LAPB modes,
//! IOCTL commands, and timer parameters.

// ---------------------------------------------------------------------------
// LAPB modes
// ---------------------------------------------------------------------------

/// Standard mode (modulo 8 — 3-bit sequence numbers).
pub const LAPB_STANDARD: u32 = 0x00;
/// Extended mode (modulo 128 — 7-bit sequence numbers).
pub const LAPB_EXTENDED: u32 = 0x01;

// ---------------------------------------------------------------------------
// LAPB DTE/DCE mode
// ---------------------------------------------------------------------------

/// DTE (Data Terminal Equipment) mode.
pub const LAPB_DTE: u32 = 0x00;
/// DCE (Data Circuit-terminating Equipment) mode.
pub const LAPB_DCE: u32 = 0x04;

// ---------------------------------------------------------------------------
// LAPB IOCTL commands
// ---------------------------------------------------------------------------

/// Get LAPB parameters.
pub const SIOCLAPBGETPARMS: u32 = 0x8980;
/// Set LAPB parameters.
pub const SIOCLAPBSETPARMS: u32 = 0x8981;

// ---------------------------------------------------------------------------
// LAPB timer defaults
// ---------------------------------------------------------------------------

/// Default T1 timer (3 seconds, in 100ms units).
pub const LAPB_DEFAULT_T1: u32 = 30;
/// Default T2 timer (1 second, in 100ms units).
pub const LAPB_DEFAULT_T2: u32 = 10;
/// Default N2 retry count.
pub const LAPB_DEFAULT_N2: u32 = 20;
/// Default window size (standard mode).
pub const LAPB_DEFAULT_WINDOW: u32 = 7;
/// Maximum window (standard, modulo 8).
pub const LAPB_MAX_WINDOW_STD: u32 = 7;
/// Maximum window (extended, modulo 128).
pub const LAPB_MAX_WINDOW_EXT: u32 = 127;

// ---------------------------------------------------------------------------
// LAPB state
// ---------------------------------------------------------------------------

/// Disconnected.
pub const LAPB_STATE_0: u32 = 0;
/// Awaiting connection.
pub const LAPB_STATE_1: u32 = 1;
/// Frame reject.
pub const LAPB_STATE_2: u32 = 2;
/// Data transfer.
pub const LAPB_STATE_3: u32 = 3;
/// Awaiting disconnect.
pub const LAPB_STATE_4: u32 = 4;

// ---------------------------------------------------------------------------
// LAPB reason codes
// ---------------------------------------------------------------------------

/// Normal disconnect.
pub const LAPB_OK: u32 = 0;
/// Bad token.
pub const LAPB_BADTOKEN: u32 = 1;
/// No connection.
pub const LAPB_NOTCONNECTED: u32 = 2;
/// Already connected.
pub const LAPB_CONNECTED: u32 = 3;
/// Out of memory.
pub const LAPB_NOMEM: u32 = 4;
/// Unknown error.
pub const LAPB_BADN: u32 = 5;
/// Timed out.
pub const LAPB_TIMEDOUT: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        assert_ne!(LAPB_STANDARD, LAPB_EXTENDED);
    }

    #[test]
    fn test_dte_dce_distinct() {
        assert_ne!(LAPB_DTE, LAPB_DCE);
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(SIOCLAPBGETPARMS, SIOCLAPBSETPARMS);
    }

    #[test]
    fn test_defaults() {
        assert!(LAPB_DEFAULT_WINDOW <= LAPB_MAX_WINDOW_STD);
        assert!(LAPB_MAX_WINDOW_STD < LAPB_MAX_WINDOW_EXT);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            LAPB_STATE_0,
            LAPB_STATE_1,
            LAPB_STATE_2,
            LAPB_STATE_3,
            LAPB_STATE_4,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_reasons_distinct() {
        let reasons = [
            LAPB_OK,
            LAPB_BADTOKEN,
            LAPB_NOTCONNECTED,
            LAPB_CONNECTED,
            LAPB_NOMEM,
            LAPB_BADN,
            LAPB_TIMEDOUT,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_ok_is_zero() {
        assert_eq!(LAPB_OK, 0);
    }

    #[test]
    fn test_standard_is_zero() {
        assert_eq!(LAPB_STANDARD, 0);
    }
}
