//! `<linux/if_team.h>` — Team device (link aggregation) constants.
//!
//! The team driver is a modern replacement for bonding that provides
//! link aggregation with a modular architecture. Different "runners"
//! implement load-balancing strategies (round-robin, active-backup,
//! LACP 802.3ad, broadcast, random). Configuration is via netlink
//! with the teamd daemon providing the userspace control plane.
//! Used for NIC bonding, failover, and bandwidth aggregation.

// ---------------------------------------------------------------------------
// Team netlink commands (TEAM_CMD_*)
// ---------------------------------------------------------------------------

/// No operation.
pub const TEAM_CMD_NOOP: u32 = 0;
/// Get team options.
pub const TEAM_CMD_OPTIONS_GET: u32 = 1;
/// Set team options.
pub const TEAM_CMD_OPTIONS_SET: u32 = 2;
/// Get port list.
pub const TEAM_CMD_PORT_LIST_GET: u32 = 3;

// ---------------------------------------------------------------------------
// Team option types (TEAM_ATTR_OPTION_TYPE_*)
// ---------------------------------------------------------------------------

/// Option type: unsigned 32-bit integer.
pub const TEAM_ATTR_OPTION_TYPE_U32: u32 = 1;
/// Option type: string.
pub const TEAM_ATTR_OPTION_TYPE_STRING: u32 = 2;
/// Option type: binary data.
pub const TEAM_ATTR_OPTION_TYPE_BINARY: u32 = 3;
/// Option type: boolean.
pub const TEAM_ATTR_OPTION_TYPE_BOOL: u32 = 4;
/// Option type: signed 32-bit integer.
pub const TEAM_ATTR_OPTION_TYPE_S32: u32 = 5;

// ---------------------------------------------------------------------------
// Team top-level attributes (TEAM_ATTR_*)
// ---------------------------------------------------------------------------

/// Team identifier (ifindex).
pub const TEAM_ATTR_TEAM_IFINDEX: u32 = 1;
/// Options list.
pub const TEAM_ATTR_LIST_OPTION: u32 = 2;
/// Port list.
pub const TEAM_ATTR_LIST_PORT: u32 = 3;

// ---------------------------------------------------------------------------
// Team option attributes (TEAM_ATTR_OPTION_*)
// ---------------------------------------------------------------------------

/// Option name.
pub const TEAM_ATTR_OPTION_NAME: u32 = 1;
/// Option changed flag.
pub const TEAM_ATTR_OPTION_CHANGED: u32 = 2;
/// Option type.
pub const TEAM_ATTR_OPTION_TYPE: u32 = 3;
/// Option data.
pub const TEAM_ATTR_OPTION_DATA: u32 = 4;
/// Option is removed.
pub const TEAM_ATTR_OPTION_REMOVED: u32 = 5;
/// Option port interface index (port-specific option).
pub const TEAM_ATTR_OPTION_PORT_IFINDEX: u32 = 6;
/// Option array index.
pub const TEAM_ATTR_OPTION_ARRAY_INDEX: u32 = 7;

// ---------------------------------------------------------------------------
// Team port attributes (TEAM_ATTR_PORT_*)
// ---------------------------------------------------------------------------

/// Port interface index.
pub const TEAM_ATTR_PORT_IFINDEX: u32 = 1;
/// Port changed flag.
pub const TEAM_ATTR_PORT_CHANGED: u32 = 2;
/// Port linkup state.
pub const TEAM_ATTR_PORT_LINKUP: u32 = 3;
/// Port link speed (Mbps).
pub const TEAM_ATTR_PORT_SPEED: u32 = 4;
/// Port link duplex.
pub const TEAM_ATTR_PORT_DUPLEX: u32 = 5;
/// Port removed flag.
pub const TEAM_ATTR_PORT_REMOVED: u32 = 6;

// ---------------------------------------------------------------------------
// Runner types (well-known runner names)
// These are string identifiers, but we define numeric IDs for internal use.
// ---------------------------------------------------------------------------

/// Round-robin runner.
pub const TEAM_RUNNER_ROUNDROBIN: u32 = 0;
/// Active-backup runner (failover).
pub const TEAM_RUNNER_ACTIVEBACKUP: u32 = 1;
/// LACP (Link Aggregation Control Protocol, 802.3ad) runner.
pub const TEAM_RUNNER_LACP: u32 = 2;
/// Broadcast runner (all ports).
pub const TEAM_RUNNER_BROADCAST: u32 = 3;
/// Random runner.
pub const TEAM_RUNNER_RANDOM: u32 = 4;
/// Load-balance runner (hash-based).
pub const TEAM_RUNNER_LOADBALANCE: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            TEAM_CMD_NOOP,
            TEAM_CMD_OPTIONS_GET,
            TEAM_CMD_OPTIONS_SET,
            TEAM_CMD_PORT_LIST_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_option_types_distinct() {
        let types = [
            TEAM_ATTR_OPTION_TYPE_U32,
            TEAM_ATTR_OPTION_TYPE_STRING,
            TEAM_ATTR_OPTION_TYPE_BINARY,
            TEAM_ATTR_OPTION_TYPE_BOOL,
            TEAM_ATTR_OPTION_TYPE_S32,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_top_level_attrs_distinct() {
        let attrs = [
            TEAM_ATTR_TEAM_IFINDEX,
            TEAM_ATTR_LIST_OPTION,
            TEAM_ATTR_LIST_PORT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_option_attrs_distinct() {
        let attrs = [
            TEAM_ATTR_OPTION_NAME,
            TEAM_ATTR_OPTION_CHANGED,
            TEAM_ATTR_OPTION_TYPE,
            TEAM_ATTR_OPTION_DATA,
            TEAM_ATTR_OPTION_REMOVED,
            TEAM_ATTR_OPTION_PORT_IFINDEX,
            TEAM_ATTR_OPTION_ARRAY_INDEX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_port_attrs_distinct() {
        let attrs = [
            TEAM_ATTR_PORT_IFINDEX,
            TEAM_ATTR_PORT_CHANGED,
            TEAM_ATTR_PORT_LINKUP,
            TEAM_ATTR_PORT_SPEED,
            TEAM_ATTR_PORT_DUPLEX,
            TEAM_ATTR_PORT_REMOVED,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_runners_distinct() {
        let runners = [
            TEAM_RUNNER_ROUNDROBIN,
            TEAM_RUNNER_ACTIVEBACKUP,
            TEAM_RUNNER_LACP,
            TEAM_RUNNER_BROADCAST,
            TEAM_RUNNER_RANDOM,
            TEAM_RUNNER_LOADBALANCE,
        ];
        for i in 0..runners.len() {
            for j in (i + 1)..runners.len() {
                assert_ne!(runners[i], runners[j]);
            }
        }
    }
}
