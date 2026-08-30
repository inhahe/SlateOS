//! `<linux/inet_diag.h>` — TCP/inet socket diagnostics constants.
//!
//! The inet diag (sock_diag) netlink interface is used by `ss`,
//! `netstat`, and monitoring tools to query TCP/UDP/SCTP socket
//! state without reading /proc/net/tcp. It supports filtering by
//! state, address, port, and provides detailed per-socket info:
//! TCP congestion state, timer info, memory usage, TCP_INFO struct,
//! and more. Far more efficient than parsing procfs for systems
//! with thousands of connections.

// ---------------------------------------------------------------------------
// Inet diag request types
// ---------------------------------------------------------------------------

/// Get socket info (SOCK_DIAG_BY_FAMILY).
pub const SOCK_DIAG_BY_FAMILY: u32 = 20;
/// Destroy a socket.
pub const SOCK_DESTROY: u32 = 21;

// ---------------------------------------------------------------------------
// Inet diag extension bits (INET_DIAG_*)
// ---------------------------------------------------------------------------

/// No extension info requested.
pub const INET_DIAG_NONE: u32 = 0;
/// Request memory info.
pub const INET_DIAG_MEMINFO: u32 = 1;
/// Request TCP info (tcp_info struct).
pub const INET_DIAG_INFO: u32 = 2;
/// Request VegasInfo (TCP Vegas congestion).
pub const INET_DIAG_VEGASINFO: u32 = 3;
/// Request congestion control algorithm name.
pub const INET_DIAG_CONG: u32 = 4;
/// Request TOS value.
pub const INET_DIAG_TOS: u32 = 5;
/// Request traffic class.
pub const INET_DIAG_TCLASS: u32 = 6;
/// Request socket memory info (SK_MEMINFO).
pub const INET_DIAG_SKMEMINFO: u32 = 7;
/// Request shutdown state.
pub const INET_DIAG_SHUTDOWN: u32 = 8;
/// Request DCTCP info.
pub const INET_DIAG_DCTCPINFO: u32 = 9;
/// Request protocol-specific info.
pub const INET_DIAG_PROTOCOL: u32 = 10;
/// Request socket mark (fwmark).
pub const INET_DIAG_SKV6ONLY: u32 = 11;
/// Request local addresses (for SCTP).
pub const INET_DIAG_LOCALS: u32 = 12;
/// Request peer addresses (for SCTP).
pub const INET_DIAG_PEERS: u32 = 13;
/// Request padding.
pub const INET_DIAG_PAD: u32 = 14;
/// Request socket mark.
pub const INET_DIAG_MARK: u32 = 15;
/// Request BBR info.
pub const INET_DIAG_BBRINFO: u32 = 16;
/// Request class ID.
pub const INET_DIAG_CLASS_ID: u32 = 17;
/// Request MD5 signature info.
pub const INET_DIAG_MD5SIG: u32 = 18;
/// Request ULP (upper layer protocol) info.
pub const INET_DIAG_ULP_INFO: u32 = 19;
/// Request socket cookie.
pub const INET_DIAG_SK_BPF_STORAGES: u32 = 20;
/// Request cgroup ID.
pub const INET_DIAG_CGROUP_ID: u32 = 21;
/// Request socket options.
pub const INET_DIAG_SOCKOPT: u32 = 22;

// ---------------------------------------------------------------------------
// TCP states (for state filter bitmask)
// ---------------------------------------------------------------------------

/// TCP ESTABLISHED state.
pub const TCP_ESTABLISHED: u32 = 1;
/// TCP SYN_SENT state.
pub const TCP_SYN_SENT: u32 = 2;
/// TCP SYN_RECV state.
pub const TCP_SYN_RECV: u32 = 3;
/// TCP FIN_WAIT1 state.
pub const TCP_FIN_WAIT1: u32 = 4;
/// TCP FIN_WAIT2 state.
pub const TCP_FIN_WAIT2: u32 = 5;
/// TCP TIME_WAIT state.
pub const TCP_TIME_WAIT: u32 = 6;
/// TCP CLOSE state.
pub const TCP_CLOSE: u32 = 7;
/// TCP CLOSE_WAIT state.
pub const TCP_CLOSE_WAIT: u32 = 8;
/// TCP LAST_ACK state.
pub const TCP_LAST_ACK: u32 = 9;
/// TCP LISTEN state.
pub const TCP_LISTEN: u32 = 10;
/// TCP CLOSING state.
pub const TCP_CLOSING: u32 = 11;
/// TCP NEW_SYN_RECV state (SYN cookies).
pub const TCP_NEW_SYN_RECV: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diag_request_types_distinct() {
        assert_ne!(SOCK_DIAG_BY_FAMILY, SOCK_DESTROY);
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
            INET_DIAG_DCTCPINFO,
            INET_DIAG_PROTOCOL,
            INET_DIAG_SKV6ONLY,
            INET_DIAG_LOCALS,
            INET_DIAG_PEERS,
            INET_DIAG_PAD,
            INET_DIAG_MARK,
            INET_DIAG_BBRINFO,
            INET_DIAG_CLASS_ID,
            INET_DIAG_MD5SIG,
            INET_DIAG_ULP_INFO,
            INET_DIAG_SK_BPF_STORAGES,
            INET_DIAG_CGROUP_ID,
            INET_DIAG_SOCKOPT,
        ];
        for i in 0..exts.len() {
            for j in (i + 1)..exts.len() {
                assert_ne!(exts[i], exts[j]);
            }
        }
    }

    #[test]
    fn test_tcp_states_distinct() {
        let states = [
            TCP_ESTABLISHED,
            TCP_SYN_SENT,
            TCP_SYN_RECV,
            TCP_FIN_WAIT1,
            TCP_FIN_WAIT2,
            TCP_TIME_WAIT,
            TCP_CLOSE,
            TCP_CLOSE_WAIT,
            TCP_LAST_ACK,
            TCP_LISTEN,
            TCP_CLOSING,
            TCP_NEW_SYN_RECV,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_tcp_states_sequential() {
        assert_eq!(TCP_ESTABLISHED, 1);
        assert_eq!(TCP_NEW_SYN_RECV, 12);
    }
}
