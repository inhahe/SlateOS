//! `<sys/socket.h>` — Control message (cmsg) layout helpers.
//!
//! Ancillary data passes alongside `sendmsg`/`recvmsg` to carry file
//! descriptors (SCM_RIGHTS), credentials, IPv4 packet info, etc.
//! Each cmsg is a `cmsghdr` header followed by aligned data.

// ---------------------------------------------------------------------------
// struct cmsghdr field offsets (64-bit Linux)
// ---------------------------------------------------------------------------

/// `socklen_t cmsg_len` — total length including header.
pub const CMSGHDR_OFF_LEN: usize = 0;
/// `int cmsg_level` — originating protocol level.
pub const CMSGHDR_OFF_LEVEL: usize = 8;
/// `int cmsg_type` — protocol-specific type.
pub const CMSGHDR_OFF_TYPE: usize = 12;
/// Header size before data (cmsg_len + level + type + pad).
pub const CMSGHDR_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Alignment for cmsg data
// ---------------------------------------------------------------------------

/// CMSG payload is aligned to sizeof(size_t) = 8 on 64-bit.
pub const CMSG_ALIGN_BYTES: usize = 8;

// ---------------------------------------------------------------------------
// SCM_* (socket-level cmsg types under SOL_SOCKET)
// ---------------------------------------------------------------------------

/// Pass file descriptors.
pub const SCM_RIGHTS: u32 = 0x01;
/// Pass UID/GID/PID credentials.
pub const SCM_CREDENTIALS: u32 = 0x02;
/// Pass arbitrary security tag (LSM).
pub const SCM_SECURITY: u32 = 0x03;
/// Pidfd of the sender.
pub const SCM_PIDFD: u32 = 0x04;
/// Timestamp (tv_sec, tv_usec).
pub const SCM_TIMESTAMP: u32 = 0x1D;
/// Timestamp with nanosecond resolution.
pub const SCM_TIMESTAMPNS: u32 = 0x23;

// ---------------------------------------------------------------------------
// IP-level cmsg types (level = SOL_IP / IPPROTO_IP)
// ---------------------------------------------------------------------------

pub const IP_PKTINFO: u32 = 8;
pub const IP_TTL: u32 = 2;
pub const IP_TOS: u32 = 1;
pub const IP_RECVERR: u32 = 11;

// ---------------------------------------------------------------------------
// IPv6-level cmsg types (level = SOL_IPV6 / IPPROTO_IPV6)
// ---------------------------------------------------------------------------

pub const IPV6_PKTINFO: u32 = 50;
pub const IPV6_HOPLIMIT: u32 = 52;
pub const IPV6_TCLASS: u32 = 67;
pub const IPV6_RECVERR: u32 = 25;

// ---------------------------------------------------------------------------
// Maximum SCM_RIGHTS fds in a single cmsg
// ---------------------------------------------------------------------------

/// SCM_MAX_FD — kernel cap on file descriptors per SCM_RIGHTS cmsg.
pub const SCM_MAX_FD: usize = 253;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmsghdr_layout() {
        assert_eq!(CMSGHDR_OFF_LEN, 0);
        assert_eq!(CMSGHDR_OFF_LEVEL, 8);
        assert_eq!(CMSGHDR_OFF_TYPE, 12);
        assert_eq!(CMSGHDR_SIZE, 16);
    }

    #[test]
    fn test_cmsg_align_is_8() {
        assert_eq!(CMSG_ALIGN_BYTES, 8);
        assert!(CMSG_ALIGN_BYTES.is_power_of_two());
    }

    #[test]
    fn test_scm_types_distinct() {
        let s = [
            SCM_RIGHTS,
            SCM_CREDENTIALS,
            SCM_SECURITY,
            SCM_PIDFD,
            SCM_TIMESTAMP,
            SCM_TIMESTAMPNS,
        ];
        for (i, &x) in s.iter().enumerate() {
            for &y in &s[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // SCM_RIGHTS is canonically 0x01.
        assert_eq!(SCM_RIGHTS, 1);
    }

    #[test]
    fn test_ip_cmsg_types_distinct() {
        let ip = [IP_PKTINFO, IP_TTL, IP_TOS, IP_RECVERR];
        for (i, &x) in ip.iter().enumerate() {
            for &y in &ip[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_ipv6_cmsg_types_distinct() {
        let ipv6 = [IPV6_PKTINFO, IPV6_HOPLIMIT, IPV6_TCLASS, IPV6_RECVERR];
        for (i, &x) in ipv6.iter().enumerate() {
            for &y in &ipv6[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_scm_max_fd_is_253() {
        // 253 chosen because cmsghdr + 253*sizeof(int) + alignment
        // stays under SCM_MAX_FD page-sized envelope.
        assert_eq!(SCM_MAX_FD, 253);
    }
}
