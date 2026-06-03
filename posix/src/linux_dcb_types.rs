//! `<linux/dcbnl.h>` — Data Center Bridging (DCB) constants.
//!
//! DCB provides lossless Ethernet features for data center
//! networks.  These constants define DCB attribute types,
//! priority group parameters, PFC (Priority Flow Control)
//! settings, and IEEE 802.1Qaz parameters.

// ---------------------------------------------------------------------------
// DCB command types (DCB_CMD_*)
// ---------------------------------------------------------------------------

/// Get IEEE parameters.
pub const DCB_CMD_IEEE_GET: u32 = 0;
/// Set IEEE parameters.
pub const DCB_CMD_IEEE_SET: u32 = 1;
/// Delete IEEE parameters.
pub const DCB_CMD_IEEE_DEL: u32 = 2;
/// Get DCBx state.
pub const DCB_CMD_GDCBX: u32 = 3;
/// Set DCBx state.
pub const DCB_CMD_SDCBX: u32 = 4;
/// Get feature configuration.
pub const DCB_CMD_GFEATCFG: u32 = 5;
/// Set feature configuration.
pub const DCB_CMD_SFEATCFG: u32 = 6;
/// Get CEE (Converged Enhanced Ethernet) params.
pub const DCB_CMD_CEE_GET: u32 = 7;

// ---------------------------------------------------------------------------
// DCB attribute types (DCB_ATTR_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DCB_ATTR_UNDEFINED: u32 = 0;
/// Interface name.
pub const DCB_ATTR_IFNAME: u32 = 1;
/// State (enabled/disabled).
pub const DCB_ATTR_STATE: u32 = 2;
/// Priority flow control config.
pub const DCB_ATTR_PFC_CFG: u32 = 3;
/// Number of traffic classes.
pub const DCB_ATTR_NUM_TC: u32 = 4;
/// Priority group config.
pub const DCB_ATTR_PG_CFG: u32 = 5;
/// Priority group config for TX.
pub const DCB_ATTR_SET_ALL: u32 = 6;
/// Permission attribute.
pub const DCB_ATTR_PERM_HWADDR: u32 = 7;
/// Capabilities.
pub const DCB_ATTR_CAP: u32 = 8;
/// Number of PFC TCs.
pub const DCB_ATTR_NUMTCS: u32 = 9;
/// BCN (Backward Congestion Notification).
pub const DCB_ATTR_BCN: u32 = 10;
/// Application priority.
pub const DCB_ATTR_APP: u32 = 11;
/// IEEE parameters.
pub const DCB_ATTR_IEEE: u32 = 12;
/// DCBx mode.
pub const DCB_ATTR_DCBX: u32 = 13;
/// Feature config.
pub const DCB_ATTR_FEATCFG: u32 = 14;
/// CEE params.
pub const DCB_ATTR_CEE: u32 = 15;

// ---------------------------------------------------------------------------
// DCBx mode flags
// ---------------------------------------------------------------------------

/// IEEE 802.1Qaz mode.
pub const DCB_CAP_DCBX_VER_IEEE: u8 = 1 << 0;
/// CEE (Intel) mode.
pub const DCB_CAP_DCBX_VER_CEE: u8 = 1 << 1;
/// Static configuration.
pub const DCB_CAP_DCBX_STATIC: u8 = 1 << 2;
/// Host mode.
pub const DCB_CAP_DCBX_HOST: u8 = 1 << 3;
/// LLD managed.
pub const DCB_CAP_DCBX_LLD_MANAGED: u8 = 1 << 4;

// ---------------------------------------------------------------------------
// IEEE attribute types (DCB_ATTR_IEEE_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DCB_ATTR_IEEE_UNSPEC: u32 = 0;
/// ETS (Enhanced Transmission Selection).
pub const DCB_ATTR_IEEE_ETS: u32 = 1;
/// PFC (Priority Flow Control).
pub const DCB_ATTR_IEEE_PFC: u32 = 2;
/// Application priority table.
pub const DCB_ATTR_IEEE_APP_TABLE: u32 = 3;
/// Peer ETS.
pub const DCB_ATTR_IEEE_PEER_ETS: u32 = 4;
/// Peer PFC.
pub const DCB_ATTR_IEEE_PEER_PFC: u32 = 5;
/// Peer application table.
pub const DCB_ATTR_IEEE_PEER_APP: u32 = 6;
/// Maximum rate.
pub const DCB_ATTR_IEEE_MAXRATE: u32 = 7;
/// QCN (Quantized Congestion Notification).
pub const DCB_ATTR_IEEE_QCN: u32 = 8;
/// QCN stats.
pub const DCB_ATTR_IEEE_QCN_STATS: u32 = 9;

// ---------------------------------------------------------------------------
// Maximum traffic classes
// ---------------------------------------------------------------------------

/// Maximum number of IEEE 802.1Q traffic classes.
pub const IEEE_8021QAZ_MAX_TCS: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_types_distinct() {
        let cmds = [
            DCB_CMD_IEEE_GET,
            DCB_CMD_IEEE_SET,
            DCB_CMD_IEEE_DEL,
            DCB_CMD_GDCBX,
            DCB_CMD_SDCBX,
            DCB_CMD_GFEATCFG,
            DCB_CMD_SFEATCFG,
            DCB_CMD_CEE_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attr_types_distinct() {
        let attrs = [
            DCB_ATTR_UNDEFINED,
            DCB_ATTR_IFNAME,
            DCB_ATTR_STATE,
            DCB_ATTR_PFC_CFG,
            DCB_ATTR_NUM_TC,
            DCB_ATTR_PG_CFG,
            DCB_ATTR_SET_ALL,
            DCB_ATTR_PERM_HWADDR,
            DCB_ATTR_CAP,
            DCB_ATTR_NUMTCS,
            DCB_ATTR_BCN,
            DCB_ATTR_APP,
            DCB_ATTR_IEEE,
            DCB_ATTR_DCBX,
            DCB_ATTR_FEATCFG,
            DCB_ATTR_CEE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_dcbx_flags_powers_of_two() {
        let flags = [
            DCB_CAP_DCBX_VER_IEEE,
            DCB_CAP_DCBX_VER_CEE,
            DCB_CAP_DCBX_STATIC,
            DCB_CAP_DCBX_HOST,
            DCB_CAP_DCBX_LLD_MANAGED,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_dcbx_flags_no_overlap() {
        let flags: [u8; 5] = [
            DCB_CAP_DCBX_VER_IEEE,
            DCB_CAP_DCBX_VER_CEE,
            DCB_CAP_DCBX_STATIC,
            DCB_CAP_DCBX_HOST,
            DCB_CAP_DCBX_LLD_MANAGED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ieee_attrs_distinct() {
        let attrs = [
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
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_max_tcs() {
        assert_eq!(IEEE_8021QAZ_MAX_TCS, 8);
    }

    #[test]
    fn test_undefined_is_zero() {
        assert_eq!(DCB_ATTR_UNDEFINED, 0);
    }
}
