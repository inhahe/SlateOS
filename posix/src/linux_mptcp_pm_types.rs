//! `<linux/mptcp_pm.h>` — MPTCP path-manager netlink constants.
//!
//! Generic-netlink family used by `mptcpd` and `ip mptcp` to manage
//! MPTCP (Multipath TCP) subflows and address advertisements from
//! userspace.

// ---------------------------------------------------------------------------
// Generic-netlink family identifiers
// ---------------------------------------------------------------------------

/// Generic-netlink family name string.
pub const MPTCP_PM_NAME: &str = "mptcp_pm";
/// Family-version reported in the genl header.
pub const MPTCP_PM_VER: u32 = 1;
/// Multicast-group name for endpoint-event notifications.
pub const MPTCP_PM_EV_GRP_NAME: &str = "mptcp_pm_events";

// ---------------------------------------------------------------------------
// Generic-netlink commands (MPTCP_PM_CMD_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const MPTCP_PM_CMD_UNSPEC: u32 = 0;
/// Add an advertised local address.
pub const MPTCP_PM_CMD_ADD_ADDR: u32 = 1;
/// Remove an advertised local address.
pub const MPTCP_PM_CMD_DEL_ADDR: u32 = 2;
/// Retrieve the configured address list.
pub const MPTCP_PM_CMD_GET_ADDR: u32 = 3;
/// Flush all addresses.
pub const MPTCP_PM_CMD_FLUSH_ADDRS: u32 = 4;
/// Set per-tuple PM limits.
pub const MPTCP_PM_CMD_SET_LIMITS: u32 = 5;
/// Retrieve PM limits.
pub const MPTCP_PM_CMD_GET_LIMITS: u32 = 6;
/// Set per-flow PM behaviour.
pub const MPTCP_PM_CMD_SET_FLAGS: u32 = 7;
/// Announce a new address to the peer.
pub const MPTCP_PM_CMD_ANNOUNCE: u32 = 8;
/// Remove a peer-announced address.
pub const MPTCP_PM_CMD_REMOVE: u32 = 9;
/// Open a new subflow.
pub const MPTCP_PM_CMD_SUBFLOW_CREATE: u32 = 10;
/// Close an existing subflow.
pub const MPTCP_PM_CMD_SUBFLOW_DESTROY: u32 = 11;

// ---------------------------------------------------------------------------
// Endpoint flag bits (mptcp_pm_addr_attr.flags)
// ---------------------------------------------------------------------------

/// Subflow address (initiates outgoing subflows).
pub const MPTCP_PM_ADDR_FLAG_SUBFLOW: u32 = 1 << 0;
/// Signal this address to the peer.
pub const MPTCP_PM_ADDR_FLAG_SIGNAL: u32 = 1 << 1;
/// Address is a backup path.
pub const MPTCP_PM_ADDR_FLAG_BACKUP: u32 = 1 << 2;
/// Use as fullmesh source for every peer address.
pub const MPTCP_PM_ADDR_FLAG_FULLMESH: u32 = 1 << 3;
/// Implicit address (auto-added by kernel).
pub const MPTCP_PM_ADDR_FLAG_IMPLICIT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Event types (mptcp_pm_event.type)
// ---------------------------------------------------------------------------

/// Connection created.
pub const MPTCP_EVENT_CREATED: u32 = 1;
/// Connection established.
pub const MPTCP_EVENT_ESTABLISHED: u32 = 2;
/// Connection closed.
pub const MPTCP_EVENT_CLOSED: u32 = 3;
/// Peer announced a new address (ADD_ADDR).
pub const MPTCP_EVENT_ANNOUNCED: u32 = 6;
/// Peer removed an address (REMOVE_ADDR).
pub const MPTCP_EVENT_REMOVED: u32 = 7;
/// New subflow established.
pub const MPTCP_EVENT_SUB_ESTABLISHED: u32 = 10;
/// Subflow closed.
pub const MPTCP_EVENT_SUB_CLOSED: u32 = 11;
/// Subflow set/unset to backup.
pub const MPTCP_EVENT_SUB_PRIORITY: u32 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_names_nonempty_and_distinct() {
        assert_eq!(MPTCP_PM_NAME, "mptcp_pm");
        assert_eq!(MPTCP_PM_EV_GRP_NAME, "mptcp_pm_events");
        assert_ne!(MPTCP_PM_NAME, MPTCP_PM_EV_GRP_NAME);
        assert!(MPTCP_PM_VER >= 1);
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            MPTCP_PM_CMD_UNSPEC,
            MPTCP_PM_CMD_ADD_ADDR,
            MPTCP_PM_CMD_DEL_ADDR,
            MPTCP_PM_CMD_GET_ADDR,
            MPTCP_PM_CMD_FLUSH_ADDRS,
            MPTCP_PM_CMD_SET_LIMITS,
            MPTCP_PM_CMD_GET_LIMITS,
            MPTCP_PM_CMD_SET_FLAGS,
            MPTCP_PM_CMD_ANNOUNCE,
            MPTCP_PM_CMD_REMOVE,
            MPTCP_PM_CMD_SUBFLOW_CREATE,
            MPTCP_PM_CMD_SUBFLOW_DESTROY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_addr_flags_distinct_powers_of_two() {
        let flags = [
            MPTCP_PM_ADDR_FLAG_SUBFLOW,
            MPTCP_PM_ADDR_FLAG_SIGNAL,
            MPTCP_PM_ADDR_FLAG_BACKUP,
            MPTCP_PM_ADDR_FLAG_FULLMESH,
            MPTCP_PM_ADDR_FLAG_IMPLICIT,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_event_types_distinct() {
        let events = [
            MPTCP_EVENT_CREATED,
            MPTCP_EVENT_ESTABLISHED,
            MPTCP_EVENT_CLOSED,
            MPTCP_EVENT_ANNOUNCED,
            MPTCP_EVENT_REMOVED,
            MPTCP_EVENT_SUB_ESTABLISHED,
            MPTCP_EVENT_SUB_CLOSED,
            MPTCP_EVENT_SUB_PRIORITY,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
