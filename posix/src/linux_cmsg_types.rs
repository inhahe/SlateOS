//! `<sys/socket.h>` — Control message (cmsg) type constants.
//!
//! Control messages are ancillary data sent alongside regular socket
//! data via `sendmsg()`/`recvmsg()`. They carry metadata like file
//! descriptors, credentials, timestamps, and IP options through the
//! `cmsghdr` structures in the `msg_control` buffer.

// ---------------------------------------------------------------------------
// Control message levels
// ---------------------------------------------------------------------------

/// Socket-level control messages.
pub const SOL_SOCKET: u32 = 1;
/// IP-level control messages.
pub const SOL_IP: u32 = 0;
/// IPv6-level control messages.
pub const SOL_IPV6: u32 = 41;
/// TCP-level control messages.
pub const SOL_TCP: u32 = 6;
/// UDP-level control messages.
pub const SOL_UDP: u32 = 17;

// ---------------------------------------------------------------------------
// SCM (socket control message) types for SOL_SOCKET
// ---------------------------------------------------------------------------

/// Pass file descriptors.
pub const SCM_RIGHTS: u32 = 0x01;
/// Pass credentials (pid, uid, gid).
pub const SCM_CREDENTIALS: u32 = 0x02;
/// Security label.
pub const SCM_SECURITY: u32 = 0x03;

// ---------------------------------------------------------------------------
// IP-level cmsg types
// ---------------------------------------------------------------------------

/// Receive TOS (type of service) value.
pub const IP_TOS: u32 = 1;
/// Receive TTL value.
pub const IP_TTL: u32 = 2;
/// Receive packet info (destination addr + ifindex).
pub const IP_PKTINFO: u32 = 8;
/// Receive original destination address.
pub const IP_ORIGDSTADDR: u32 = 20;

// ---------------------------------------------------------------------------
// Timestamp control message types
// ---------------------------------------------------------------------------

/// Software receive timestamp.
pub const SO_TIMESTAMP: u32 = 29;
/// Nanosecond software timestamp.
pub const SO_TIMESTAMPNS: u32 = 35;
/// Hardware/software timestamping.
pub const SO_TIMESTAMPING: u32 = 37;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sol_levels_distinct() {
        let levels = [SOL_SOCKET, SOL_IP, SOL_IPV6, SOL_TCP, SOL_UDP];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
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
    fn test_scm_rights_value() {
        assert_eq!(SCM_RIGHTS, 0x01);
    }

    #[test]
    fn test_sol_socket() {
        assert_eq!(SOL_SOCKET, 1);
    }

    #[test]
    fn test_ip_cmsg_types_distinct() {
        let types = [IP_TOS, IP_TTL, IP_PKTINFO, IP_ORIGDSTADDR];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_timestamp_types_distinct() {
        let types = [SO_TIMESTAMP, SO_TIMESTAMPNS, SO_TIMESTAMPING];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
