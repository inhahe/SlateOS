//! `<linux/if_ipvlan.h>` — IPVLAN constants.
//!
//! IPVLAN creates virtual interfaces that share the parent's
//! MAC address but have distinct IP addresses. Unlike MACVLAN,
//! all IPVLAN devices share one MAC, making it suitable for
//! environments that restrict MAC addresses (cloud providers).

// ---------------------------------------------------------------------------
// IPVLAN modes
// ---------------------------------------------------------------------------

/// L2 mode (bridge-like, forwards at layer 2).
pub const IPVLAN_MODE_L2: u32 = 0;
/// L3 mode (routes at layer 3, no broadcast/multicast).
pub const IPVLAN_MODE_L3: u32 = 1;
/// L3S mode (L3 with source routing for policy compliance).
pub const IPVLAN_MODE_L3S: u32 = 2;

// ---------------------------------------------------------------------------
// IPVLAN flags
// ---------------------------------------------------------------------------

/// Bridge mode (allow inter-slave communication).
pub const IPVLAN_F_BRIDGE: u32 = 1 << 0;
/// Private mode (no inter-slave communication).
pub const IPVLAN_F_PRIVATE: u32 = 1 << 1;
/// VEPA mode (traffic via external switch).
pub const IPVLAN_F_VEPA: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Link type
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND for ipvlan.
pub const IPVLAN_KIND: &str = "ipvlan";
/// IFLA_INFO_KIND for ipvtap (ipvlan + TAP).
pub const IPVTAP_KIND: &str = "ipvtap";

// ---------------------------------------------------------------------------
// Mode name strings
// ---------------------------------------------------------------------------

/// "l2"
pub const IPVLAN_MODE_NAME_L2: &str = "l2";
/// "l3"
pub const IPVLAN_MODE_NAME_L3: &str = "l3";
/// "l3s"
pub const IPVLAN_MODE_NAME_L3S: &str = "l3s";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [IPVLAN_MODE_L2, IPVLAN_MODE_L3, IPVLAN_MODE_L3S];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [IPVLAN_F_BRIDGE, IPVLAN_F_PRIVATE, IPVLAN_F_VEPA];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [IPVLAN_F_BRIDGE, IPVLAN_F_PRIVATE, IPVLAN_F_VEPA];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_kinds_distinct() {
        assert_ne!(IPVLAN_KIND, IPVTAP_KIND);
    }

    #[test]
    fn test_mode_names_distinct() {
        let names = [
            IPVLAN_MODE_NAME_L2,
            IPVLAN_MODE_NAME_L3,
            IPVLAN_MODE_NAME_L3S,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }
}
