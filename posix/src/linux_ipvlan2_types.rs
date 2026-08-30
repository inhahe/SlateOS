//! `<linux/if_ipvlan.h>` — Additional IPVLAN constants.
//!
//! Supplementary IPVLAN constants covering operating modes,
//! flags, and IP address types.

// ---------------------------------------------------------------------------
// IPVLAN modes
// ---------------------------------------------------------------------------

/// L2 mode (bridge at layer 2).
pub const IPVLAN_MODE_L2: u32 = 0;
/// L3 mode (route at layer 3).
pub const IPVLAN_MODE_L3: u32 = 1;
/// L3S mode (layer 3 with source check).
pub const IPVLAN_MODE_L3S: u32 = 2;

// ---------------------------------------------------------------------------
// IPVLAN flags
// ---------------------------------------------------------------------------

/// Bridge mode flag.
pub const IPVLAN_F_BRIDGE: u32 = 1 << 0;
/// Private mode flag.
pub const IPVLAN_F_PRIVATE: u32 = 1 << 1;
/// VEPA mode flag.
pub const IPVLAN_F_VEPA: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// IPVLAN address types
// ---------------------------------------------------------------------------

/// IPv4 address.
pub const IPVLAN_ADDR_IPV4: u32 = 0;
/// IPv6 address.
pub const IPVLAN_ADDR_IPV6: u32 = 1;

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
    fn test_flags_power_of_two() {
        assert!(IPVLAN_F_BRIDGE.is_power_of_two());
        assert!(IPVLAN_F_PRIVATE.is_power_of_two());
        assert!(IPVLAN_F_VEPA.is_power_of_two());
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
    fn test_addr_types_distinct() {
        assert_ne!(IPVLAN_ADDR_IPV4, IPVLAN_ADDR_IPV6);
    }

    #[test]
    fn test_l2_is_zero() {
        assert_eq!(IPVLAN_MODE_L2, 0);
    }
}
