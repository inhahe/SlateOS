//! `<linux/dcbnl.h>` — Data Center Bridging (DCB) netlink constants.
//!
//! DCB is a set of IEEE 802.1 enhancements for lossless Ethernet in
//! data centers. It includes Priority-based Flow Control (PFC,
//! 802.1Qbb), Enhanced Transmission Selection (ETS, 802.1Qaz), and
//! Congestion Notification (CN, 802.1Qau). The netlink API configures
//! these features on DCB-capable NICs. Used by lldpad, iSCSI/FCoE
//! initiators, and RDMA (RoCE) deployments that need lossless fabric.

// ---------------------------------------------------------------------------
// DCB netlink commands (DCB_CMD_*)
// ---------------------------------------------------------------------------

/// Get DCB state (enabled/disabled).
pub const DCB_CMD_GSTATE: u32 = 1;
/// Set DCB state.
pub const DCB_CMD_SSTATE: u32 = 2;
/// Get PFC (Priority-based Flow Control) configuration.
pub const DCB_CMD_PFC_GCFG: u32 = 3;
/// Set PFC configuration.
pub const DCB_CMD_PFC_SCFG: u32 = 4;
/// Get PFC statistics.
pub const DCB_CMD_PFC_GSTAT: u32 = 5;
/// Get PG (Priority Group / ETS) configuration.
pub const DCB_CMD_GPGTX: u32 = 6;
/// Set PG TX configuration.
pub const DCB_CMD_SPGTX: u32 = 7;
/// Get PG RX configuration.
pub const DCB_CMD_GPGRX: u32 = 8;
/// Set PG RX configuration.
pub const DCB_CMD_SPGRX: u32 = 9;
/// Get application priority table.
pub const DCB_CMD_GAPP: u32 = 10;
/// Set application priority table.
pub const DCB_CMD_SAPP: u32 = 11;
/// Get DCBX mode.
pub const DCB_CMD_GDCBX: u32 = 12;
/// Set DCBX mode.
pub const DCB_CMD_SDCBX: u32 = 13;
/// Get feature flags.
pub const DCB_CMD_GFEATCFG: u32 = 14;
/// Set feature flags.
pub const DCB_CMD_SFEATCFG: u32 = 15;
/// Get IEEE 802.1Qaz configuration.
pub const DCB_CMD_IEEE_GET: u32 = 18;
/// Set IEEE 802.1Qaz configuration.
pub const DCB_CMD_IEEE_SET: u32 = 19;
/// Delete IEEE 802.1Qaz configuration.
pub const DCB_CMD_IEEE_DEL: u32 = 20;
/// Get CEE (Converged Enhanced Ethernet) configuration.
pub const DCB_CMD_CEE_GET: u32 = 21;

// ---------------------------------------------------------------------------
// DCBX mode flags
// ---------------------------------------------------------------------------

/// DCBX host mode (local configuration).
pub const DCB_CAP_DCBX_HOST: u32 = 1 << 0;
/// DCBX CEE mode (pre-standard Cisco/Intel).
pub const DCB_CAP_DCBX_LLD_MANAGED: u32 = 1 << 1;
/// DCBX IEEE 802.1Qaz mode.
pub const DCB_CAP_DCBX_VER_CEE: u32 = 1 << 2;
/// DCBX IEEE 802.1Qaz version.
pub const DCB_CAP_DCBX_VER_IEEE: u32 = 1 << 3;
/// DCBX static (no protocol, manual configuration).
pub const DCB_CAP_DCBX_STATIC: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// PFC configuration
// ---------------------------------------------------------------------------

/// Maximum number of traffic classes (priorities 0-7).
pub const DCB_PFC_MAX_TCS: u32 = 8;

// ---------------------------------------------------------------------------
// ETS (Enhanced Transmission Selection) traffic classes
// ---------------------------------------------------------------------------

/// Maximum number of ETS traffic classes.
pub const DCB_ETS_MAX_TCS: u32 = 8;
/// Strict priority scheduling.
pub const DCB_ETS_TSA_STRICT: u32 = 0;
/// Credit-based shaper (CBS, 802.1Qav).
pub const DCB_ETS_TSA_CB_SHAPER: u32 = 1;
/// ETS algorithm (802.1Qaz bandwidth sharing).
pub const DCB_ETS_TSA_ETS: u32 = 2;
/// Vendor specific scheduling.
pub const DCB_ETS_TSA_VENDOR: u32 = 255;

// ---------------------------------------------------------------------------
// DCB general attributes (DCB_ATTR_*)
// ---------------------------------------------------------------------------

/// Interface name attribute.
pub const DCB_ATTR_IFNAME: u32 = 1;
/// State (enabled/disabled) attribute.
pub const DCB_ATTR_STATE: u32 = 2;
/// PFC state attribute.
pub const DCB_ATTR_PFC_STATE: u32 = 3;
/// PFC configuration attribute.
pub const DCB_ATTR_PFC_CFG: u32 = 4;
/// Priority group attribute.
pub const DCB_ATTR_PG_CFG: u32 = 5;
/// Application priority attribute.
pub const DCB_ATTR_APP: u32 = 6;
/// IEEE 802.1Qaz attribute.
pub const DCB_ATTR_IEEE: u32 = 7;
/// DCBX mode attribute.
pub const DCB_ATTR_DCBX: u32 = 8;
/// Feature configuration attribute.
pub const DCB_ATTR_FEATCFG: u32 = 9;
/// CEE attribute.
pub const DCB_ATTR_CEE: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            DCB_CMD_GSTATE, DCB_CMD_SSTATE,
            DCB_CMD_PFC_GCFG, DCB_CMD_PFC_SCFG,
            DCB_CMD_PFC_GSTAT,
            DCB_CMD_GPGTX, DCB_CMD_SPGTX,
            DCB_CMD_GPGRX, DCB_CMD_SPGRX,
            DCB_CMD_GAPP, DCB_CMD_SAPP,
            DCB_CMD_GDCBX, DCB_CMD_SDCBX,
            DCB_CMD_GFEATCFG, DCB_CMD_SFEATCFG,
            DCB_CMD_IEEE_GET, DCB_CMD_IEEE_SET, DCB_CMD_IEEE_DEL,
            DCB_CMD_CEE_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dcbx_caps_no_overlap() {
        let caps = [
            DCB_CAP_DCBX_HOST, DCB_CAP_DCBX_LLD_MANAGED,
            DCB_CAP_DCBX_VER_CEE, DCB_CAP_DCBX_VER_IEEE,
            DCB_CAP_DCBX_STATIC,
        ];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_ets_tsa_distinct() {
        let tsa = [
            DCB_ETS_TSA_STRICT, DCB_ETS_TSA_CB_SHAPER,
            DCB_ETS_TSA_ETS, DCB_ETS_TSA_VENDOR,
        ];
        for i in 0..tsa.len() {
            for j in (i + 1)..tsa.len() {
                assert_ne!(tsa[i], tsa[j]);
            }
        }
    }

    #[test]
    fn test_max_tcs() {
        assert_eq!(DCB_PFC_MAX_TCS, 8);
        assert_eq!(DCB_ETS_MAX_TCS, 8);
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            DCB_ATTR_IFNAME, DCB_ATTR_STATE,
            DCB_ATTR_PFC_STATE, DCB_ATTR_PFC_CFG,
            DCB_ATTR_PG_CFG, DCB_ATTR_APP,
            DCB_ATTR_IEEE, DCB_ATTR_DCBX,
            DCB_ATTR_FEATCFG, DCB_ATTR_CEE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
