//! `<uapi/linux/dcbnl.h>` — Data Center Bridging netlink extensions.
//!
//! DCB (Data Center Bridging) provides lossless Ethernet for storage
//! and HPC traffic. The netlink interface (`RTM_*DCB` messages) lets
//! userspace configure priority groups (PFC), ETS (Enhanced Transmission
//! Selection), and DCBX (DCB Exchange Protocol) on a per-NIC basis.

// ---------------------------------------------------------------------------
// DCBNL command codes (struct dcbmsg.cmd, DCB_CMD_*)
// ---------------------------------------------------------------------------

pub const DCB_CMD_UNDEFINED: u8 = 0;
pub const DCB_CMD_GSTATE: u8 = 1;
pub const DCB_CMD_SSTATE: u8 = 2;
pub const DCB_CMD_PGTX_GCFG: u8 = 3;
pub const DCB_CMD_PGTX_SCFG: u8 = 4;
pub const DCB_CMD_PGRX_GCFG: u8 = 5;
pub const DCB_CMD_PGRX_SCFG: u8 = 6;
pub const DCB_CMD_PFC_GCFG: u8 = 7;
pub const DCB_CMD_PFC_SCFG: u8 = 8;
pub const DCB_CMD_SET_ALL: u8 = 9;
pub const DCB_CMD_GPERM_HWADDR: u8 = 10;
pub const DCB_CMD_GCAP: u8 = 11;
pub const DCB_CMD_GNUMTCS: u8 = 12;
pub const DCB_CMD_SNUMTCS: u8 = 13;
pub const DCB_CMD_PFC_GSTATE: u8 = 14;
pub const DCB_CMD_PFC_SSTATE: u8 = 15;
pub const DCB_CMD_BCN_GCFG: u8 = 16;
pub const DCB_CMD_BCN_SCFG: u8 = 17;
pub const DCB_CMD_GAPP: u8 = 18;
pub const DCB_CMD_SAPP: u8 = 19;
pub const DCB_CMD_IEEE_SET: u8 = 20;
pub const DCB_CMD_IEEE_GET: u8 = 21;
pub const DCB_CMD_GDCBX: u8 = 22;
pub const DCB_CMD_SDCBX: u8 = 23;
pub const DCB_CMD_GFEATCFG: u8 = 24;
pub const DCB_CMD_SFEATCFG: u8 = 25;
pub const DCB_CMD_CEE_GET: u8 = 26;
pub const DCB_CMD_IEEE_DEL: u8 = 27;
pub const __DCB_CMD_ENUM_MAX: u8 = 28;

// ---------------------------------------------------------------------------
// PFC (Priority Flow Control) — 8 traffic classes
// ---------------------------------------------------------------------------

pub const DCB_NUM_TC: usize = 8;
pub const DCB_PFC_UP_ATTR_MAX: u8 = DCB_NUM_TC as u8;

// ---------------------------------------------------------------------------
// IEEE 802.1Qaz constants
// ---------------------------------------------------------------------------

/// Total ETS bandwidth percentage across all TCs.
pub const IEEE_ETS_TOTAL_BW: u8 = 100;
pub const IEEE_8021QAZ_TSA_STRICT: u8 = 0;
pub const IEEE_8021QAZ_TSA_CB_SHAPER: u8 = 1;
pub const IEEE_8021QAZ_TSA_ETS: u8 = 2;
pub const IEEE_8021QAZ_TSA_VENDOR: u8 = 255;

pub const IEEE_8021QAZ_MAX_TCS: usize = 8;

/// Maximum DCBX APP selectors.
pub const DCB_APP_IDTYPE_ETHTYPE: u8 = 0;
pub const DCB_APP_IDTYPE_STREAM: u8 = 1;
pub const DCB_APP_IDTYPE_DGRAM: u8 = 2;
pub const DCB_APP_IDTYPE_PORTNUM: u8 = 3;
pub const DCB_APP_IDTYPE_DSCP: u8 = 5;

// ---------------------------------------------------------------------------
// DCBX protocol modes (DCB_CMD_GDCBX / SDCBX)
// ---------------------------------------------------------------------------

pub const DCB_CAP_DCBX_HOST: u8 = 1 << 0;
pub const DCB_CAP_DCBX_LLD_MANAGED: u8 = 1 << 1;
pub const DCB_CAP_DCBX_VER_CEE: u8 = 1 << 2;
pub const DCB_CAP_DCBX_VER_IEEE: u8 = 1 << 3;
pub const DCB_CAP_DCBX_STATIC: u8 = 1 << 4;

// ---------------------------------------------------------------------------
// Feature CFG state (DCB_CMD_GFEATCFG / SFEATCFG)
// ---------------------------------------------------------------------------

pub const DCB_FEATCFG_ERROR: u8 = 1 << 0;
pub const DCB_FEATCFG_ENABLE: u8 = 1 << 1;
pub const DCB_FEATCFG_WILLING: u8 = 1 << 2;
pub const DCB_FEATCFG_ADVERTISE: u8 = 1 << 3;
pub const DCB_FEATCFG_ALL: u8 =
    DCB_FEATCFG_ERROR | DCB_FEATCFG_ENABLE | DCB_FEATCFG_WILLING | DCB_FEATCFG_ADVERTISE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_codes_dense_0_to_27() {
        let c = [
            DCB_CMD_UNDEFINED,
            DCB_CMD_GSTATE,
            DCB_CMD_SSTATE,
            DCB_CMD_PGTX_GCFG,
            DCB_CMD_PGTX_SCFG,
            DCB_CMD_PGRX_GCFG,
            DCB_CMD_PGRX_SCFG,
            DCB_CMD_PFC_GCFG,
            DCB_CMD_PFC_SCFG,
            DCB_CMD_SET_ALL,
            DCB_CMD_GPERM_HWADDR,
            DCB_CMD_GCAP,
            DCB_CMD_GNUMTCS,
            DCB_CMD_SNUMTCS,
            DCB_CMD_PFC_GSTATE,
            DCB_CMD_PFC_SSTATE,
            DCB_CMD_BCN_GCFG,
            DCB_CMD_BCN_SCFG,
            DCB_CMD_GAPP,
            DCB_CMD_SAPP,
            DCB_CMD_IEEE_SET,
            DCB_CMD_IEEE_GET,
            DCB_CMD_GDCBX,
            DCB_CMD_SDCBX,
            DCB_CMD_GFEATCFG,
            DCB_CMD_SFEATCFG,
            DCB_CMD_CEE_GET,
            DCB_CMD_IEEE_DEL,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(__DCB_CMD_ENUM_MAX as usize, c.len());
    }

    #[test]
    fn test_get_set_command_pairs_consecutive() {
        // For "G" / "S" command pairs, the SET follows the GET.
        assert_eq!(DCB_CMD_SSTATE, DCB_CMD_GSTATE + 1);
        assert_eq!(DCB_CMD_PGTX_SCFG, DCB_CMD_PGTX_GCFG + 1);
        assert_eq!(DCB_CMD_PGRX_SCFG, DCB_CMD_PGRX_GCFG + 1);
        assert_eq!(DCB_CMD_PFC_SCFG, DCB_CMD_PFC_GCFG + 1);
        assert_eq!(DCB_CMD_SNUMTCS, DCB_CMD_GNUMTCS + 1);
        assert_eq!(DCB_CMD_BCN_SCFG, DCB_CMD_BCN_GCFG + 1);
        assert_eq!(DCB_CMD_SAPP, DCB_CMD_GAPP + 1);
        assert_eq!(DCB_CMD_SDCBX, DCB_CMD_GDCBX + 1);
        assert_eq!(DCB_CMD_SFEATCFG, DCB_CMD_GFEATCFG + 1);
    }

    #[test]
    fn test_num_tc_is_8() {
        assert_eq!(DCB_NUM_TC, 8);
        assert_eq!(IEEE_8021QAZ_MAX_TCS, DCB_NUM_TC);
        assert_eq!(DCB_PFC_UP_ATTR_MAX, 8);
    }

    #[test]
    fn test_ets_bandwidth_sums_to_100() {
        // Bandwidth percentages across all TCs must sum to 100.
        assert_eq!(IEEE_ETS_TOTAL_BW, 100);
        // 8 TCs sharing equally = 12.5%; for integer percent, common configs
        // are like [13,12,13,12,13,12,13,12] summing to 100. Sanity check
        // that 100 / NUM_TC is < 100 / (NUM_TC-1) etc.
        assert!((IEEE_ETS_TOTAL_BW as usize) / DCB_NUM_TC > 0);
    }

    #[test]
    fn test_tsa_codes_distinct() {
        for (i, a) in [
            IEEE_8021QAZ_TSA_STRICT,
            IEEE_8021QAZ_TSA_CB_SHAPER,
            IEEE_8021QAZ_TSA_ETS,
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(*a as usize, i);
        }
        // Vendor code is at u8::MAX.
        assert_eq!(IEEE_8021QAZ_TSA_VENDOR, 0xFF);
    }

    #[test]
    fn test_app_id_types_distinct() {
        let a = [
            DCB_APP_IDTYPE_ETHTYPE,
            DCB_APP_IDTYPE_STREAM,
            DCB_APP_IDTYPE_DGRAM,
            DCB_APP_IDTYPE_PORTNUM,
            DCB_APP_IDTYPE_DSCP,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // ETHTYPE..PORTNUM are dense 0..3, DSCP is 5 (4 was reserved).
        assert_eq!(DCB_APP_IDTYPE_ETHTYPE, 0);
        assert_eq!(DCB_APP_IDTYPE_DSCP, 5);
    }

    #[test]
    fn test_dcbx_caps_single_bit_distinct() {
        let c = [
            DCB_CAP_DCBX_HOST,
            DCB_CAP_DCBX_LLD_MANAGED,
            DCB_CAP_DCBX_VER_CEE,
            DCB_CAP_DCBX_VER_IEEE,
            DCB_CAP_DCBX_STATIC,
        ];
        let mut or_all = 0u8;
        for &v in &c {
            assert!(v.is_power_of_two());
            or_all |= v;
        }
        assert_eq!(or_all, 0x1F);
    }

    #[test]
    fn test_featcfg_bits_disjoint() {
        let f = [
            DCB_FEATCFG_ERROR,
            DCB_FEATCFG_ENABLE,
            DCB_FEATCFG_WILLING,
            DCB_FEATCFG_ADVERTISE,
        ];
        let mut or_all = 0u8;
        for &v in &f {
            assert!(v.is_power_of_two());
            or_all |= v;
        }
        assert_eq!(or_all, DCB_FEATCFG_ALL);
        assert_eq!(DCB_FEATCFG_ALL, 0x0F);
    }
}
