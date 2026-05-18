//! `<linux/if_macvlan.h>` — MACVLAN/MACVTAP device constants.
//!
//! MACVLAN creates virtual interfaces based on MAC addresses
//! on a parent device.  These constants define MACVLAN
//! attribute types, modes, and flags.

// ---------------------------------------------------------------------------
// MACVLAN netlink attribute types (IFLA_MACVLAN_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_MACVLAN_UNSPEC: u32 = 0;
/// Mode.
pub const IFLA_MACVLAN_MODE: u32 = 1;
/// Flags.
pub const IFLA_MACVLAN_FLAGS: u32 = 2;
/// Source list (MAC address list).
pub const IFLA_MACVLAN_MACADDR_MODE: u32 = 3;
/// Individual MAC address.
pub const IFLA_MACVLAN_MACADDR: u32 = 4;
/// MAC address data.
pub const IFLA_MACVLAN_MACADDR_DATA: u32 = 5;
/// MAC address count.
pub const IFLA_MACVLAN_MACADDR_COUNT: u32 = 6;
/// Broadcast cutoff.
pub const IFLA_MACVLAN_BC_CUTOFF: u32 = 7;

// ---------------------------------------------------------------------------
// MACVLAN modes (MACVLAN_MODE_*)
// ---------------------------------------------------------------------------

/// Private mode (no communication between macvlans).
pub const MACVLAN_MODE_PRIVATE: u32 = 1;
/// VEPA mode (Virtual Ethernet Port Aggregator).
pub const MACVLAN_MODE_VEPA: u32 = 2;
/// Bridge mode (macvlans can communicate).
pub const MACVLAN_MODE_BRIDGE: u32 = 4;
/// Passthrough mode (single macvlan takes all traffic).
pub const MACVLAN_MODE_PASSTHRU: u32 = 8;
/// Source mode (filter by source MAC).
pub const MACVLAN_MODE_SOURCE: u32 = 16;

// ---------------------------------------------------------------------------
// MACVLAN flags
// ---------------------------------------------------------------------------

/// Don't strip 802.1Q headers.
pub const MACVLAN_FLAG_NOPROMISC: u32 = 1;
/// Don't forward to parent.
pub const MACVLAN_FLAG_NODST: u32 = 2;

// ---------------------------------------------------------------------------
// MACVLAN source mode operations
// ---------------------------------------------------------------------------

/// Add source MAC.
pub const MACVLAN_MACADDR_ADD: u32 = 0;
/// Delete source MAC.
pub const MACVLAN_MACADDR_DEL: u32 = 1;
/// Flush all source MACs.
pub const MACVLAN_MACADDR_FLUSH: u32 = 2;
/// Set source MAC list.
pub const MACVLAN_MACADDR_SET: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            IFLA_MACVLAN_UNSPEC, IFLA_MACVLAN_MODE,
            IFLA_MACVLAN_FLAGS, IFLA_MACVLAN_MACADDR_MODE,
            IFLA_MACVLAN_MACADDR, IFLA_MACVLAN_MACADDR_DATA,
            IFLA_MACVLAN_MACADDR_COUNT, IFLA_MACVLAN_BC_CUTOFF,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_modes_powers_of_two() {
        let modes = [
            MACVLAN_MODE_PRIVATE, MACVLAN_MODE_VEPA,
            MACVLAN_MODE_BRIDGE, MACVLAN_MODE_PASSTHRU,
            MACVLAN_MODE_SOURCE,
        ];
        for m in &modes {
            assert!(m.is_power_of_two());
        }
    }

    #[test]
    fn test_modes_no_overlap() {
        let modes = [
            MACVLAN_MODE_PRIVATE, MACVLAN_MODE_VEPA,
            MACVLAN_MODE_BRIDGE, MACVLAN_MODE_PASSTHRU,
            MACVLAN_MODE_SOURCE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_macaddr_ops_distinct() {
        let ops = [
            MACVLAN_MACADDR_ADD, MACVLAN_MACADDR_DEL,
            MACVLAN_MACADDR_FLUSH, MACVLAN_MACADDR_SET,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(IFLA_MACVLAN_UNSPEC, 0);
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(MACVLAN_FLAG_NOPROMISC & MACVLAN_FLAG_NODST, 0);
    }
}
