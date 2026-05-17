//! `<linux/un.h>` — AF_UNIX (local) socket constants.
//!
//! Unix domain sockets provide IPC between processes on the same host.
//! They support stream (SOCK_STREAM), datagram (SOCK_DGRAM), and
//! sequenced-packet (SOCK_SEQPACKET) modes. They can transfer file
//! descriptors and credentials between processes via ancillary data
//! (SCM_RIGHTS, SCM_CREDENTIALS).

// ---------------------------------------------------------------------------
// Unix socket path limits
// ---------------------------------------------------------------------------

/// Maximum path length in sockaddr_un (including null terminator).
pub const UNIX_PATH_MAX: u32 = 108;

// ---------------------------------------------------------------------------
// Unix socket options (SOL_SOCKET level)
// ---------------------------------------------------------------------------

/// Pass credentials with messages.
pub const SO_PASSCRED: u32 = 16;
/// Peek at credentials.
pub const SO_PEERCRED: u32 = 17;
/// Pass security label.
pub const SO_PASSSEC: u32 = 34;
/// Peek at security label.
pub const SO_PEERSEC: u32 = 31;

// ---------------------------------------------------------------------------
// Ancillary message types (cmsg_type for SOL_SOCKET)
// ---------------------------------------------------------------------------

/// Transfer file descriptors.
pub const SCM_RIGHTS: u32 = 0x01;
/// Transfer credentials (pid, uid, gid).
pub const SCM_CREDENTIALS: u32 = 0x02;
/// Transfer security label.
pub const SCM_SECURITY: u32 = 0x03;

// ---------------------------------------------------------------------------
// Abstract namespace prefix
// ---------------------------------------------------------------------------

/// Abstract socket name starts with null byte (path[0] == 0).
pub const UNIX_ABSTRACT_PREFIX: u8 = 0;

// ---------------------------------------------------------------------------
// Unix socket flags (send/recv)
// ---------------------------------------------------------------------------

/// Out-of-band data.
pub const MSG_OOB: u32 = 0x01;
/// Peek at incoming data.
pub const MSG_PEEK: u32 = 0x02;
/// Don't generate SIGPIPE.
pub const MSG_NOSIGNAL: u32 = 0x4000;
/// Wait for full request.
pub const MSG_WAITALL: u32 = 0x0100;
/// Non-blocking (per-call).
pub const MSG_DONTWAIT: u32 = 0x0040;
/// Send ancillary data (cmsg).
pub const MSG_CMSG_CLOEXEC: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_max() {
        assert_eq!(UNIX_PATH_MAX, 108);
    }

    #[test]
    fn test_scm_types_distinct() {
        let types = [SCM_RIGHTS, SCM_CREDENTIALS, SCM_SECURITY];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_so_options_distinct() {
        let opts = [SO_PASSCRED, SO_PEERCRED, SO_PASSSEC, SO_PEERSEC];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_msg_flags_distinct() {
        let flags = [MSG_OOB, MSG_PEEK, MSG_NOSIGNAL, MSG_WAITALL, MSG_DONTWAIT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_abstract_prefix() {
        assert_eq!(UNIX_ABSTRACT_PREFIX, 0);
    }
}
