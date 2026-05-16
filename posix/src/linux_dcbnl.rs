//! `<linux/dcbnl.h>` — Data Center Bridging (DCB) Netlink constants.
//!
//! DCB is a set of IEEE 802.1 standards for lossless Ethernet in data
//! center environments. Includes PFC (Priority Flow Control), ETS
//! (Enhanced Transmission Selection), and DCBX protocol negotiation.

// ---------------------------------------------------------------------------
// DCB commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DCB_CMD_UNDEFINED: u8 = 0;
/// Get state.
pub const DCB_CMD_GSTATE: u8 = 1;
/// Set state.
pub const DCB_CMD_SSTATE: u8 = 2;
/// Get PFC config.
pub const DCB_CMD_PFC_GCFG: u8 = 3;
/// Set PFC config.
pub const DCB_CMD_PFC_SCFG: u8 = 4;
/// Get PFC state.
pub const DCB_CMD_GAPP: u8 = 5;
/// Set application priority.
pub const DCB_CMD_SAPP: u8 = 6;
/// Get peer PFC.
pub const DCB_CMD_PGTX_GCFG: u8 = 7;
/// Set TX PG config.
pub const DCB_CMD_PGTX_SCFG: u8 = 8;
/// Get RX PG config.
pub const DCB_CMD_PGRX_GCFG: u8 = 9;
/// Set RX PG config.
pub const DCB_CMD_PGRX_SCFG: u8 = 10;
/// Set all (combined config push).
pub const DCB_CMD_SET_ALL: u8 = 11;
/// Get permanent config.
pub const DCB_CMD_GPERM_HWADDR: u8 = 12;
/// Get capabilities.
pub const DCB_CMD_GCAP: u8 = 13;
/// Get number of TCs.
pub const DCB_CMD_GNUMTCS: u8 = 14;
/// Set number of TCs.
pub const DCB_CMD_SNUMTCS: u8 = 15;
/// Get PFC stats.
pub const DCB_CMD_PFC_GSTAT: u8 = 16;
/// Get IEEE config.
pub const DCB_CMD_IEEE_GET: u8 = 19;
/// Set IEEE config.
pub const DCB_CMD_IEEE_SET: u8 = 20;
/// Delete IEEE config.
pub const DCB_CMD_IEEE_DEL: u8 = 21;
/// Get DCBX mode.
pub const DCB_CMD_GDCBX: u8 = 22;
/// Set DCBX mode.
pub const DCB_CMD_SDCBX: u8 = 23;
/// Get feature config.
pub const DCB_CMD_GFEATCFG: u8 = 24;
/// Set feature config.
pub const DCB_CMD_SFEATCFG: u8 = 25;

// ---------------------------------------------------------------------------
// DCB attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DCB_ATTR_UNSPEC: u16 = 0;
/// Interface name.
pub const DCB_ATTR_IFNAME: u16 = 1;
/// State.
pub const DCB_ATTR_STATE: u16 = 2;
/// PFC state.
pub const DCB_ATTR_PFC_STATE: u16 = 3;
/// PFC config.
pub const DCB_ATTR_PFC_CFG: u16 = 4;
/// Number of TCs.
pub const DCB_ATTR_NUM_TC: u16 = 5;
/// PG config.
pub const DCB_ATTR_PG_CFG: u16 = 6;
/// Application priority.
pub const DCB_ATTR_APP: u16 = 11;
/// IEEE config (nested).
pub const DCB_ATTR_IEEE: u16 = 12;
/// DCBX mode.
pub const DCB_ATTR_DCBX: u16 = 13;
/// Feature config.
pub const DCB_ATTR_FEATCFG: u16 = 14;

// ---------------------------------------------------------------------------
// DCBX modes
// ---------------------------------------------------------------------------

/// Host mode.
pub const DCB_CAP_DCBX_HOST: u8 = 1;
/// LLD managed.
pub const DCB_CAP_DCBX_LLD_MANAGED: u8 = 2;
/// Firmware version.
pub const DCB_CAP_DCBX_VER_CEE: u8 = 4;
/// IEEE version.
pub const DCB_CAP_DCBX_VER_IEEE: u8 = 8;
/// Static config.
pub const DCB_CAP_DCBX_STATIC: u8 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            DCB_CMD_UNDEFINED, DCB_CMD_GSTATE, DCB_CMD_SSTATE,
            DCB_CMD_PFC_GCFG, DCB_CMD_PFC_SCFG, DCB_CMD_GAPP,
            DCB_CMD_SAPP, DCB_CMD_IEEE_GET, DCB_CMD_IEEE_SET,
            DCB_CMD_GDCBX, DCB_CMD_SDCBX,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            DCB_ATTR_UNSPEC, DCB_ATTR_IFNAME, DCB_ATTR_STATE,
            DCB_ATTR_PFC_STATE, DCB_ATTR_PFC_CFG, DCB_ATTR_NUM_TC,
            DCB_ATTR_APP, DCB_ATTR_IEEE, DCB_ATTR_DCBX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_dcbx_modes_powers_of_two() {
        let modes = [
            DCB_CAP_DCBX_HOST, DCB_CAP_DCBX_LLD_MANAGED,
            DCB_CAP_DCBX_VER_CEE, DCB_CAP_DCBX_VER_IEEE,
            DCB_CAP_DCBX_STATIC,
        ];
        for m in &modes {
            assert!(m.is_power_of_two(), "mode {m} not power of 2");
        }
    }
}
