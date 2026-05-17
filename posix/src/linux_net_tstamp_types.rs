//! `<linux/net_tstamp.h>` — Network packet timestamping constants.
//!
//! Linux supports hardware and software packet timestamps for precise
//! timing of network events (PTP/IEEE 1588, NTP, latency measurement).
//! Timestamps can be recorded at various points in the TX/RX path.
//! Configured via socket options (SO_TIMESTAMPING) and queried from
//! control messages (SCM_TIMESTAMPING).

// ---------------------------------------------------------------------------
// SO_TIMESTAMPING flags
// ---------------------------------------------------------------------------

/// Generate software TX timestamp.
pub const SOF_TIMESTAMPING_TX_SOFTWARE: u32 = 1 << 1;
/// Generate hardware TX timestamp (NIC).
pub const SOF_TIMESTAMPING_TX_HARDWARE: u32 = 1 << 0;
/// Generate software RX timestamp.
pub const SOF_TIMESTAMPING_RX_SOFTWARE: u32 = 1 << 3;
/// Generate hardware RX timestamp.
pub const SOF_TIMESTAMPING_RX_HARDWARE: u32 = 1 << 2;
/// Report software timestamps.
pub const SOF_TIMESTAMPING_SOFTWARE: u32 = 1 << 4;
/// Report raw hardware timestamps.
pub const SOF_TIMESTAMPING_RAW_HARDWARE: u32 = 1 << 6;
/// Request OPT_ID on error queue.
pub const SOF_TIMESTAMPING_OPT_ID: u32 = 1 << 7;
/// TX timestamp on SYN/ACK.
pub const SOF_TIMESTAMPING_OPT_TSONLY: u32 = 1 << 11;
/// Stats in error queue.
pub const SOF_TIMESTAMPING_OPT_STATS: u32 = 1 << 12;
/// Enable per-packet timestamp type.
pub const SOF_TIMESTAMPING_OPT_PKTINFO: u32 = 1 << 13;
/// TX SCM_TXTIME-based scheduling.
pub const SOF_TIMESTAMPING_TX_SCHED: u32 = 1 << 8;
/// TX ACK timestamp.
pub const SOF_TIMESTAMPING_TX_ACK: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Hardware timestamp config (SIOCGHWTSTAMP/SIOCSHWTSTAMP)
// ---------------------------------------------------------------------------

/// No hardware timestamping.
pub const HWTSTAMP_TX_OFF: u32 = 0;
/// Enable HW TX timestamps.
pub const HWTSTAMP_TX_ON: u32 = 1;
/// Onestep (PTP: timestamp inserted in packet by HW).
pub const HWTSTAMP_TX_ONESTEP_SYNC: u32 = 2;
/// Onestep P2P.
pub const HWTSTAMP_TX_ONESTEP_P2P: u32 = 3;

/// No RX hardware filter.
pub const HWTSTAMP_FILTER_NONE: u32 = 0;
/// Timestamp all RX packets.
pub const HWTSTAMP_FILTER_ALL: u32 = 1;
/// Some RX packets (best effort).
pub const HWTSTAMP_FILTER_SOME: u32 = 2;
/// PTP v1 L4 event messages only.
pub const HWTSTAMP_FILTER_PTP_V1_L4_EVENT: u32 = 3;
/// PTP v2 L4 event messages.
pub const HWTSTAMP_FILTER_PTP_V2_L4_EVENT: u32 = 6;
/// PTP v2 L2 event messages.
pub const HWTSTAMP_FILTER_PTP_V2_L2_EVENT: u32 = 9;
/// PTP v2 event messages (any layer).
pub const HWTSTAMP_FILTER_PTP_V2_EVENT: u32 = 12;
/// NTP all packets.
pub const HWTSTAMP_FILTER_NTP_ALL: u32 = 15;

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
            SOF_TIMESTAMPING_RAW_HARDWARE,
            SOF_TIMESTAMPING_OPT_ID,
            SOF_TIMESTAMPING_TX_SCHED,
            SOF_TIMESTAMPING_TX_ACK,
            SOF_TIMESTAMPING_OPT_TSONLY,
            SOF_TIMESTAMPING_OPT_STATS,
            SOF_TIMESTAMPING_OPT_PKTINFO,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_hwtstamp_tx_distinct() {
        let txmodes = [
            HWTSTAMP_TX_OFF, HWTSTAMP_TX_ON,
            HWTSTAMP_TX_ONESTEP_SYNC, HWTSTAMP_TX_ONESTEP_P2P,
        ];
        for i in 0..txmodes.len() {
            for j in (i + 1)..txmodes.len() {
                assert_ne!(txmodes[i], txmodes[j]);
            }
        }
    }

    #[test]
    fn test_hwtstamp_filter_distinct() {
        let filters = [
            HWTSTAMP_FILTER_NONE, HWTSTAMP_FILTER_ALL,
            HWTSTAMP_FILTER_SOME,
            HWTSTAMP_FILTER_PTP_V1_L4_EVENT,
            HWTSTAMP_FILTER_PTP_V2_L4_EVENT,
            HWTSTAMP_FILTER_PTP_V2_L2_EVENT,
            HWTSTAMP_FILTER_PTP_V2_EVENT,
            HWTSTAMP_FILTER_NTP_ALL,
        ];
        for i in 0..filters.len() {
            for j in (i + 1)..filters.len() {
                assert_ne!(filters[i], filters[j]);
            }
        }
    }
}
