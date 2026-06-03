//! `<linux/unix_diag.h>` — Unix domain socket diagnostics constants.
//!
//! The Unix domain socket diagnostics interface allows `ss` and
//! monitoring tools to query AF_UNIX socket state via netlink.
//! It provides socket type, state, peer info, inode numbers,
//! VFS path, memory usage, and pending connection info. More
//! efficient than parsing /proc/net/unix for hosts with many
//! Unix domain sockets (systemd, D-Bus, container runtimes).

// ---------------------------------------------------------------------------
// Unix diag request attributes (UDIAG_*)
// ---------------------------------------------------------------------------

/// Show name (abstract/filesystem path).
pub const UDIAG_SHOW_NAME: u32 = 1 << 0;
/// Show VFS inode info.
pub const UDIAG_SHOW_VFS: u32 = 1 << 1;
/// Show peer socket info.
pub const UDIAG_SHOW_PEER: u32 = 1 << 2;
/// Show pending connections (listen queue).
pub const UDIAG_SHOW_ICONS: u32 = 1 << 3;
/// Show receive queue length.
pub const UDIAG_SHOW_RQLEN: u32 = 1 << 4;
/// Show memory info.
pub const UDIAG_SHOW_MEMINFO: u32 = 1 << 5;
/// Show UID info.
pub const UDIAG_SHOW_UID: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Unix diag response attributes (UNIX_DIAG_*)
// ---------------------------------------------------------------------------

/// Socket name (path or abstract).
pub const UNIX_DIAG_NAME: u32 = 0;
/// VFS device + inode.
pub const UNIX_DIAG_VFS: u32 = 1;
/// Peer socket inode.
pub const UNIX_DIAG_PEER: u32 = 2;
/// Pending connections (inode list).
pub const UNIX_DIAG_ICONS: u32 = 3;
/// Receive queue length.
pub const UNIX_DIAG_RQLEN: u32 = 4;
/// Memory info.
pub const UNIX_DIAG_MEMINFO: u32 = 5;
/// Shutdown state.
pub const UNIX_DIAG_SHUTDOWN: u32 = 6;
/// UID of socket owner.
pub const UNIX_DIAG_UID: u32 = 7;

// ---------------------------------------------------------------------------
// Unix socket states
// ---------------------------------------------------------------------------

/// Socket is free.
pub const UNIX_SS_FREE: u32 = 0;
/// Socket is unconnected.
pub const UNIX_SS_UNCONNECTED: u32 = 1;
/// Socket is connecting.
pub const UNIX_SS_CONNECTING: u32 = 2;
/// Socket is connected.
pub const UNIX_SS_CONNECTED: u32 = 3;
/// Socket is disconnecting.
pub const UNIX_SS_DISCONNECTING: u32 = 4;
/// Socket is listening.
pub const UNIX_SS_LISTEN: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_flags_no_overlap() {
        let flags = [
            UDIAG_SHOW_NAME,
            UDIAG_SHOW_VFS,
            UDIAG_SHOW_PEER,
            UDIAG_SHOW_ICONS,
            UDIAG_SHOW_RQLEN,
            UDIAG_SHOW_MEMINFO,
            UDIAG_SHOW_UID,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_response_attrs_distinct() {
        let attrs = [
            UNIX_DIAG_NAME,
            UNIX_DIAG_VFS,
            UNIX_DIAG_PEER,
            UNIX_DIAG_ICONS,
            UNIX_DIAG_RQLEN,
            UNIX_DIAG_MEMINFO,
            UNIX_DIAG_SHUTDOWN,
            UNIX_DIAG_UID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            UNIX_SS_FREE,
            UNIX_SS_UNCONNECTED,
            UNIX_SS_CONNECTING,
            UNIX_SS_CONNECTED,
            UNIX_SS_DISCONNECTING,
            UNIX_SS_LISTEN,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
