//! `<linux/sock_diag.h>` — Socket diagnostics (ss/netstat) constants.
//!
//! The sock_diag netlink interface provides detailed information
//! about kernel sockets. Used by `ss` and `netstat` to query
//! socket state, buffer usage, and connection details without
//! parsing /proc/net/*.

// ---------------------------------------------------------------------------
// Netlink family
// ---------------------------------------------------------------------------

/// Socket diagnostics netlink protocol.
pub const NETLINK_SOCK_DIAG: u32 = 4;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Request socket info (generic).
pub const SOCK_DIAG_BY_FAMILY: u32 = 20;
/// Destroy a socket.
pub const SOCK_DESTROY: u32 = 21;

// ---------------------------------------------------------------------------
// Address families for sock_diag
// ---------------------------------------------------------------------------

/// Unix domain sockets.
pub const AF_UNIX_DIAG: u8 = 1;
/// IPv4 sockets.
pub const AF_INET_DIAG: u8 = 2;
/// IPv6 sockets.
pub const AF_INET6_DIAG: u8 = 10;
/// Netlink sockets.
pub const AF_NETLINK_DIAG: u8 = 16;
/// Packet sockets.
pub const AF_PACKET_DIAG: u8 = 17;

// ---------------------------------------------------------------------------
// INET diagnostic extensions (what info to return)
// ---------------------------------------------------------------------------

/// No extension.
pub const INET_DIAG_NONE: u8 = 0;
/// Memory info.
pub const INET_DIAG_MEMINFO: u8 = 1;
/// TCP info struct.
pub const INET_DIAG_INFO: u8 = 2;
/// TCP Vegas info.
pub const INET_DIAG_VEGASINFO: u8 = 3;
/// Congestion control algorithm name.
pub const INET_DIAG_CONG: u8 = 4;
/// TOS value.
pub const INET_DIAG_TOS: u8 = 5;
/// Traffic class (IPv6).
pub const INET_DIAG_TCLASS: u8 = 6;
/// Socket memory usage.
pub const INET_DIAG_SKMEMINFO: u8 = 7;
/// Shutdown state.
pub const INET_DIAG_SHUTDOWN: u8 = 8;
/// Class ID (cgroup).
pub const INET_DIAG_CLASS_ID: u8 = 15;

// ---------------------------------------------------------------------------
// Socket state filter bits
// ---------------------------------------------------------------------------

/// TCP_ESTABLISHED.
pub const TCPF_ESTABLISHED: u32 = 1 << 1;
/// TCP_SYN_SENT.
pub const TCPF_SYN_SENT: u32 = 1 << 2;
/// TCP_SYN_RECV.
pub const TCPF_SYN_RECV: u32 = 1 << 3;
/// TCP_FIN_WAIT1.
pub const TCPF_FIN_WAIT1: u32 = 1 << 4;
/// TCP_FIN_WAIT2.
pub const TCPF_FIN_WAIT2: u32 = 1 << 5;
/// TCP_TIME_WAIT.
pub const TCPF_TIME_WAIT: u32 = 1 << 6;
/// TCP_CLOSE.
pub const TCPF_CLOSE: u32 = 1 << 7;
/// TCP_CLOSE_WAIT.
pub const TCPF_CLOSE_WAIT: u32 = 1 << 8;
/// TCP_LAST_ACK.
pub const TCPF_LAST_ACK: u32 = 1 << 9;
/// TCP_LISTEN.
pub const TCPF_LISTEN: u32 = 1 << 10;
/// TCP_CLOSING.
pub const TCPF_CLOSING: u32 = 1 << 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_types_distinct() {
        assert_ne!(SOCK_DIAG_BY_FAMILY, SOCK_DESTROY);
    }

    #[test]
    fn test_af_diag_distinct() {
        let afs = [
            AF_UNIX_DIAG,
            AF_INET_DIAG,
            AF_INET6_DIAG,
            AF_NETLINK_DIAG,
            AF_PACKET_DIAG,
        ];
        for i in 0..afs.len() {
            for j in (i + 1)..afs.len() {
                assert_ne!(afs[i], afs[j]);
            }
        }
    }

    #[test]
    fn test_extensions_distinct() {
        let exts = [
            INET_DIAG_NONE,
            INET_DIAG_MEMINFO,
            INET_DIAG_INFO,
            INET_DIAG_VEGASINFO,
            INET_DIAG_CONG,
            INET_DIAG_TOS,
            INET_DIAG_TCLASS,
            INET_DIAG_SKMEMINFO,
            INET_DIAG_SHUTDOWN,
            INET_DIAG_CLASS_ID,
        ];
        for i in 0..exts.len() {
            for j in (i + 1)..exts.len() {
                assert_ne!(exts[i], exts[j]);
            }
        }
    }

    #[test]
    fn test_state_filters_powers_of_two() {
        let states = [
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
        for state in &states {
            assert!(state.is_power_of_two(), "0x{:x}", state);
        }
    }

    #[test]
    fn test_state_filters_no_overlap() {
        let states = [
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
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_eq!(states[i] & states[j], 0);
            }
        }
    }
}
