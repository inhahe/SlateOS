//! `<linux/un.h>` — Unix domain socket constants.
//!
//! Unix domain sockets provide local IPC with socket semantics.
//! They support both stream (SOCK_STREAM, like TCP) and datagram
//! (SOCK_DGRAM, like UDP) modes, plus the unique SOCK_SEQPACKET
//! (reliable datagrams). Unlike network sockets, Unix sockets can
//! pass file descriptors (SCM_RIGHTS) and credentials (SCM_CREDENTIALS)
//! between processes. They're identified by filesystem paths or
//! abstract names (Linux extension: names starting with \0).

// ---------------------------------------------------------------------------
// Unix socket address limits
// ---------------------------------------------------------------------------

/// Maximum path length for a Unix socket address.
pub const UNIX_PATH_MAX: u32 = 108;
/// Size of struct sockaddr_un (address family + path).
pub const UNIX_ADDR_SIZE: u32 = 110;

// ---------------------------------------------------------------------------
// Unix socket types
// ---------------------------------------------------------------------------

/// Stream socket (reliable, ordered, connection-oriented).
pub const UNIX_SOCK_STREAM: u32 = 1;
/// Datagram socket (unreliable, unordered, connectionless).
pub const UNIX_SOCK_DGRAM: u32 = 2;
/// Sequential packet socket (reliable datagrams, connection-oriented).
pub const UNIX_SOCK_SEQPACKET: u32 = 5;

// ---------------------------------------------------------------------------
// Ancillary message types (cmsg)
// ---------------------------------------------------------------------------

/// Pass file descriptors.
pub const SCM_RIGHTS: u32 = 1;
/// Pass sender credentials (pid, uid, gid).
pub const SCM_CREDENTIALS: u32 = 2;
/// Pass security context label.
pub const SCM_SECURITY: u32 = 3;
/// Pass Unix socket peer credentials (pidfd).
pub const SCM_PIDFD: u32 = 4;

// ---------------------------------------------------------------------------
// Unix socket states
// ---------------------------------------------------------------------------

/// Socket is unconnected.
pub const UNIX_STATE_UNCONNECTED: u32 = 0;
/// Socket is connecting (non-blocking connect in progress).
pub const UNIX_STATE_CONNECTING: u32 = 1;
/// Socket is connected.
pub const UNIX_STATE_CONNECTED: u32 = 2;
/// Socket is disconnecting.
pub const UNIX_STATE_DISCONNECTING: u32 = 3;
/// Socket is listening.
pub const UNIX_STATE_LISTENING: u32 = 4;

// ---------------------------------------------------------------------------
// Unix socket flags
// ---------------------------------------------------------------------------

/// Socket is using abstract namespace (name starts with \0).
pub const UNIX_FLAG_ABSTRACT: u32 = 0x01;
/// Socket is bound to a path.
pub const UNIX_FLAG_BOUND: u32 = 0x02;
/// Socket passes credentials automatically.
pub const UNIX_FLAG_PASSCRED: u32 = 0x04;
/// Socket passes security labels.
pub const UNIX_FLAG_PASSSEC: u32 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_max() {
        assert_eq!(UNIX_PATH_MAX, 108);
        assert!(UNIX_ADDR_SIZE > UNIX_PATH_MAX);
    }

    #[test]
    fn test_sock_types_distinct() {
        let types = [UNIX_SOCK_STREAM, UNIX_SOCK_DGRAM, UNIX_SOCK_SEQPACKET];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_scm_types_distinct() {
        let types = [SCM_RIGHTS, SCM_CREDENTIALS, SCM_SECURITY, SCM_PIDFD];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            UNIX_STATE_UNCONNECTED, UNIX_STATE_CONNECTING,
            UNIX_STATE_CONNECTED, UNIX_STATE_DISCONNECTING,
            UNIX_STATE_LISTENING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            UNIX_FLAG_ABSTRACT, UNIX_FLAG_BOUND,
            UNIX_FLAG_PASSCRED, UNIX_FLAG_PASSSEC,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
