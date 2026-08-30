//! `<linux/mptcp.h>` — Multipath TCP (MPTCP) constants.
//!
//! MPTCP (RFC 8684) extends TCP to use multiple paths (e.g., WiFi +
//! cellular) simultaneously for a single connection. The kernel
//! manages subflows transparently; userspace sees a single socket.
//! The netlink interface controls path management, address
//! announcement, and subflow limits. Used by mobile devices for
//! seamless handover and by data centers for bandwidth aggregation.

// ---------------------------------------------------------------------------
// MPTCP socket options (SOL_MPTCP level)
// ---------------------------------------------------------------------------

/// MPTCP socket option level.
pub const SOL_MPTCP: u32 = 284;
/// Get/set MPTCP info (like TCP_INFO for MPTCP).
pub const MPTCP_INFO: u32 = 1;
/// Get/set TCP-level info for a subflow.
pub const MPTCP_TCPINFO: u32 = 2;
/// Get subflow addresses.
pub const MPTCP_SUBFLOW_ADDRS: u32 = 3;
/// Get full MPTCP info (combined).
pub const MPTCP_FULL_INFO: u32 = 4;

// ---------------------------------------------------------------------------
// MPTCP netlink commands (MPTCP_PM_CMD_*)
// ---------------------------------------------------------------------------

/// Get path manager info.
pub const MPTCP_PM_CMD_GET: u32 = 1;
/// Set path manager parameters.
pub const MPTCP_PM_CMD_SET: u32 = 2;
/// Add an address for path manager.
pub const MPTCP_PM_CMD_ADD_ADDR: u32 = 3;
/// Delete an address from path manager.
pub const MPTCP_PM_CMD_DEL_ADDR: u32 = 4;
/// Get address list.
pub const MPTCP_PM_CMD_GET_ADDR: u32 = 5;
/// Flush all addresses.
pub const MPTCP_PM_CMD_FLUSH_ADDRS: u32 = 6;
/// Set connection limits.
pub const MPTCP_PM_CMD_SET_LIMITS: u32 = 7;
/// Get connection limits.
pub const MPTCP_PM_CMD_GET_LIMITS: u32 = 8;
/// Set flags on an address.
pub const MPTCP_PM_CMD_SET_FLAGS: u32 = 9;
/// Announce (ADD_ADDR) event.
pub const MPTCP_PM_CMD_ANNOUNCE: u32 = 10;
/// Remove (REMOVE) event.
pub const MPTCP_PM_CMD_REMOVE: u32 = 11;
/// Create a subflow.
pub const MPTCP_PM_CMD_SUBFLOW_CREATE: u32 = 12;
/// Destroy a subflow.
pub const MPTCP_PM_CMD_SUBFLOW_DESTROY: u32 = 13;

// ---------------------------------------------------------------------------
// MPTCP address flags
// ---------------------------------------------------------------------------

/// Address is a signal address (announce to peer).
pub const MPTCP_PM_ADDR_FLAG_SIGNAL: u32 = 1 << 0;
/// Address is a subflow address (create subflows from it).
pub const MPTCP_PM_ADDR_FLAG_SUBFLOW: u32 = 1 << 1;
/// Address is a backup path.
pub const MPTCP_PM_ADDR_FLAG_BACKUP: u32 = 1 << 2;
/// Address is fullmesh (connect to all peer addresses).
pub const MPTCP_PM_ADDR_FLAG_FULLMESH: u32 = 1 << 3;
/// Address is implicitly created.
pub const MPTCP_PM_ADDR_FLAG_IMPLICIT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// MPTCP event types (netlink multicast)
// ---------------------------------------------------------------------------

/// New MPTCP connection created.
pub const MPTCP_EVENT_CREATED: u32 = 1;
/// MPTCP connection established.
pub const MPTCP_EVENT_ESTABLISHED: u32 = 2;
/// MPTCP connection closed.
pub const MPTCP_EVENT_CLOSED: u32 = 3;
/// New subflow established.
pub const MPTCP_EVENT_SUB_ESTABLISHED: u32 = 10;
/// Subflow closed.
pub const MPTCP_EVENT_SUB_CLOSED: u32 = 11;
/// Subflow priority changed.
pub const MPTCP_EVENT_SUB_PRIORITY: u32 = 13;
/// Peer announced an address.
pub const MPTCP_EVENT_ANNOUNCE: u32 = 6;
/// Peer removed an address.
pub const MPTCP_EVENT_REMOVE: u32 = 7;
/// Listener created.
pub const MPTCP_EVENT_LISTENER_CREATED: u32 = 15;
/// Listener closed.
pub const MPTCP_EVENT_LISTENER_CLOSED: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            MPTCP_INFO,
            MPTCP_TCPINFO,
            MPTCP_SUBFLOW_ADDRS,
            MPTCP_FULL_INFO,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_pm_commands_distinct() {
        let cmds = [
            MPTCP_PM_CMD_GET,
            MPTCP_PM_CMD_SET,
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
    fn test_addr_flags_no_overlap() {
        let flags = [
            MPTCP_PM_ADDR_FLAG_SIGNAL,
            MPTCP_PM_ADDR_FLAG_SUBFLOW,
            MPTCP_PM_ADDR_FLAG_BACKUP,
            MPTCP_PM_ADDR_FLAG_FULLMESH,
            MPTCP_PM_ADDR_FLAG_IMPLICIT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            MPTCP_EVENT_CREATED,
            MPTCP_EVENT_ESTABLISHED,
            MPTCP_EVENT_CLOSED,
            MPTCP_EVENT_ANNOUNCE,
            MPTCP_EVENT_REMOVE,
            MPTCP_EVENT_SUB_ESTABLISHED,
            MPTCP_EVENT_SUB_CLOSED,
            MPTCP_EVENT_SUB_PRIORITY,
            MPTCP_EVENT_LISTENER_CREATED,
            MPTCP_EVENT_LISTENER_CLOSED,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_sol_mptcp() {
        assert_eq!(SOL_MPTCP, 284);
    }
}
