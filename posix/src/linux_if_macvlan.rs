//! `<linux/if_macvlan.h>` — MACVLAN/MACVTAP interface constants.
//!
//! MACVLAN creates virtual network interfaces with their own MAC
//! address on top of a physical device. MACVTAP provides a tap-like
//! interface with MACVLAN semantics. Used for containers, VMs, and
//! network namespace isolation.

// ---------------------------------------------------------------------------
// MACVLAN modes
// ---------------------------------------------------------------------------

/// Private mode: no communication between macvlans on same parent.
pub const MACVLAN_MODE_PRIVATE: u32 = 1;
/// VEPA (Virtual Ethernet Port Aggregator): hairpin via external switch.
pub const MACVLAN_MODE_VEPA: u32 = 2;
/// Bridge mode: macvlans can communicate with each other.
pub const MACVLAN_MODE_BRIDGE: u32 = 4;
/// Passthrough mode: single macvlan takes over parent's MAC.
pub const MACVLAN_MODE_PASSTHRU: u32 = 8;
/// Source mode: filter by source MAC address list.
pub const MACVLAN_MODE_SOURCE: u32 = 16;

// ---------------------------------------------------------------------------
// MACVLAN flags
// ---------------------------------------------------------------------------

/// Don't fail if link is not ready.
pub const MACVLAN_FLAG_NOPROMISC: u32 = 1;
/// Don't modify link features.
pub const MACVLAN_FLAG_NODST: u32 = 2;

// ---------------------------------------------------------------------------
// MACVLAN source command
// ---------------------------------------------------------------------------

/// Add source MAC.
pub const MACVLAN_MACADDR_ADD: u32 = 0;
/// Delete source MAC.
pub const MACVLAN_MACADDR_DEL: u32 = 1;
/// Flush source MACs.
pub const MACVLAN_MACADDR_FLUSH: u32 = 2;
/// Set source MACs (replace all).
pub const MACVLAN_MACADDR_SET: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_powers_of_two() {
        let modes = [
            MACVLAN_MODE_PRIVATE, MACVLAN_MODE_VEPA,
            MACVLAN_MODE_BRIDGE, MACVLAN_MODE_PASSTHRU,
            MACVLAN_MODE_SOURCE,
        ];
        for m in &modes {
            assert!(m.is_power_of_two(), "mode {m} not power of 2");
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            MACVLAN_MODE_PRIVATE, MACVLAN_MODE_VEPA,
            MACVLAN_MODE_BRIDGE, MACVLAN_MODE_PASSTHRU,
            MACVLAN_MODE_SOURCE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_macaddr_cmds() {
        assert_eq!(MACVLAN_MACADDR_ADD, 0);
        assert_eq!(MACVLAN_MACADDR_DEL, 1);
        assert_eq!(MACVLAN_MACADDR_FLUSH, 2);
        assert_eq!(MACVLAN_MACADDR_SET, 3);
    }

    #[test]
    fn test_flags() {
        assert_eq!(MACVLAN_FLAG_NOPROMISC, 1);
        assert_eq!(MACVLAN_FLAG_NODST, 2);
    }
}
