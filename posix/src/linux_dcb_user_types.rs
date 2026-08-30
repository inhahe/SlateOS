//! `<uapi/linux/dcbnl.h>` — DCB (Data Center Bridging) user-facing attributes.
//!
//! User-space configuration of DCB on Ethernet NICs via the `rtnetlink`
//! socket. Defines the DCB_ATTR_* netlink attribute identifiers, PFC
//! configuration enums, and BCN (Backward Congestion Notification)
//! parameters that complement the command codes in `linux_dcb2`.

// ---------------------------------------------------------------------------
// Top-level DCB netlink attributes (struct dcbmsg payload)
// ---------------------------------------------------------------------------

pub const DCB_ATTR_UNDEFINED: u16 = 0;
pub const DCB_ATTR_IFNAME: u16 = 1;
pub const DCB_ATTR_STATE: u16 = 2;
pub const DCB_ATTR_PFC_STATE: u16 = 3;
pub const DCB_ATTR_PFC_CFG: u16 = 4;
pub const DCB_ATTR_NUM_TC: u16 = 5;
pub const DCB_ATTR_PG_CFG: u16 = 6;
pub const DCB_ATTR_SET_ALL: u16 = 7;
pub const DCB_ATTR_PERM_HWADDR: u16 = 8;
pub const DCB_ATTR_CAP: u16 = 9;
pub const DCB_ATTR_NUMTCS: u16 = 10;
pub const DCB_ATTR_BCN: u16 = 11;
pub const DCB_ATTR_APP: u16 = 12;
pub const DCB_ATTR_IEEE: u16 = 13;
pub const DCB_ATTR_DCBX: u16 = 14;
pub const DCB_ATTR_FEATCFG: u16 = 15;
pub const DCB_ATTR_CEE: u16 = 16;
pub const __DCB_ATTR_ENUM_MAX: u16 = 17;

// ---------------------------------------------------------------------------
// DCB_ATTR_PFC_CFG nested attribute set (DCB_PFC_UP_ATTR_*)
// ---------------------------------------------------------------------------

pub const DCB_PFC_UP_ATTR_UNDEFINED: u16 = 0;
pub const DCB_PFC_UP_ATTR_0: u16 = 1;
pub const DCB_PFC_UP_ATTR_1: u16 = 2;
pub const DCB_PFC_UP_ATTR_2: u16 = 3;
pub const DCB_PFC_UP_ATTR_3: u16 = 4;
pub const DCB_PFC_UP_ATTR_4: u16 = 5;
pub const DCB_PFC_UP_ATTR_5: u16 = 6;
pub const DCB_PFC_UP_ATTR_6: u16 = 7;
pub const DCB_PFC_UP_ATTR_7: u16 = 8;
pub const DCB_PFC_UP_ATTR_ALL: u16 = 9;

// ---------------------------------------------------------------------------
// PFC values (Priority Flow Control "enabled" state)
// ---------------------------------------------------------------------------

pub const DCB_PFC_DISABLED: u8 = 0;
pub const DCB_PFC_ENABLED: u8 = 1;

// ---------------------------------------------------------------------------
// DCB_ATTR_CAP nested set
// ---------------------------------------------------------------------------

pub const DCB_CAP_ATTR_UNDEFINED: u16 = 0;
pub const DCB_CAP_ATTR_ALL: u16 = 1;
pub const DCB_CAP_ATTR_PG: u16 = 2;
pub const DCB_CAP_ATTR_PFC: u16 = 3;
pub const DCB_CAP_ATTR_UP2TC: u16 = 4;
pub const DCB_CAP_ATTR_PG_TCS: u16 = 5;
pub const DCB_CAP_ATTR_PFC_TCS: u16 = 6;
pub const DCB_CAP_ATTR_GSP: u16 = 7;
pub const DCB_CAP_ATTR_BCN: u16 = 8;
pub const DCB_CAP_ATTR_DCBX: u16 = 9;

// ---------------------------------------------------------------------------
// BCN (Backward Congestion Notification) parameter ranges
// ---------------------------------------------------------------------------

/// BCN sampling weight: log2 of the rate-adjustment increment.
pub const BCN_W_MIN: u8 = 0;
pub const BCN_W_MAX: u8 = 31;
/// BCN feedback gain.
pub const BCN_GD_MIN: u8 = 0;
pub const BCN_GD_MAX: u8 = 63;

// ---------------------------------------------------------------------------
// Permanent HW address length (Ethernet MAC)
// ---------------------------------------------------------------------------

pub const DCB_PERM_HWADDR_LEN: usize = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_top_level_attrs_dense_0_to_16() {
        let a = [
            DCB_ATTR_UNDEFINED,
            DCB_ATTR_IFNAME,
            DCB_ATTR_STATE,
            DCB_ATTR_PFC_STATE,
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
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(__DCB_ATTR_ENUM_MAX as usize, a.len());
    }

    #[test]
    fn test_pfc_up_attrs_dense_0_to_9() {
        let p = [
            DCB_PFC_UP_ATTR_UNDEFINED,
            DCB_PFC_UP_ATTR_0,
            DCB_PFC_UP_ATTR_1,
            DCB_PFC_UP_ATTR_2,
            DCB_PFC_UP_ATTR_3,
            DCB_PFC_UP_ATTR_4,
            DCB_PFC_UP_ATTR_5,
            DCB_PFC_UP_ATTR_6,
            DCB_PFC_UP_ATTR_7,
            DCB_PFC_UP_ATTR_ALL,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_pfc_up_attrs_cover_all_8_priorities() {
        // PFC_UP_ATTR_0..PFC_UP_ATTR_7 = 8 entries.
        assert_eq!(DCB_PFC_UP_ATTR_7 - DCB_PFC_UP_ATTR_0 + 1, 8);
    }

    #[test]
    fn test_pfc_disabled_enabled_distinct() {
        assert_ne!(DCB_PFC_DISABLED, DCB_PFC_ENABLED);
        assert_eq!(DCB_PFC_DISABLED, 0);
        assert_eq!(DCB_PFC_ENABLED, 1);
    }

    #[test]
    fn test_cap_attrs_dense_0_to_9() {
        let c = [
            DCB_CAP_ATTR_UNDEFINED,
            DCB_CAP_ATTR_ALL,
            DCB_CAP_ATTR_PG,
            DCB_CAP_ATTR_PFC,
            DCB_CAP_ATTR_UP2TC,
            DCB_CAP_ATTR_PG_TCS,
            DCB_CAP_ATTR_PFC_TCS,
            DCB_CAP_ATTR_GSP,
            DCB_CAP_ATTR_BCN,
            DCB_CAP_ATTR_DCBX,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_bcn_ranges_fit_5_and_6_bits() {
        assert_eq!(BCN_W_MAX, 31);
        assert!((BCN_W_MAX as u32) < (1u32 << 5));
        assert_eq!(BCN_GD_MAX, 63);
        assert!((BCN_GD_MAX as u32) < (1u32 << 6));
        assert_eq!(BCN_W_MIN, 0);
        assert_eq!(BCN_GD_MIN, 0);
    }

    #[test]
    fn test_hwaddr_len_is_eth_alen() {
        assert_eq!(DCB_PERM_HWADDR_LEN, 6);
    }
}
