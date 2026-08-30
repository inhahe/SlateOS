//! `<linux/llc.h>` — Additional LLC constants.
//!
//! Supplementary LLC (Logical Link Control) constants covering
//! SAP values, socket options, and control field types.

// ---------------------------------------------------------------------------
// LLC SAP values
// ---------------------------------------------------------------------------

/// Null SAP.
pub const LLC_SAP_NULL: u32 = 0x00;
/// LLC sublayer management.
pub const LLC_SAP_LLC: u32 = 0x02;
/// SNA path control.
pub const LLC_SAP_SNA: u32 = 0x04;
/// TCP/IP.
pub const LLC_SAP_PNM: u32 = 0x0E;
/// SNA.
pub const LLC_SAP_BSPAN: u32 = 0x42;
/// ISO network layer.
pub const LLC_SAP_ISO: u32 = 0xFE;
/// IP.
pub const LLC_SAP_IP: u32 = 0x06;
/// SNAP.
pub const LLC_SAP_SNAP: u32 = 0xAA;
/// Global DSAP.
pub const LLC_SAP_GLOBAL: u32 = 0xFF;

// ---------------------------------------------------------------------------
// LLC socket options
// ---------------------------------------------------------------------------

/// Operational mode.
pub const LLC_OPT_UNKNOWN: u32 = 0;
/// Connection-oriented mode 2.
pub const LLC_OPT_RETRY: u32 = 1;
/// Size.
pub const LLC_OPT_SIZE: u32 = 2;
/// Acknowledge timer.
pub const LLC_OPT_ACK_TMR_EXP: u32 = 3;
/// Peer busy timer.
pub const LLC_OPT_P_TMR_EXP: u32 = 4;
/// Reject timer.
pub const LLC_OPT_REJ_TMR_EXP: u32 = 5;
/// Busy state timer.
pub const LLC_OPT_BUSY_TMR_EXP: u32 = 6;
/// TX window.
pub const LLC_OPT_TX_WIN: u32 = 7;
/// RX window.
pub const LLC_OPT_RX_WIN: u32 = 8;
/// Retransmissions.
pub const LLC_OPT_PKTINFO: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sap_values_distinct() {
        let saps = [
            LLC_SAP_NULL,
            LLC_SAP_LLC,
            LLC_SAP_SNA,
            LLC_SAP_PNM,
            LLC_SAP_BSPAN,
            LLC_SAP_ISO,
            LLC_SAP_IP,
            LLC_SAP_SNAP,
            LLC_SAP_GLOBAL,
        ];
        for i in 0..saps.len() {
            for j in (i + 1)..saps.len() {
                assert_ne!(saps[i], saps[j]);
            }
        }
    }

    #[test]
    fn test_sockopts_distinct() {
        let opts = [
            LLC_OPT_UNKNOWN,
            LLC_OPT_RETRY,
            LLC_OPT_SIZE,
            LLC_OPT_ACK_TMR_EXP,
            LLC_OPT_P_TMR_EXP,
            LLC_OPT_REJ_TMR_EXP,
            LLC_OPT_BUSY_TMR_EXP,
            LLC_OPT_TX_WIN,
            LLC_OPT_RX_WIN,
            LLC_OPT_PKTINFO,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
