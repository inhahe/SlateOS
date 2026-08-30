//! `<linux/net_tstamp.h>` — Network timestamping constants.
//!
//! Hardware and software packet timestamping for PTP, latency
//! measurement, and network monitoring. Accessed via SO_TIMESTAMPING
//! socket option and cmsg ancillary data.

// ---------------------------------------------------------------------------
// SO_TIMESTAMPING flags (bit positions)
// ---------------------------------------------------------------------------

/// Software TX timestamp.
pub const SOF_TIMESTAMPING_TX_SOFTWARE: u32 = 1 << 1;
/// Software RX timestamp.
pub const SOF_TIMESTAMPING_RX_SOFTWARE: u32 = 1 << 3;
/// Software timestamp generation.
pub const SOF_TIMESTAMPING_SOFTWARE: u32 = 1 << 4;
/// Hardware TX timestamp (schedule).
pub const SOF_TIMESTAMPING_TX_HARDWARE: u32 = 1 << 0;
/// Hardware RX timestamp.
pub const SOF_TIMESTAMPING_RX_HARDWARE: u32 = 1 << 2;
/// Raw hardware timestamp.
pub const SOF_TIMESTAMPING_RAW_HARDWARE: u32 = 1 << 5;
/// Report optional stats (drops).
pub const SOF_TIMESTAMPING_OPT_STATS: u32 = 1 << 12;
/// Use ID for timestamp matching.
pub const SOF_TIMESTAMPING_OPT_ID: u32 = 1 << 7;
/// Timestamp reporting via cmsg.
pub const SOF_TIMESTAMPING_OPT_CMSG: u32 = 1 << 10;
/// TX SHed timestamp.
pub const SOF_TIMESTAMPING_TX_SCHED: u32 = 1 << 8;
/// TX ACK timestamp.
pub const SOF_TIMESTAMPING_TX_ACK: u32 = 1 << 9;
/// Timestamp pktinfo.
pub const SOF_TIMESTAMPING_OPT_PKTINFO: u32 = 1 << 11;
/// Timestamp TX SWR.
pub const SOF_TIMESTAMPING_OPT_TX_SWHW: u32 = 1 << 14;
/// Bind PHC (PTP Hardware Clock).
pub const SOF_TIMESTAMPING_BIND_PHC: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Hardware timestamp config (SIOCSHWTSTAMP / SIOCGHWTSTAMP)
// ---------------------------------------------------------------------------

/// No hardware TX timestamp.
pub const HWTSTAMP_TX_OFF: i32 = 0;
/// Hardware TX timestamp on.
pub const HWTSTAMP_TX_ON: i32 = 1;
/// One-step TX sync.
pub const HWTSTAMP_TX_ONESTEP_SYNC: i32 = 2;
/// One-step P2P sync.
pub const HWTSTAMP_TX_ONESTEP_P2P: i32 = 3;

/// No RX filter.
pub const HWTSTAMP_FILTER_NONE: i32 = 0;
/// Timestamp all RX packets.
pub const HWTSTAMP_FILTER_ALL: i32 = 1;
/// Some RX packets.
pub const HWTSTAMP_FILTER_SOME: i32 = 2;
/// PTP v1 L4 event packets.
pub const HWTSTAMP_FILTER_PTP_V1_L4_EVENT: i32 = 3;
/// PTP v1 L4 sync.
pub const HWTSTAMP_FILTER_PTP_V1_L4_SYNC: i32 = 4;
/// PTP v1 L4 delay request.
pub const HWTSTAMP_FILTER_PTP_V1_L4_DELAY_REQ: i32 = 5;
/// PTP v2 L4 event.
pub const HWTSTAMP_FILTER_PTP_V2_L4_EVENT: i32 = 6;
/// PTP v2 L4 sync.
pub const HWTSTAMP_FILTER_PTP_V2_L4_SYNC: i32 = 7;
/// PTP v2 L4 delay request.
pub const HWTSTAMP_FILTER_PTP_V2_L4_DELAY_REQ: i32 = 8;
/// PTP v2 L2 event.
pub const HWTSTAMP_FILTER_PTP_V2_L2_EVENT: i32 = 9;
/// PTP v2 L2 sync.
pub const HWTSTAMP_FILTER_PTP_V2_L2_SYNC: i32 = 10;
/// PTP v2 L2 delay request.
pub const HWTSTAMP_FILTER_PTP_V2_L2_DELAY_REQ: i32 = 11;
/// PTP v2 event.
pub const HWTSTAMP_FILTER_PTP_V2_EVENT: i32 = 12;
/// PTP v2 sync.
pub const HWTSTAMP_FILTER_PTP_V2_SYNC: i32 = 13;
/// PTP v2 delay request.
pub const HWTSTAMP_FILTER_PTP_V2_DELAY_REQ: i32 = 14;
/// NTP all.
pub const HWTSTAMP_FILTER_NTP_ALL: i32 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sof_flags_are_powers_of_two() {
        let flags = [
            SOF_TIMESTAMPING_TX_HARDWARE,
            SOF_TIMESTAMPING_TX_SOFTWARE,
            SOF_TIMESTAMPING_RX_HARDWARE,
            SOF_TIMESTAMPING_RX_SOFTWARE,
            SOF_TIMESTAMPING_SOFTWARE,
            SOF_TIMESTAMPING_RAW_HARDWARE,
            SOF_TIMESTAMPING_OPT_ID,
            SOF_TIMESTAMPING_TX_SCHED,
            SOF_TIMESTAMPING_TX_ACK,
            SOF_TIMESTAMPING_OPT_CMSG,
            SOF_TIMESTAMPING_OPT_PKTINFO,
            SOF_TIMESTAMPING_OPT_STATS,
            SOF_TIMESTAMPING_OPT_TX_SWHW,
            SOF_TIMESTAMPING_BIND_PHC,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not a power of two", flag);
        }
    }

    #[test]
    fn test_hwtstamp_tx_distinct() {
        let modes = [
            HWTSTAMP_TX_OFF,
            HWTSTAMP_TX_ON,
            HWTSTAMP_TX_ONESTEP_SYNC,
            HWTSTAMP_TX_ONESTEP_P2P,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_hwtstamp_filter_distinct() {
        let filters = [
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
            HWTSTAMP_FILTER_PTP_V2_L2_SYNC,
            HWTSTAMP_FILTER_PTP_V2_L2_DELAY_REQ,
            HWTSTAMP_FILTER_PTP_V2_EVENT,
            HWTSTAMP_FILTER_PTP_V2_SYNC,
            HWTSTAMP_FILTER_PTP_V2_DELAY_REQ,
            HWTSTAMP_FILTER_NTP_ALL,
        ];
        for i in 0..filters.len() {
            for j in (i + 1)..filters.len() {
                assert_ne!(filters[i], filters[j]);
            }
        }
    }

    #[test]
    fn test_tx_values() {
        assert_eq!(HWTSTAMP_TX_OFF, 0);
        assert_eq!(HWTSTAMP_TX_ON, 1);
    }
}
