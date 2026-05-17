//! `<linux/socket.h>` — Socket domain, type, and protocol constants.
//!
//! Sockets are the primary IPC mechanism for network communication.
//! The socket() syscall takes a domain (protocol family), type
//! (stream/datagram/raw), and protocol number. These constants define
//! the Linux kernel's socket interface for all address families.

// ---------------------------------------------------------------------------
// Address families (AF_*) / Protocol families (PF_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const AF_UNSPEC: u32 = 0;
/// Unix domain sockets (local IPC).
pub const AF_UNIX: u32 = 1;
/// IPv4 Internet protocols.
pub const AF_INET: u32 = 2;
/// IPv6 Internet protocols.
pub const AF_INET6: u32 = 10;
/// Netlink (kernel/userspace communication).
pub const AF_NETLINK: u32 = 16;
/// Raw packet access (layer 2).
pub const AF_PACKET: u32 = 17;
/// Bluetooth.
pub const AF_BLUETOOTH: u32 = 31;
/// VM sockets (host/guest communication).
pub const AF_VSOCK: u32 = 40;
/// CAN bus.
pub const AF_CAN: u32 = 29;
/// Kernel crypto API.
pub const AF_ALG: u32 = 38;
/// XDP (eXpress Data Path).
pub const AF_XDP: u32 = 44;

// ---------------------------------------------------------------------------
// Socket types (SOCK_*)
// ---------------------------------------------------------------------------

/// Byte-stream (TCP-like).
pub const SOCK_STREAM: u32 = 1;
/// Datagram (UDP-like).
pub const SOCK_DGRAM: u32 = 2;
/// Raw protocol access.
pub const SOCK_RAW: u32 = 3;
/// Reliably-delivered message.
pub const SOCK_RDM: u32 = 4;
/// Sequenced, reliable, connection-based datagrams.
pub const SOCK_SEQPACKET: u32 = 5;
/// Datagram congestion control (DCCP).
pub const SOCK_DCCP: u32 = 6;

// ---------------------------------------------------------------------------
// Socket type flags (OR with SOCK_*)
// ---------------------------------------------------------------------------

/// Set close-on-exec.
pub const SOCK_CLOEXEC: u32 = 0o200_0000;
/// Set non-blocking.
pub const SOCK_NONBLOCK: u32 = 0o000_4000;

// ---------------------------------------------------------------------------
// Socket level for setsockopt/getsockopt
// ---------------------------------------------------------------------------

/// Socket-level options.
pub const SOL_SOCKET: u32 = 1;
/// TCP-level options.
pub const SOL_TCP: u32 = 6;
/// UDP-level options.
pub const SOL_UDP: u32 = 17;
/// IPv6-level options.
pub const SOL_IPV6: u32 = 41;
/// IP-level options.
pub const SOL_IP: u32 = 0;

// ---------------------------------------------------------------------------
// Common socket options (SOL_SOCKET level)
// ---------------------------------------------------------------------------

/// Reuse local address.
pub const SO_REUSEADDR: u32 = 2;
/// Reuse local port.
pub const SO_REUSEPORT: u32 = 15;
/// Keep connections alive.
pub const SO_KEEPALIVE: u32 = 9;
/// Send buffer size.
pub const SO_SNDBUF: u32 = 7;
/// Receive buffer size.
pub const SO_RCVBUF: u32 = 8;
/// Get socket error.
pub const SO_ERROR: u32 = 4;
/// Receive timeout.
pub const SO_RCVTIMEO: u32 = 20;
/// Send timeout.
pub const SO_SNDTIMEO: u32 = 21;
/// Linger on close.
pub const SO_LINGER: u32 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_families_distinct() {
        let families = [
            AF_UNSPEC, AF_UNIX, AF_INET, AF_INET6, AF_NETLINK,
            AF_PACKET, AF_BLUETOOTH, AF_VSOCK, AF_CAN, AF_ALG, AF_XDP,
        ];
        for i in 0..families.len() {
            for j in (i + 1)..families.len() {
                assert_ne!(families[i], families[j]);
            }
        }
    }

    #[test]
    fn test_socket_types_distinct() {
        let types = [
            SOCK_STREAM, SOCK_DGRAM, SOCK_RAW,
            SOCK_RDM, SOCK_SEQPACKET, SOCK_DCCP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sock_flags_no_base_overlap() {
        // CLOEXEC and NONBLOCK should not overlap with base type values
        assert_eq!(SOCK_CLOEXEC & 0xFF, 0);
        assert_eq!(SOCK_NONBLOCK & 0xFF, 0);
    }

    #[test]
    fn test_sol_levels_distinct() {
        let levels = [SOL_SOCKET, SOL_TCP, SOL_UDP, SOL_IPV6, SOL_IP];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            SO_REUSEADDR, SO_REUSEPORT, SO_KEEPALIVE,
            SO_SNDBUF, SO_RCVBUF, SO_ERROR,
            SO_RCVTIMEO, SO_SNDTIMEO, SO_LINGER,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
