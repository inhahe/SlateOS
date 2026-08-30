//! `<sys/socket.h>` — Socket control message (SCM) protocol constants.
//!
//! SCM messages carry out-of-band metadata alongside socket data.
//! This module covers the pidfd-based credential passing added in
//! Linux 5.9+ and the extended timestamping flags used for precise
//! network timing measurements.

// ---------------------------------------------------------------------------
// SCM_PIDFD (Linux 6.2+)
// ---------------------------------------------------------------------------

/// Pass pidfd via ancillary data.
pub const SCM_PIDFD: u32 = 0x04;

// ---------------------------------------------------------------------------
// SO_TIMESTAMPING flags
// ---------------------------------------------------------------------------

/// Request software transmit timestamp.
pub const SOF_TIMESTAMPING_TX_SOFTWARE: u32 = 1 << 1;
/// Request software receive timestamp.
pub const SOF_TIMESTAMPING_RX_SOFTWARE: u32 = 1 << 3;
/// Request hardware transmit timestamp.
pub const SOF_TIMESTAMPING_TX_HARDWARE: u32 = 1 << 0;
/// Request hardware receive timestamp.
pub const SOF_TIMESTAMPING_RX_HARDWARE: u32 = 1 << 2;
/// Report raw hardware timestamp.
pub const SOF_TIMESTAMPING_RAW_HARDWARE: u32 = 1 << 4;
/// Use software clock for timestamps.
pub const SOF_TIMESTAMPING_SOFTWARE: u32 = 1 << 5;
/// Report schedule timestamp.
pub const SOF_TIMESTAMPING_TX_SCHED: u32 = 1 << 8;
/// Report ACK timestamp.
pub const SOF_TIMESTAMPING_TX_ACK: u32 = 1 << 9;
/// Request OPT_ID semantics.
pub const SOF_TIMESTAMPING_OPT_ID: u32 = 1 << 10;
/// Timestamp only on errors.
pub const SOF_TIMESTAMPING_OPT_TSONLY: u32 = 1 << 11;
/// Include original packet stats.
pub const SOF_TIMESTAMPING_OPT_STATS: u32 = 1 << 12;
/// Receive timestamps on error queue.
pub const SOF_TIMESTAMPING_OPT_PKTINFO: u32 = 1 << 13;
/// Include tx flags in cmsg.
pub const SOF_TIMESTAMPING_OPT_TX_SWHW: u32 = 1 << 14;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scm_pidfd() {
        assert_eq!(SCM_PIDFD, 0x04);
    }

    #[test]
    fn test_timestamping_flags_power_of_two() {
        let flags = [
            SOF_TIMESTAMPING_TX_HARDWARE,
            SOF_TIMESTAMPING_TX_SOFTWARE,
            SOF_TIMESTAMPING_RX_HARDWARE,
            SOF_TIMESTAMPING_RX_SOFTWARE,
            SOF_TIMESTAMPING_RAW_HARDWARE,
            SOF_TIMESTAMPING_SOFTWARE,
            SOF_TIMESTAMPING_TX_SCHED,
            SOF_TIMESTAMPING_TX_ACK,
            SOF_TIMESTAMPING_OPT_ID,
            SOF_TIMESTAMPING_OPT_TSONLY,
            SOF_TIMESTAMPING_OPT_STATS,
            SOF_TIMESTAMPING_OPT_PKTINFO,
            SOF_TIMESTAMPING_OPT_TX_SWHW,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f} is not power of two");
        }
    }

    #[test]
    fn test_timestamping_flags_no_overlap() {
        let flags = [
            SOF_TIMESTAMPING_TX_HARDWARE,
            SOF_TIMESTAMPING_TX_SOFTWARE,
            SOF_TIMESTAMPING_RX_HARDWARE,
            SOF_TIMESTAMPING_RX_SOFTWARE,
            SOF_TIMESTAMPING_RAW_HARDWARE,
            SOF_TIMESTAMPING_SOFTWARE,
            SOF_TIMESTAMPING_TX_SCHED,
            SOF_TIMESTAMPING_TX_ACK,
            SOF_TIMESTAMPING_OPT_ID,
            SOF_TIMESTAMPING_OPT_TSONLY,
            SOF_TIMESTAMPING_OPT_STATS,
            SOF_TIMESTAMPING_OPT_PKTINFO,
            SOF_TIMESTAMPING_OPT_TX_SWHW,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tx_hardware_is_bit0() {
        assert_eq!(SOF_TIMESTAMPING_TX_HARDWARE, 1);
    }
}
