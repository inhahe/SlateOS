//! `<linux/dcbnl.h>` — Data Center Bridging (DCB) constants.
//!
//! DCB extends Ethernet for data center use with priority-based
//! flow control (PFC), enhanced transmission selection (ETS), and
//! congestion notification (CN). These features enable lossless
//! Ethernet for storage (RoCE, FCoE) alongside best-effort traffic.

// ---------------------------------------------------------------------------
// Traffic classes (priorities 0-7)
// ---------------------------------------------------------------------------

/// Maximum number of traffic classes.
pub const DCB_MAX_TCS: u8 = 8;
/// Maximum priority groups.
pub const DCB_MAX_PGS: u8 = 8;

// ---------------------------------------------------------------------------
// DCB attributes (netlink)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DCB_ATTR_UNDEFINED: u16 = 0;
/// Interface name.
pub const DCB_ATTR_IFNAME: u16 = 1;
/// State (enabled/disabled).
pub const DCB_ATTR_STATE: u16 = 2;
/// PFC configuration.
pub const DCB_ATTR_PFC_CFG: u16 = 3;
/// PFC state.
pub const DCB_ATTR_PFC_STATE: u16 = 4;
/// ETS configuration.
pub const DCB_ATTR_ETS: u16 = 5;
/// APP priority configuration.
pub const DCB_ATTR_APP: u16 = 6;
/// IEEE mode.
pub const DCB_ATTR_IEEE: u16 = 7;

// ---------------------------------------------------------------------------
// ETS (Enhanced Transmission Selection) modes
// ---------------------------------------------------------------------------

/// Strict priority.
pub const DCB_ETS_STRICT: u8 = 0;
/// Credit-based shaper.
pub const DCB_ETS_CBS: u8 = 1;
/// ETS algorithm.
pub const DCB_ETS_ETS: u8 = 2;
/// Vendor specific.
pub const DCB_ETS_VENDOR: u8 = 255;

// ---------------------------------------------------------------------------
// APP protocol selectors
// ---------------------------------------------------------------------------

/// EtherType selector.
pub const DCB_APP_SEL_ETHERTYPE: u8 = 1;
/// TCP/SCTP port selector.
pub const DCB_APP_SEL_STREAM: u8 = 2;
/// UDP/DCCP port selector.
pub const DCB_APP_SEL_DGRAM: u8 = 3;
/// TCP/UDP/SCTP/DCCP port selector.
pub const DCB_APP_SEL_ANY: u8 = 4;
/// DSCP selector.
pub const DCB_APP_SEL_DSCP: u8 = 5;

// ---------------------------------------------------------------------------
// Common DCB app priority assignments
// ---------------------------------------------------------------------------

/// FCoE default priority.
pub const DCB_APP_FCOE_PRIO: u8 = 3;
/// iSCSI default priority.
pub const DCB_APP_ISCSI_PRIO: u8 = 4;
/// RoCE default priority.
pub const DCB_APP_ROCE_PRIO: u8 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            DCB_ATTR_UNDEFINED, DCB_ATTR_IFNAME, DCB_ATTR_STATE,
            DCB_ATTR_PFC_CFG, DCB_ATTR_PFC_STATE, DCB_ATTR_ETS,
            DCB_ATTR_APP, DCB_ATTR_IEEE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ets_modes_distinct() {
        let modes = [DCB_ETS_STRICT, DCB_ETS_CBS, DCB_ETS_ETS, DCB_ETS_VENDOR];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_app_selectors_distinct() {
        let sels = [
            DCB_APP_SEL_ETHERTYPE, DCB_APP_SEL_STREAM,
            DCB_APP_SEL_DGRAM, DCB_APP_SEL_ANY, DCB_APP_SEL_DSCP,
        ];
        for i in 0..sels.len() {
            for j in (i + 1)..sels.len() {
                assert_ne!(sels[i], sels[j]);
            }
        }
    }

    #[test]
    fn test_app_priorities_distinct() {
        let prios = [DCB_APP_FCOE_PRIO, DCB_APP_ISCSI_PRIO, DCB_APP_ROCE_PRIO];
        for i in 0..prios.len() {
            for j in (i + 1)..prios.len() {
                assert_ne!(prios[i], prios[j]);
            }
        }
    }

    #[test]
    fn test_max_tcs() {
        assert_eq!(DCB_MAX_TCS, 8);
    }
}
