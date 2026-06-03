//! `<linux/ethtool_netlink.h>` — Ethtool netlink interface constants.
//!
//! The ethtool netlink interface (ethnl) replaces the legacy ioctl
//! interface for querying and configuring Ethernet device parameters.
//! It provides richer semantics: notifications on link state changes,
//! batched get/set, cable test, module EEPROM access, and per-queue
//! statistics. Used by the `ethtool` CLI tool (versions 5.8+),
//! NetworkManager, and monitoring systems.

// ---------------------------------------------------------------------------
// Ethtool netlink commands (ETHTOOL_MSG_*)
// ---------------------------------------------------------------------------

/// Get string set (feature names, stat names, etc.).
pub const ETHTOOL_MSG_STRSET_GET: u32 = 1;
/// Get link info (speed, duplex, autoneg).
pub const ETHTOOL_MSG_LINKINFO_GET: u32 = 2;
/// Set link info.
pub const ETHTOOL_MSG_LINKINFO_SET: u32 = 3;
/// Get link modes (supported/advertised speeds).
pub const ETHTOOL_MSG_LINKMODES_GET: u32 = 4;
/// Set link modes.
pub const ETHTOOL_MSG_LINKMODES_SET: u32 = 5;
/// Get link state (up/down).
pub const ETHTOOL_MSG_LINKSTATE_GET: u32 = 6;
/// Get debug level.
pub const ETHTOOL_MSG_DEBUG_GET: u32 = 7;
/// Set debug level.
pub const ETHTOOL_MSG_DEBUG_SET: u32 = 8;
/// Get WOL (Wake-on-LAN) configuration.
pub const ETHTOOL_MSG_WOL_GET: u32 = 9;
/// Set WOL configuration.
pub const ETHTOOL_MSG_WOL_SET: u32 = 10;
/// Get features (offload flags).
pub const ETHTOOL_MSG_FEATURES_GET: u32 = 11;
/// Set features.
pub const ETHTOOL_MSG_FEATURES_SET: u32 = 12;
/// Get private flags.
pub const ETHTOOL_MSG_PRIVFLAGS_GET: u32 = 13;
/// Set private flags.
pub const ETHTOOL_MSG_PRIVFLAGS_SET: u32 = 14;
/// Get ring buffer sizes.
pub const ETHTOOL_MSG_RINGS_GET: u32 = 15;
/// Set ring buffer sizes.
pub const ETHTOOL_MSG_RINGS_SET: u32 = 16;
/// Get channel counts (queues).
pub const ETHTOOL_MSG_CHANNELS_GET: u32 = 17;
/// Set channel counts.
pub const ETHTOOL_MSG_CHANNELS_SET: u32 = 18;
/// Get coalesce parameters.
pub const ETHTOOL_MSG_COALESCE_GET: u32 = 19;
/// Set coalesce parameters.
pub const ETHTOOL_MSG_COALESCE_SET: u32 = 20;
/// Get pause frame configuration.
pub const ETHTOOL_MSG_PAUSE_GET: u32 = 21;
/// Set pause frame configuration.
pub const ETHTOOL_MSG_PAUSE_SET: u32 = 22;
/// Get EEE (Energy Efficient Ethernet) status.
pub const ETHTOOL_MSG_EEE_GET: u32 = 23;
/// Set EEE parameters.
pub const ETHTOOL_MSG_EEE_SET: u32 = 24;
/// Get timestamping capabilities.
pub const ETHTOOL_MSG_TSINFO_GET: u32 = 25;
/// Start cable test.
pub const ETHTOOL_MSG_CABLE_TEST_ACT: u32 = 26;
/// Start cable test TDR.
pub const ETHTOOL_MSG_CABLE_TEST_TDR_ACT: u32 = 27;
/// Get tunnel offload info.
pub const ETHTOOL_MSG_TUNNEL_INFO_GET: u32 = 28;
/// Get FEC (Forward Error Correction) info.
pub const ETHTOOL_MSG_FEC_GET: u32 = 29;
/// Set FEC parameters.
pub const ETHTOOL_MSG_FEC_SET: u32 = 30;
/// Get module EEPROM data.
pub const ETHTOOL_MSG_MODULE_EEPROM_GET: u32 = 31;
/// Get per-queue statistics.
pub const ETHTOOL_MSG_STATS_GET: u32 = 32;
/// Get PHC VCLOCKS info.
pub const ETHTOOL_MSG_PHC_VCLOCKS_GET: u32 = 33;
/// Get module info.
pub const ETHTOOL_MSG_MODULE_GET: u32 = 34;
/// Set module parameters.
pub const ETHTOOL_MSG_MODULE_SET: u32 = 35;
/// Get RSS configuration.
pub const ETHTOOL_MSG_RSS_GET: u32 = 36;

// ---------------------------------------------------------------------------
// Header flags (ETHTOOL_FLAG_*)
// ---------------------------------------------------------------------------

/// Request compact bitsets.
pub const ETHTOOL_FLAG_COMPACT_BITSETS: u32 = 1 << 0;
/// Omit reply (set operations only).
pub const ETHTOOL_FLAG_OMIT_REPLY: u32 = 1 << 1;
/// Include statistics in get reply.
pub const ETHTOOL_FLAG_STATS: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Notification monitors
// ---------------------------------------------------------------------------

/// Monitor group: link info changes.
pub const ETHNL_MCGRP_MONITOR: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            ETHTOOL_MSG_STRSET_GET,
            ETHTOOL_MSG_LINKINFO_GET,
            ETHTOOL_MSG_LINKINFO_SET,
            ETHTOOL_MSG_LINKMODES_GET,
            ETHTOOL_MSG_LINKMODES_SET,
            ETHTOOL_MSG_LINKSTATE_GET,
            ETHTOOL_MSG_DEBUG_GET,
            ETHTOOL_MSG_DEBUG_SET,
            ETHTOOL_MSG_WOL_GET,
            ETHTOOL_MSG_WOL_SET,
            ETHTOOL_MSG_FEATURES_GET,
            ETHTOOL_MSG_FEATURES_SET,
            ETHTOOL_MSG_PRIVFLAGS_GET,
            ETHTOOL_MSG_PRIVFLAGS_SET,
            ETHTOOL_MSG_RINGS_GET,
            ETHTOOL_MSG_RINGS_SET,
            ETHTOOL_MSG_CHANNELS_GET,
            ETHTOOL_MSG_CHANNELS_SET,
            ETHTOOL_MSG_COALESCE_GET,
            ETHTOOL_MSG_COALESCE_SET,
            ETHTOOL_MSG_PAUSE_GET,
            ETHTOOL_MSG_PAUSE_SET,
            ETHTOOL_MSG_EEE_GET,
            ETHTOOL_MSG_EEE_SET,
            ETHTOOL_MSG_TSINFO_GET,
            ETHTOOL_MSG_CABLE_TEST_ACT,
            ETHTOOL_MSG_CABLE_TEST_TDR_ACT,
            ETHTOOL_MSG_TUNNEL_INFO_GET,
            ETHTOOL_MSG_FEC_GET,
            ETHTOOL_MSG_FEC_SET,
            ETHTOOL_MSG_MODULE_EEPROM_GET,
            ETHTOOL_MSG_STATS_GET,
            ETHTOOL_MSG_PHC_VCLOCKS_GET,
            ETHTOOL_MSG_MODULE_GET,
            ETHTOOL_MSG_MODULE_SET,
            ETHTOOL_MSG_RSS_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            ETHTOOL_FLAG_COMPACT_BITSETS,
            ETHTOOL_FLAG_OMIT_REPLY,
            ETHTOOL_FLAG_STATS,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_get_set_pairs() {
        // Get and set commands for the same feature should be consecutive
        assert_eq!(ETHTOOL_MSG_LINKINFO_SET, ETHTOOL_MSG_LINKINFO_GET + 1);
        assert_eq!(ETHTOOL_MSG_LINKMODES_SET, ETHTOOL_MSG_LINKMODES_GET + 1);
        assert_eq!(ETHTOOL_MSG_WOL_SET, ETHTOOL_MSG_WOL_GET + 1);
        assert_eq!(ETHTOOL_MSG_FEATURES_SET, ETHTOOL_MSG_FEATURES_GET + 1);
        assert_eq!(ETHTOOL_MSG_RINGS_SET, ETHTOOL_MSG_RINGS_GET + 1);
        assert_eq!(ETHTOOL_MSG_CHANNELS_SET, ETHTOOL_MSG_CHANNELS_GET + 1);
        assert_eq!(ETHTOOL_MSG_COALESCE_SET, ETHTOOL_MSG_COALESCE_GET + 1);
    }

    #[test]
    fn test_commands_sequential_start() {
        assert_eq!(ETHTOOL_MSG_STRSET_GET, 1);
    }
}
