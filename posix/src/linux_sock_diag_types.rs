//! `<linux/sock_diag.h>` — Socket diagnostics (ss/netstat) constants.
//!
//! The sock_diag netlink interface provides detailed socket status
//! information to userspace tools like `ss` and `netstat`. It can
//! query TCP, UDP, Unix, and other socket types for connection
//! state, buffer usage, and associated metadata.

// ---------------------------------------------------------------------------
// Socket diagnostic request families
// ---------------------------------------------------------------------------

/// Request for TCP sockets (AF_INET/AF_INET6 SOCK_STREAM).
pub const SOCK_DIAG_BY_FAMILY: u32 = 20;
/// Socket destroy request.
pub const SOCK_DESTROY: u32 = 21;

// ---------------------------------------------------------------------------
// Diagnostic attribute types (inet_diag_attr)
// ---------------------------------------------------------------------------

/// No attribute.
pub const INET_DIAG_NONE: u32 = 0;
/// Memory info (send/recv buffers).
pub const INET_DIAG_MEMINFO: u32 = 1;
/// TCP info structure.
pub const INET_DIAG_INFO: u32 = 2;
/// VFS info (inode, device).
pub const INET_DIAG_VEGASINFO: u32 = 3;
/// Congestion control algorithm name.
pub const INET_DIAG_CONG: u32 = 4;
/// TOS value.
pub const INET_DIAG_TOS: u32 = 5;
/// Traffic class.
pub const INET_DIAG_TCLASS: u32 = 6;
/// Socket memory usage (skmem).
pub const INET_DIAG_SKMEMINFO: u32 = 7;
/// Shutdown state.
pub const INET_DIAG_SHUTDOWN: u32 = 8;
/// Protocol (IPPROTO_*).
pub const INET_DIAG_PROTOCOL: u32 = 9;
/// Socket mark.
pub const INET_DIAG_MARK: u32 = 15;
/// BPF filter info.
pub const INET_DIAG_BBRINFO: u32 = 16;
/// Class ID.
pub const INET_DIAG_CLASS_ID: u32 = 17;
/// Socket cookie.
pub const INET_DIAG_SOCKOPT: u32 = 21;

// ---------------------------------------------------------------------------
// Diagnostic filter flags
// ---------------------------------------------------------------------------

/// Show TCP_ESTABLISHED sockets.
pub const TCPF_ESTABLISHED: u32 = 1 << 1;
/// Show TCP_SYN_SENT sockets.
pub const TCPF_SYN_SENT: u32 = 1 << 2;
/// Show TCP_SYN_RECV sockets.
pub const TCPF_SYN_RECV: u32 = 1 << 3;
/// Show TCP_FIN_WAIT1 sockets.
pub const TCPF_FIN_WAIT1: u32 = 1 << 4;
/// Show TCP_FIN_WAIT2 sockets.
pub const TCPF_FIN_WAIT2: u32 = 1 << 5;
/// Show TCP_TIME_WAIT sockets.
pub const TCPF_TIME_WAIT: u32 = 1 << 6;
/// Show TCP_CLOSE sockets.
pub const TCPF_CLOSE: u32 = 1 << 7;
/// Show TCP_CLOSE_WAIT sockets.
pub const TCPF_CLOSE_WAIT: u32 = 1 << 8;
/// Show TCP_LAST_ACK sockets.
pub const TCPF_LAST_ACK: u32 = 1 << 9;
/// Show TCP_LISTEN sockets.
pub const TCPF_LISTEN: u32 = 1 << 10;
/// Show TCP_CLOSING sockets.
pub const TCPF_CLOSING: u32 = 1 << 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diag_attrs_distinct() {
        let attrs = [
            INET_DIAG_NONE,
            INET_DIAG_MEMINFO,
            INET_DIAG_INFO,
            INET_DIAG_VEGASINFO,
            INET_DIAG_CONG,
            INET_DIAG_TOS,
            INET_DIAG_TCLASS,
            INET_DIAG_SKMEMINFO,
            INET_DIAG_SHUTDOWN,
            INET_DIAG_PROTOCOL,
            INET_DIAG_MARK,
            INET_DIAG_BBRINFO,
            INET_DIAG_CLASS_ID,
            INET_DIAG_SOCKOPT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_tcpf_state_flags_no_overlap() {
        let flags = [
            TCPF_ESTABLISHED,
            TCPF_SYN_SENT,
            TCPF_SYN_RECV,
            TCPF_FIN_WAIT1,
            TCPF_FIN_WAIT2,
            TCPF_TIME_WAIT,
            TCPF_CLOSE,
            TCPF_CLOSE_WAIT,
            TCPF_LAST_ACK,
            TCPF_LISTEN,
            TCPF_CLOSING,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_diag_commands() {
        assert_ne!(SOCK_DIAG_BY_FAMILY, SOCK_DESTROY);
    }
}
