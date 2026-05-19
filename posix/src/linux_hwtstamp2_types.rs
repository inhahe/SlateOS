//! `<linux/net_tstamp.h>` — Additional hardware timestamp constants.
//!
//! Supplementary hardware timestamping constants covering
//! TX types, RX filters, and timestamp source flags.

// ---------------------------------------------------------------------------
// Hardware timestamp TX types (HWTSTAMP_TX_*)
// ---------------------------------------------------------------------------

/// TX: off.
pub const HWTSTAMP_TX_OFF: u32 = 0;
/// TX: on.
pub const HWTSTAMP_TX_ON: u32 = 1;
/// TX: one-step sync (PTP).
pub const HWTSTAMP_TX_ONESTEP_SYNC: u32 = 2;
/// TX: one-step P2P.
pub const HWTSTAMP_TX_ONESTEP_P2P: u32 = 3;

// ---------------------------------------------------------------------------
// Hardware timestamp RX filters (HWTSTAMP_FILTER_*)
// ---------------------------------------------------------------------------

/// RX: no filter (disabled).
pub const HWTSTAMP_FILTER_NONE: u32 = 0;
/// RX: all packets.
pub const HWTSTAMP_FILTER_ALL: u32 = 1;
/// RX: some packets.
pub const HWTSTAMP_FILTER_SOME: u32 = 2;
/// RX: PTP v1 L4 event.
pub const HWTSTAMP_FILTER_PTP_V1_L4_EVENT: u32 = 3;
/// RX: PTP v1 L4 sync.
pub const HWTSTAMP_FILTER_PTP_V1_L4_SYNC: u32 = 4;
/// RX: PTP v1 L4 delay request.
pub const HWTSTAMP_FILTER_PTP_V1_L4_DELAY_REQ: u32 = 5;
/// RX: PTP v2 L4 event.
pub const HWTSTAMP_FILTER_PTP_V2_L4_EVENT: u32 = 6;
/// RX: PTP v2 L4 sync.
pub const HWTSTAMP_FILTER_PTP_V2_L4_SYNC: u32 = 7;
/// RX: PTP v2 L4 delay request.
pub const HWTSTAMP_FILTER_PTP_V2_L4_DELAY_REQ: u32 = 8;
/// RX: PTP v2 L2 event.
pub const HWTSTAMP_FILTER_PTP_V2_L2_EVENT: u32 = 9;
/// RX: PTP v2 L2 sync.
pub const HWTSTAMP_FILTER_PTP_V2_L2_SYNC: u32 = 10;
/// RX: PTP v2 L2 delay request.
pub const HWTSTAMP_FILTER_PTP_V2_L2_DELAY_REQ: u32 = 11;
/// RX: PTP v2 event.
pub const HWTSTAMP_FILTER_PTP_V2_EVENT: u32 = 12;
/// RX: PTP v2 sync.
pub const HWTSTAMP_FILTER_PTP_V2_SYNC: u32 = 13;
/// RX: PTP v2 delay request.
pub const HWTSTAMP_FILTER_PTP_V2_DELAY_REQ: u32 = 14;
/// RX: NTP all.
pub const HWTSTAMP_FILTER_NTP_ALL: u32 = 15;

// ---------------------------------------------------------------------------
// SO_TIMESTAMPING flags
// ---------------------------------------------------------------------------

/// Software TX timestamp.
pub const SOF_TIMESTAMPING_TX_SOFTWARE: u32 = 1 << 1;
/// Hardware TX timestamp.
pub const SOF_TIMESTAMPING_TX_HARDWARE: u32 = 1 << 0;
/// Software RX timestamp.
pub const SOF_TIMESTAMPING_RX_SOFTWARE: u32 = 1 << 3;
/// Hardware RX timestamp.
pub const SOF_TIMESTAMPING_RX_HARDWARE: u32 = 1 << 2;
/// Software system clock.
pub const SOF_TIMESTAMPING_SOFTWARE: u32 = 1 << 4;
/// Raw hardware timestamp.
pub const SOF_TIMESTAMPING_RAW_HARDWARE: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_types_distinct() {
        let types = [
            HWTSTAMP_TX_OFF, HWTSTAMP_TX_ON,
            HWTSTAMP_TX_ONESTEP_SYNC, HWTSTAMP_TX_ONESTEP_P2P,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_rx_filters_distinct() {
        let filters = [
            HWTSTAMP_FILTER_NONE, HWTSTAMP_FILTER_ALL,
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
    fn test_sof_flags_power_of_two() {
        let flags = [
            SOF_TIMESTAMPING_TX_HARDWARE,
            SOF_TIMESTAMPING_TX_SOFTWARE,
            SOF_TIMESTAMPING_RX_HARDWARE,
            SOF_TIMESTAMPING_RX_SOFTWARE,
            SOF_TIMESTAMPING_SOFTWARE,
            SOF_TIMESTAMPING_RAW_HARDWARE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_sof_flags_no_overlap() {
        let flags = [
            SOF_TIMESTAMPING_TX_HARDWARE,
            SOF_TIMESTAMPING_TX_SOFTWARE,
            SOF_TIMESTAMPING_RX_HARDWARE,
            SOF_TIMESTAMPING_RX_SOFTWARE,
            SOF_TIMESTAMPING_SOFTWARE,
            SOF_TIMESTAMPING_RAW_HARDWARE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
