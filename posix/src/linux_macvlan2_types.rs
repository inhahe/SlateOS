//! `<linux/if_macvlan.h>` — Additional MACVLAN constants.
//!
//! Supplementary MACVLAN constants covering operating modes,
//! macvtap flags, and macvlan source modes.

// ---------------------------------------------------------------------------
// MACVLAN modes
// ---------------------------------------------------------------------------

/// Private mode (no inter-macvlan communication).
pub const MACVLAN_MODE_PRIVATE: u32 = 1 << 0;
/// VEPA mode (Virtual Ethernet Port Aggregator).
pub const MACVLAN_MODE_VEPA: u32 = 1 << 1;
/// Bridge mode (switch between macvlans).
pub const MACVLAN_MODE_BRIDGE: u32 = 1 << 2;
/// Passthru mode (one macvlan per lower device).
pub const MACVLAN_MODE_PASSTHRU: u32 = 1 << 3;
/// Source mode (filter by source MAC).
pub const MACVLAN_MODE_SOURCE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// MACVLAN flags
// ---------------------------------------------------------------------------

/// No carrier flag.
pub const MACVLAN_FLAG_NOPROMISC: u32 = 1 << 0;
/// Broadcast cutoff.
pub const MACVLAN_FLAG_NODST: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// MACVLAN source entry commands
// ---------------------------------------------------------------------------

/// Add source MAC entry.
pub const MACVLAN_MACADDR_ADD: u32 = 0;
/// Delete source MAC entry.
pub const MACVLAN_MACADDR_DEL: u32 = 1;
/// Flush all source MAC entries.
pub const MACVLAN_MACADDR_FLUSH: u32 = 2;
/// Set (replace all) source MAC entries.
pub const MACVLAN_MACADDR_SET: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_power_of_two() {
        let modes = [
            MACVLAN_MODE_PRIVATE,
            MACVLAN_MODE_VEPA,
            MACVLAN_MODE_BRIDGE,
            MACVLAN_MODE_PASSTHRU,
            MACVLAN_MODE_SOURCE,
        ];
        for m in &modes {
            assert!(m.is_power_of_two(), "0x{:08x} not power of two", m);
        }
    }

    #[test]
    fn test_modes_no_overlap() {
        let modes = [
            MACVLAN_MODE_PRIVATE,
            MACVLAN_MODE_VEPA,
            MACVLAN_MODE_BRIDGE,
            MACVLAN_MODE_PASSTHRU,
            MACVLAN_MODE_SOURCE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_flags_power_of_two() {
        assert!(MACVLAN_FLAG_NOPROMISC.is_power_of_two());
        assert!(MACVLAN_FLAG_NODST.is_power_of_two());
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(MACVLAN_FLAG_NOPROMISC & MACVLAN_FLAG_NODST, 0);
    }

    #[test]
    fn test_macaddr_commands_distinct() {
        let cmds = [
            MACVLAN_MACADDR_ADD,
            MACVLAN_MACADDR_DEL,
            MACVLAN_MACADDR_FLUSH,
            MACVLAN_MACADDR_SET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
