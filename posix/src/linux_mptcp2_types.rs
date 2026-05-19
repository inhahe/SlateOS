//! `<linux/mptcp.h>` — Additional MPTCP constants.
//!
//! Supplementary MPTCP constants covering subflow flags,
//! PM commands, and address flags.

// ---------------------------------------------------------------------------
// MPTCP PM (Path Manager) commands
// ---------------------------------------------------------------------------

/// Unspec command.
pub const MPTCP_PM_CMD_UNSPEC: u32 = 0;
/// Add address.
pub const MPTCP_PM_CMD_ADD_ADDR: u32 = 1;
/// Delete address.
pub const MPTCP_PM_CMD_DEL_ADDR: u32 = 2;
/// Get address.
pub const MPTCP_PM_CMD_GET_ADDR: u32 = 3;
/// Flush addresses.
pub const MPTCP_PM_CMD_FLUSH_ADDRS: u32 = 4;
/// Set limits.
pub const MPTCP_PM_CMD_SET_LIMITS: u32 = 5;
/// Get limits.
pub const MPTCP_PM_CMD_GET_LIMITS: u32 = 6;
/// Set flags.
pub const MPTCP_PM_CMD_SET_FLAGS: u32 = 7;
/// Announce.
pub const MPTCP_PM_CMD_ANNOUNCE: u32 = 8;
/// Remove subflow.
pub const MPTCP_PM_CMD_REMOVE: u32 = 9;
/// Create subflow.
pub const MPTCP_PM_CMD_SUBFLOW_CREATE: u32 = 10;
/// Destroy subflow.
pub const MPTCP_PM_CMD_SUBFLOW_DESTROY: u32 = 11;

// ---------------------------------------------------------------------------
// MPTCP address flags (MPTCP_PM_ADDR_FLAG_*)
// ---------------------------------------------------------------------------

/// Signal address to peer.
pub const MPTCP_PM_ADDR_FLAG_SIGNAL: u32 = 1 << 0;
/// Create subflow to this address.
pub const MPTCP_PM_ADDR_FLAG_SUBFLOW: u32 = 1 << 1;
/// Backup path.
pub const MPTCP_PM_ADDR_FLAG_BACKUP: u32 = 1 << 2;
/// Fullmesh mode.
pub const MPTCP_PM_ADDR_FLAG_FULLMESH: u32 = 1 << 3;
/// Implicit address (kernel-added).
pub const MPTCP_PM_ADDR_FLAG_IMPLICIT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// MPTCP socket options
// ---------------------------------------------------------------------------

/// MPTCP info socket option.
pub const MPTCP_INFO: u32 = 1;
/// TCP subflow info.
pub const MPTCP_TCPINFO: u32 = 2;
/// Subflow addresses.
pub const MPTCP_SUBFLOW_ADDRS: u32 = 3;
/// Full info.
pub const MPTCP_FULL_INFO: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pm_commands_distinct() {
        let cmds = [
            MPTCP_PM_CMD_UNSPEC, MPTCP_PM_CMD_ADD_ADDR,
            MPTCP_PM_CMD_DEL_ADDR, MPTCP_PM_CMD_GET_ADDR,
            MPTCP_PM_CMD_FLUSH_ADDRS, MPTCP_PM_CMD_SET_LIMITS,
            MPTCP_PM_CMD_GET_LIMITS, MPTCP_PM_CMD_SET_FLAGS,
            MPTCP_PM_CMD_ANNOUNCE, MPTCP_PM_CMD_REMOVE,
            MPTCP_PM_CMD_SUBFLOW_CREATE, MPTCP_PM_CMD_SUBFLOW_DESTROY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_addr_flags_power_of_two() {
        let flags = [
            MPTCP_PM_ADDR_FLAG_SIGNAL, MPTCP_PM_ADDR_FLAG_SUBFLOW,
            MPTCP_PM_ADDR_FLAG_BACKUP, MPTCP_PM_ADDR_FLAG_FULLMESH,
            MPTCP_PM_ADDR_FLAG_IMPLICIT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_addr_flags_no_overlap() {
        let flags = [
            MPTCP_PM_ADDR_FLAG_SIGNAL, MPTCP_PM_ADDR_FLAG_SUBFLOW,
            MPTCP_PM_ADDR_FLAG_BACKUP, MPTCP_PM_ADDR_FLAG_FULLMESH,
            MPTCP_PM_ADDR_FLAG_IMPLICIT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sockopt_distinct() {
        let opts = [MPTCP_INFO, MPTCP_TCPINFO, MPTCP_SUBFLOW_ADDRS, MPTCP_FULL_INFO];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
