//! `<linux/net_tstamp.h>` — Additional network timestamping constants.
//!
//! Supplementary network timestamping constants covering
//! SOF_TIMESTAMPING flags, TX types, and hardware filter modes.

// ---------------------------------------------------------------------------
// SOF_TIMESTAMPING flags
// ---------------------------------------------------------------------------

/// Timestamp TX hardware.
pub const SOF_TIMESTAMPING_TX_HARDWARE: u32 = 1 << 0;
/// Timestamp TX software.
pub const SOF_TIMESTAMPING_TX_SOFTWARE: u32 = 1 << 1;
/// Timestamp RX hardware.
pub const SOF_TIMESTAMPING_RX_HARDWARE: u32 = 1 << 2;
/// Timestamp RX software.
pub const SOF_TIMESTAMPING_RX_SOFTWARE: u32 = 1 << 3;
/// Software system clock.
pub const SOF_TIMESTAMPING_SOFTWARE: u32 = 1 << 4;
/// System time with PHC.
pub const SOF_TIMESTAMPING_SYS_HARDWARE: u32 = 1 << 5;
/// Raw hardware.
pub const SOF_TIMESTAMPING_RAW_HARDWARE: u32 = 1 << 6;
/// Opt ID.
pub const SOF_TIMESTAMPING_OPT_ID: u32 = 1 << 7;
/// TX sched.
pub const SOF_TIMESTAMPING_TX_SCHED: u32 = 1 << 8;
/// TX ACK.
pub const SOF_TIMESTAMPING_TX_ACK: u32 = 1 << 9;
/// Opt CMSG.
pub const SOF_TIMESTAMPING_OPT_CMSG: u32 = 1 << 10;
/// Opt timestamp ns.
pub const SOF_TIMESTAMPING_OPT_TSONLY: u32 = 1 << 11;
/// Opt stats.
pub const SOF_TIMESTAMPING_OPT_STATS: u32 = 1 << 12;
/// Opt pktinfo.
pub const SOF_TIMESTAMPING_OPT_PKTINFO: u32 = 1 << 13;
/// Opt TX swhw.
pub const SOF_TIMESTAMPING_OPT_TX_SWHW: u32 = 1 << 14;
/// Bind PHC.
pub const SOF_TIMESTAMPING_BIND_PHC: u32 = 1 << 15;
/// Opt ID TCP.
pub const SOF_TIMESTAMPING_OPT_ID_TCP: u32 = 1 << 16;

// ---------------------------------------------------------------------------
// Hardware timestamp filter modes
// ---------------------------------------------------------------------------

/// No filter.
pub const HWTSTAMP_FILTER_NONE: u32 = 0;
/// All packets.
pub const HWTSTAMP_FILTER_ALL: u32 = 1;
/// Some packets.
pub const HWTSTAMP_FILTER_SOME: u32 = 2;
/// PTP v1 L4 event.
pub const HWTSTAMP_FILTER_PTP_V1_L4_EVENT: u32 = 3;
/// PTP v1 L4 sync.
pub const HWTSTAMP_FILTER_PTP_V1_L4_SYNC: u32 = 4;
/// PTP v1 L4 delay req.
pub const HWTSTAMP_FILTER_PTP_V1_L4_DELAY_REQ: u32 = 5;
/// PTP v2 L4 event.
pub const HWTSTAMP_FILTER_PTP_V2_L4_EVENT: u32 = 6;
/// PTP v2 L4 sync.
pub const HWTSTAMP_FILTER_PTP_V2_L4_SYNC: u32 = 7;
/// PTP v2 L4 delay req.
pub const HWTSTAMP_FILTER_PTP_V2_L4_DELAY_REQ: u32 = 8;
/// PTP v2 L2 event.
pub const HWTSTAMP_FILTER_PTP_V2_L2_EVENT: u32 = 9;
/// PTP v2 event.
pub const HWTSTAMP_FILTER_PTP_V2_EVENT: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sof_flags_no_overlap() {
        let flags = [
            SOF_TIMESTAMPING_TX_HARDWARE,
            SOF_TIMESTAMPING_TX_SOFTWARE,
            SOF_TIMESTAMPING_RX_HARDWARE,
            SOF_TIMESTAMPING_RX_SOFTWARE,
            SOF_TIMESTAMPING_SOFTWARE,
            SOF_TIMESTAMPING_SYS_HARDWARE,
            SOF_TIMESTAMPING_RAW_HARDWARE,
            SOF_TIMESTAMPING_OPT_ID,
            SOF_TIMESTAMPING_TX_SCHED,
            SOF_TIMESTAMPING_TX_ACK,
            SOF_TIMESTAMPING_OPT_CMSG,
            SOF_TIMESTAMPING_OPT_TSONLY,
            SOF_TIMESTAMPING_OPT_STATS,
            SOF_TIMESTAMPING_OPT_PKTINFO,
            SOF_TIMESTAMPING_OPT_TX_SWHW,
            SOF_TIMESTAMPING_BIND_PHC,
            SOF_TIMESTAMPING_OPT_ID_TCP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_filter_modes_distinct() {
        let modes = [
            HWTSTAMP_FILTER_NONE,
            HWTSTAMP_FILTER_ALL,
            HWTSTAMP_FILTER_SOME,
            HWTSTAMP_FILTER_PTP_V1_L4_EVENT,
            HWTSTAMP_FILTER_PTP_V1_L4_SYNC,
            HWTSTAMP_FILTER_PTP_V1_L4_DELAY_REQ,
            HWTSTAMP_FILTER_PTP_V2_L4_EVENT,
            HWTSTAMP_FILTER_PTP_V2_L4_SYNC,
            HWTSTAMP_FILTER_PTP_V2_L4_DELAY_REQ,
            HWTSTAMP_FILTER_PTP_V2_L2_EVENT,
            HWTSTAMP_FILTER_PTP_V2_EVENT,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
