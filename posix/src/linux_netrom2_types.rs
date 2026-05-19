//! `<linux/netrom.h>` — Additional NET/ROM constants.
//!
//! Supplementary NET/ROM amateur radio networking constants covering
//! socket options, node types, and protocol parameters.

// ---------------------------------------------------------------------------
// NET/ROM socket options
// ---------------------------------------------------------------------------

/// Timer T1.
pub const NETROM_T1: u32 = 1;
/// Timer T2.
pub const NETROM_T2: u32 = 2;
/// Maximum retries N2.
pub const NETROM_N2: u32 = 3;
/// Timer T4.
pub const NETROM_T4: u32 = 6;
/// Idle timer.
pub const NETROM_IDLE: u32 = 7;

// ---------------------------------------------------------------------------
// NET/ROM protocol constants
// ---------------------------------------------------------------------------

/// Connection request.
pub const NR_CONNREQ: u32 = 0x01;
/// Connection acknowledge.
pub const NR_CONNACK: u32 = 0x02;
/// Disconnect request.
pub const NR_DISCREQ: u32 = 0x03;
/// Disconnect acknowledge.
pub const NR_DISCACK: u32 = 0x04;
/// Information.
pub const NR_INFO: u32 = 0x05;
/// Information acknowledge.
pub const NR_INFOACK: u32 = 0x06;
/// Reset.
pub const NR_RESET: u32 = 0x07;

// ---------------------------------------------------------------------------
// NET/ROM control flags
// ---------------------------------------------------------------------------

/// Choke flag.
pub const NR_CHOKE_FLAG: u32 = 0x80;
/// NAK flag.
pub const NR_NAK_FLAG: u32 = 0x40;
/// More flag.
pub const NR_MORE_FLAG: u32 = 0x20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sockopts_distinct() {
        let opts = [NETROM_T1, NETROM_T2, NETROM_N2, NETROM_T4, NETROM_IDLE];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_protocol_types_distinct() {
        let types = [
            NR_CONNREQ, NR_CONNACK, NR_DISCREQ,
            NR_DISCACK, NR_INFO, NR_INFOACK, NR_RESET,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_control_flags_no_overlap() {
        let flags = [NR_CHOKE_FLAG, NR_NAK_FLAG, NR_MORE_FLAG];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
