//! `<linux/dcbnl.h>` — Additional DCB (Data Center Bridging) constants.
//!
//! Supplementary DCB constants covering traffic class types,
//! PFC settings, and IEEE 802.1Qaz parameters.

// ---------------------------------------------------------------------------
// DCB priority groups (traffic classes)
// ---------------------------------------------------------------------------

/// Maximum number of traffic classes.
pub const DCB_MAX_TCS: u32 = 8;
/// Maximum number of priority groups.
pub const DCB_MAX_PGS: u32 = 8;
/// Maximum number of user priorities.
pub const DCB_MAX_USER_PRIORITIES: u32 = 8;

// ---------------------------------------------------------------------------
// DCB TC (Traffic Class) bandwidth type
// ---------------------------------------------------------------------------

/// TC bandwidth: group strict.
pub const DCB_TC_BW_GROUP_STRICT: u32 = 0;
/// TC bandwidth: percent of link.
pub const DCB_TC_BW_PERCENT: u32 = 1;

// ---------------------------------------------------------------------------
// DCB attributes (DCB_ATTR_*)
// ---------------------------------------------------------------------------

/// IEEE 802.1Qaz support.
pub const DCB_ATTR_IEEE: u32 = 1;
/// DCBx CEE support.
pub const DCB_ATTR_DCBx: u32 = 2;
/// Feature support.
pub const DCB_ATTR_FEATCFG: u32 = 3;

// ---------------------------------------------------------------------------
// DCBx operating modes
// ---------------------------------------------------------------------------

/// IEEE DCBx mode.
pub const DCB_CAP_DCBX_HOST: u32 = 1 << 0;
/// LLD-managed DCBx.
pub const DCB_CAP_DCBX_LLD_MANAGED: u32 = 1 << 1;
/// DCBx version: IEEE.
pub const DCB_CAP_DCBX_VER_IEEE: u32 = 1 << 2;
/// DCBx version: CEE.
pub const DCB_CAP_DCBX_VER_CEE: u32 = 1 << 3;
/// Static mode (no exchange).
pub const DCB_CAP_DCBX_STATIC: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// IEEE 802.1Qaz TSA (Transmission Selection Algorithm)
// ---------------------------------------------------------------------------

/// Strict priority.
pub const IEEE_8021QAZ_TSA_STRICT: u32 = 0;
/// Credit-based shaper.
pub const IEEE_8021QAZ_TSA_CBS: u32 = 1;
/// Enhanced transmission selection.
pub const IEEE_8021QAZ_TSA_ETS: u32 = 2;
/// Vendor specific.
pub const IEEE_8021QAZ_TSA_VENDOR: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_values() {
        assert_eq!(DCB_MAX_TCS, 8);
        assert_eq!(DCB_MAX_PGS, 8);
        assert_eq!(DCB_MAX_USER_PRIORITIES, 8);
    }

    #[test]
    fn test_bw_types_distinct() {
        assert_ne!(DCB_TC_BW_GROUP_STRICT, DCB_TC_BW_PERCENT);
    }

    #[test]
    fn test_cap_dcbx_flags_power_of_two() {
        let flags = [
            DCB_CAP_DCBX_HOST, DCB_CAP_DCBX_LLD_MANAGED,
            DCB_CAP_DCBX_VER_IEEE, DCB_CAP_DCBX_VER_CEE,
            DCB_CAP_DCBX_STATIC,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_cap_dcbx_flags_no_overlap() {
        let flags = [
            DCB_CAP_DCBX_HOST, DCB_CAP_DCBX_LLD_MANAGED,
            DCB_CAP_DCBX_VER_IEEE, DCB_CAP_DCBX_VER_CEE,
            DCB_CAP_DCBX_STATIC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tsa_values_distinct() {
        let tsas = [
            IEEE_8021QAZ_TSA_STRICT, IEEE_8021QAZ_TSA_CBS,
            IEEE_8021QAZ_TSA_ETS, IEEE_8021QAZ_TSA_VENDOR,
        ];
        for i in 0..tsas.len() {
            for j in (i + 1)..tsas.len() {
                assert_ne!(tsas[i], tsas[j]);
            }
        }
    }
}
