//! `<uapi/linux/dcbnl.h>` — IEEE 802.1Qaz attribute layout.
//!
//! The IEEE branch of DCB exposes ETS, PFC, and APP table attributes
//! under DCB_ATTR_IEEE. This file mirrors the userspace ABI for
//! `struct ieee_ets`, `struct ieee_pfc`, and APP-table entries.

// ---------------------------------------------------------------------------
// DCB_ATTR_IEEE nested attribute set
// ---------------------------------------------------------------------------

pub const DCB_ATTR_IEEE_UNSPEC: u16 = 0;
pub const DCB_ATTR_IEEE_ETS: u16 = 1;
pub const DCB_ATTR_IEEE_PFC: u16 = 2;
pub const DCB_ATTR_IEEE_APP_TABLE: u16 = 3;
pub const DCB_ATTR_IEEE_PEER_ETS: u16 = 4;
pub const DCB_ATTR_IEEE_PEER_PFC: u16 = 5;
pub const DCB_ATTR_IEEE_PEER_APP: u16 = 6;
pub const DCB_ATTR_IEEE_MAXRATE: u16 = 7;
pub const DCB_ATTR_IEEE_QCN: u16 = 8;
pub const DCB_ATTR_IEEE_QCN_STATS: u16 = 9;
pub const DCB_ATTR_DCB_BUFFER: u16 = 10;
pub const DCB_ATTR_DCB_APP_TRUST_TABLE: u16 = 11;
pub const DCB_ATTR_DCB_REWR_TABLE: u16 = 12;
pub const __DCB_ATTR_IEEE_MAX: u16 = 13;

// ---------------------------------------------------------------------------
// struct ieee_ets field layout (subset)
// ---------------------------------------------------------------------------

/// Number of traffic classes covered by ETS arrays.
pub const IEEE_ETS_NUM_TCS: usize = 8;
/// Number of prio_tc entries (one per user priority 0..7).
pub const IEEE_ETS_PRIO_TC_LEN: usize = 8;
/// Number of tc_tx_bw / tc_rx_bw entries (one per TC).
pub const IEEE_ETS_BW_LEN: usize = 8;
/// Number of tc_tsa entries (transmission selection algorithm per TC).
pub const IEEE_ETS_TSA_LEN: usize = 8;

// Offsets within struct ieee_ets (16-byte struct as laid out by kernel).
pub const IEEE_ETS_OFF_WILLING: usize = 0;
pub const IEEE_ETS_OFF_ETS_CAP: usize = 1;
pub const IEEE_ETS_OFF_CBS: usize = 2;
pub const IEEE_ETS_OFF_TC_TX_BW: usize = 3;
pub const IEEE_ETS_OFF_TC_RX_BW: usize = 11;
pub const IEEE_ETS_OFF_TC_TSA: usize = 19;
pub const IEEE_ETS_OFF_PRIO_TC: usize = 27;
pub const IEEE_ETS_OFF_TC_RECO_BW: usize = 35;
pub const IEEE_ETS_OFF_TC_RECO_TSA: usize = 43;
pub const IEEE_ETS_OFF_RECO_PRIO_TC: usize = 51;
pub const IEEE_ETS_TOTAL_SIZE: usize = 59;

// ---------------------------------------------------------------------------
// struct ieee_pfc field layout
// ---------------------------------------------------------------------------

pub const IEEE_PFC_OFF_PFC_CAP: usize = 0;
pub const IEEE_PFC_OFF_PFC_EN: usize = 1;
pub const IEEE_PFC_OFF_MBC: usize = 2;
pub const IEEE_PFC_OFF_DELAY: usize = 4;
pub const IEEE_PFC_OFF_REQUESTS: usize = 8;
pub const IEEE_PFC_OFF_INDICATIONS: usize = 72;

/// Number of u64 per requests/indications array (one per priority).
pub const IEEE_PFC_PRIO_COUNTERS: usize = 8;

// ---------------------------------------------------------------------------
// APP table entry priority field
// ---------------------------------------------------------------------------

pub const IEEE_DCB_APP_DEFAULT_PRIO: u8 = 0;
pub const IEEE_DCB_APP_MAX_PRIO: u8 = 7;
/// IEEE_8021QAZ_APP_SEL_* selectors (subset).
pub const IEEE_8021QAZ_APP_SEL_ETHERTYPE: u8 = 1;
pub const IEEE_8021QAZ_APP_SEL_STREAM: u8 = 2;
pub const IEEE_8021QAZ_APP_SEL_DGRAM: u8 = 3;
pub const IEEE_8021QAZ_APP_SEL_ANY: u8 = 4;
pub const IEEE_8021QAZ_APP_SEL_DSCP: u8 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ieee_attrs_dense_0_to_12() {
        let a = [
            DCB_ATTR_IEEE_UNSPEC,
            DCB_ATTR_IEEE_ETS,
            DCB_ATTR_IEEE_PFC,
            DCB_ATTR_IEEE_APP_TABLE,
            DCB_ATTR_IEEE_PEER_ETS,
            DCB_ATTR_IEEE_PEER_PFC,
            DCB_ATTR_IEEE_PEER_APP,
            DCB_ATTR_IEEE_MAXRATE,
            DCB_ATTR_IEEE_QCN,
            DCB_ATTR_IEEE_QCN_STATS,
            DCB_ATTR_DCB_BUFFER,
            DCB_ATTR_DCB_APP_TRUST_TABLE,
            DCB_ATTR_DCB_REWR_TABLE,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(__DCB_ATTR_IEEE_MAX as usize, a.len());
    }

    #[test]
    fn test_local_peer_pairs_distinct() {
        // PEER_ETS != local ETS, etc.
        assert_ne!(DCB_ATTR_IEEE_ETS, DCB_ATTR_IEEE_PEER_ETS);
        assert_ne!(DCB_ATTR_IEEE_PFC, DCB_ATTR_IEEE_PEER_PFC);
        assert_ne!(DCB_ATTR_IEEE_APP_TABLE, DCB_ATTR_IEEE_PEER_APP);
    }

    #[test]
    fn test_ets_array_lengths_all_8() {
        for n in [
            IEEE_ETS_NUM_TCS,
            IEEE_ETS_PRIO_TC_LEN,
            IEEE_ETS_BW_LEN,
            IEEE_ETS_TSA_LEN,
        ] {
            assert_eq!(n, 8);
        }
    }

    #[test]
    fn test_ets_offsets_strictly_increasing() {
        let off = [
            IEEE_ETS_OFF_WILLING,
            IEEE_ETS_OFF_ETS_CAP,
            IEEE_ETS_OFF_CBS,
            IEEE_ETS_OFF_TC_TX_BW,
            IEEE_ETS_OFF_TC_RX_BW,
            IEEE_ETS_OFF_TC_TSA,
            IEEE_ETS_OFF_PRIO_TC,
            IEEE_ETS_OFF_TC_RECO_BW,
            IEEE_ETS_OFF_TC_RECO_TSA,
            IEEE_ETS_OFF_RECO_PRIO_TC,
        ];
        for w in off.windows(2) {
            assert!(w[1] > w[0]);
        }
        // Each array of 8 bytes follows the previous.
        assert_eq!(IEEE_ETS_OFF_TC_RX_BW - IEEE_ETS_OFF_TC_TX_BW, 8);
        assert_eq!(IEEE_ETS_OFF_TC_TSA - IEEE_ETS_OFF_TC_RX_BW, 8);
        assert_eq!(IEEE_ETS_OFF_PRIO_TC - IEEE_ETS_OFF_TC_TSA, 8);
        // Total = 51 + 8 = 59.
        assert_eq!(IEEE_ETS_TOTAL_SIZE, IEEE_ETS_OFF_RECO_PRIO_TC + 8);
    }

    #[test]
    fn test_pfc_indications_follows_requests() {
        // Both arrays are 8 u64 = 64 bytes.
        assert_eq!(
            IEEE_PFC_OFF_INDICATIONS - IEEE_PFC_OFF_REQUESTS,
            IEEE_PFC_PRIO_COUNTERS * 8
        );
    }

    #[test]
    fn test_app_priority_field_3_bits() {
        // Priority field is 3 bits → 0..7.
        assert_eq!(IEEE_DCB_APP_MAX_PRIO, 7);
        assert!((IEEE_DCB_APP_MAX_PRIO as u32) < (1u32 << 3));
        assert_eq!(IEEE_DCB_APP_DEFAULT_PRIO, 0);
    }

    #[test]
    fn test_app_selectors_distinct_dense_1_to_5() {
        let s = [
            IEEE_8021QAZ_APP_SEL_ETHERTYPE,
            IEEE_8021QAZ_APP_SEL_STREAM,
            IEEE_8021QAZ_APP_SEL_DGRAM,
            IEEE_8021QAZ_APP_SEL_ANY,
            IEEE_8021QAZ_APP_SEL_DSCP,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }
}
