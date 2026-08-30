//! `<linux/rose.h>` — ROSE (X.25 over AX.25) constants.
//!
//! ROSE is a packet-switched network layer used over AX.25
//! amateur radio links.  These constants define ROSE socket
//! options, address parameters, IOCTL commands, and cause codes.

// ---------------------------------------------------------------------------
// ROSE address family
// ---------------------------------------------------------------------------

/// ROSE address family.
pub const AF_ROSE: u32 = 11;
/// ROSE protocol family.
pub const PF_ROSE: u32 = AF_ROSE;

// ---------------------------------------------------------------------------
// ROSE socket options
// ---------------------------------------------------------------------------

/// Defer accept (wait for data before accepting).
pub const ROSE_DEFER: u32 = 1;
/// T1 timer (call request timeout).
pub const ROSE_T1: u32 = 2;
/// T2 timer (reset timeout).
pub const ROSE_T2: u32 = 3;
/// T3 timer (clear timeout).
pub const ROSE_T3: u32 = 4;
/// Idle timer.
pub const ROSE_IDLE: u32 = 5;
/// Q-bit include.
pub const ROSE_QBITINCL: u32 = 6;
/// Holdback timer.
pub const ROSE_HOLDBACK: u32 = 7;

// ---------------------------------------------------------------------------
// ROSE IOCTL commands
// ---------------------------------------------------------------------------

/// Get ROSE nodes.
pub const SIOCRSGL2CALL: u32 = 0x8920;
/// Set ROSE nodes.
pub const SIOCRSACCEPT: u32 = 0x8921;
/// Clear ROSE.
pub const SIOCRSSL2CALL: u32 = 0x8922;
/// Get ROSE call.
pub const SIOCRSCLRRT: u32 = 0x8923;

// ---------------------------------------------------------------------------
// ROSE address length
// ---------------------------------------------------------------------------

/// ROSE address length (5 BCD digits = 10 nibbles).
pub const ROSE_ADDR_LEN: u32 = 5;
/// ROSE maximum digipeaters.
pub const ROSE_MAX_DIGIS: u32 = 5;

// ---------------------------------------------------------------------------
// ROSE cause codes
// ---------------------------------------------------------------------------

/// DTE originated.
pub const ROSE_DTE_ORIGINATED: u32 = 0x00;
/// Number busy.
pub const ROSE_NUMBER_BUSY: u32 = 0x01;
/// Invalid facility request.
pub const ROSE_INVALID_FACILITY: u32 = 0x03;
/// Network congestion.
pub const ROSE_NETWORK_CONGESTION: u32 = 0x05;
/// Out of order.
pub const ROSE_OUT_OF_ORDER: u32 = 0x09;
/// Access barred.
pub const ROSE_ACCESS_BARRED: u32 = 0x0B;
/// Not obtainable.
pub const ROSE_NOT_OBTAINABLE: u32 = 0x0D;
/// Remote procedure error.
pub const ROSE_REMOTE_PROCEDURE: u32 = 0x11;
/// Local procedure error.
pub const ROSE_LOCAL_PROCEDURE: u32 = 0x13;
/// Ship absent.
pub const ROSE_SHIP_ABSENT: u32 = 0x39;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_rose() {
        assert_eq!(AF_ROSE, 11);
        assert_eq!(PF_ROSE, AF_ROSE);
    }

    #[test]
    fn test_sockopts_distinct() {
        let opts = [
            ROSE_DEFER,
            ROSE_T1,
            ROSE_T2,
            ROSE_T3,
            ROSE_IDLE,
            ROSE_QBITINCL,
            ROSE_HOLDBACK,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [SIOCRSGL2CALL, SIOCRSACCEPT, SIOCRSSL2CALL, SIOCRSCLRRT];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_causes_distinct() {
        let causes = [
            ROSE_DTE_ORIGINATED,
            ROSE_NUMBER_BUSY,
            ROSE_INVALID_FACILITY,
            ROSE_NETWORK_CONGESTION,
            ROSE_OUT_OF_ORDER,
            ROSE_ACCESS_BARRED,
            ROSE_NOT_OBTAINABLE,
            ROSE_REMOTE_PROCEDURE,
            ROSE_LOCAL_PROCEDURE,
            ROSE_SHIP_ABSENT,
        ];
        for i in 0..causes.len() {
            for j in (i + 1)..causes.len() {
                assert_ne!(causes[i], causes[j]);
            }
        }
    }

    #[test]
    fn test_addr_len() {
        assert_eq!(ROSE_ADDR_LEN, 5);
    }

    #[test]
    fn test_max_digis() {
        assert_eq!(ROSE_MAX_DIGIS, 5);
    }

    #[test]
    fn test_dte_originated_is_zero() {
        assert_eq!(ROSE_DTE_ORIGINATED, 0);
    }
}
