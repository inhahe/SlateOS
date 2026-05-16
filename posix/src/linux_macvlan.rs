//! `<linux/if_macvlan.h>` — MACVLAN/MACVTAP constants.
//!
//! MACVLAN creates virtual interfaces with unique MAC addresses
//! on top of a physical interface. Each MACVLAN device acts as
//! an independent network endpoint. MACVTAP adds a TAP interface
//! to allow direct userspace access (used by VMs).

// ---------------------------------------------------------------------------
// MACVLAN modes
// ---------------------------------------------------------------------------

/// Private mode (no communication between macvlan devices).
pub const MACVLAN_MODE_PRIVATE: u32 = 1;
/// VEPA mode (all traffic goes via external switch).
pub const MACVLAN_MODE_VEPA: u32 = 2;
/// Bridge mode (macvlan devices can communicate directly).
pub const MACVLAN_MODE_BRIDGE: u32 = 4;
/// Passthru mode (single macvlan, takes over parent device).
pub const MACVLAN_MODE_PASSTHRU: u32 = 8;
/// Source mode (filter by source MAC address list).
pub const MACVLAN_MODE_SOURCE: u32 = 16;

// ---------------------------------------------------------------------------
// MACVLAN flags
// ---------------------------------------------------------------------------

/// No carrier flag (link is always up).
pub const MACVLAN_FLAG_NOPROMISC: u32 = 1;
/// No destination address check.
pub const MACVLAN_FLAG_NODST: u32 = 2;

// ---------------------------------------------------------------------------
// Link types
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND for macvlan.
pub const MACVLAN_KIND: &str = "macvlan";
/// IFLA_INFO_KIND for macvtap.
pub const MACVTAP_KIND: &str = "macvtap";

// ---------------------------------------------------------------------------
// Source mode operations
// ---------------------------------------------------------------------------

/// Add source MAC entry.
pub const MACVLAN_MACADDR_ADD: u32 = 0;
/// Delete source MAC entry.
pub const MACVLAN_MACADDR_DEL: u32 = 1;
/// Flush all source MAC entries.
pub const MACVLAN_MACADDR_FLUSH: u32 = 2;
/// Set source MAC list (replace all).
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
        for mode in &modes {
            assert!(mode.is_power_of_two(), "0x{:x}", mode);
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
    fn test_kinds_distinct() {
        assert_ne!(MACVLAN_KIND, MACVTAP_KIND);
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
}
