//! `<sys/socket.h>` — ancillary-data (control message) constants for `sendmsg`/`recvmsg`.
//!
//! Ancillary data ("control messages") rides alongside socket payload
//! to carry credentials, file descriptors, timestamps, packet metadata,
//! and per-protocol error info. Each `cmsghdr` carries a level + type
//! pair that selects the interpretation of the payload.

// ---------------------------------------------------------------------------
// SCM_* — generic socket-level (SOL_SOCKET) ancillary types
// ---------------------------------------------------------------------------

pub const SCM_RIGHTS: i32 = 0x01;
pub const SCM_CREDENTIALS: i32 = 0x02;
pub const SCM_SECURITY: i32 = 0x03;
pub const SCM_PIDFD: i32 = 0x04;

// ---------------------------------------------------------------------------
// Timestamping (`SOL_SOCKET` level, alongside SO_TIMESTAMP*)
// ---------------------------------------------------------------------------

pub const SCM_TIMESTAMP: i32 = 29; // matches SO_TIMESTAMP_OLD
pub const SCM_TIMESTAMPNS: i32 = 35; // matches SO_TIMESTAMPNS_OLD
pub const SCM_TIMESTAMPING: i32 = 37; // matches SO_TIMESTAMPING_OLD

// ---------------------------------------------------------------------------
// IPv4 ancillary types (`IPPROTO_IP` level)
// ---------------------------------------------------------------------------

pub const IP_PKTINFO: i32 = 8;
pub const IP_TTL: i32 = 2;
pub const IP_TOS: i32 = 1;
pub const IP_RECVERR: i32 = 11;
pub const IP_RECVOPTS: i32 = 6;
pub const IP_ORIGDSTADDR: i32 = 20;

// ---------------------------------------------------------------------------
// IPv6 ancillary types (`IPPROTO_IPV6` level)
// ---------------------------------------------------------------------------

pub const IPV6_PKTINFO: i32 = 50;
pub const IPV6_HOPLIMIT: i32 = 52;
pub const IPV6_TCLASS: i32 = 67;
pub const IPV6_RECVERR: i32 = 25;

// ---------------------------------------------------------------------------
// `msghdr.msg_flags` returned on recvmsg
// ---------------------------------------------------------------------------

pub const MSG_CTRUNC: i32 = 0x08;
pub const MSG_TRUNC: i32 = 0x20;
pub const MSG_OOB: i32 = 0x01;
pub const MSG_EOR: i32 = 0x80;
pub const MSG_ERRQUEUE: i32 = 0x2000;
pub const MSG_CMSG_CLOEXEC: i32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// `CMSG_ALIGN` — control-message alignment is the natural pointer size
// ---------------------------------------------------------------------------

pub const CMSG_ALIGN_BYTES: usize = core::mem::size_of::<usize>();

#[must_use]
pub const fn cmsg_align(n: usize) -> usize {
    // CMSG_ALIGN_BYTES is sizeof(usize) (>=4), so CMSG_ALIGN_BYTES - 1 cannot underflow.
    // n is bounded by control-message buffer sizes set by callers; saturating_add
    // protects against pathological inputs without changing the alignment semantics.
    n.saturating_add(CMSG_ALIGN_BYTES - 1) & !(CMSG_ALIGN_BYTES - 1)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scm_low_codes_dense_1_to_4() {
        assert_eq!(SCM_RIGHTS, 1);
        assert_eq!(SCM_CREDENTIALS, 2);
        assert_eq!(SCM_SECURITY, 3);
        assert_eq!(SCM_PIDFD, 4);
    }

    #[test]
    fn test_scm_timestamps_match_so_values() {
        // SCM_* values are deliberately equal to the matching SO_TIMESTAMP*
        // socket-option numbers so kernel code can reuse them.
        assert_eq!(SCM_TIMESTAMP, 29);
        assert_eq!(SCM_TIMESTAMPNS, 35);
        assert_eq!(SCM_TIMESTAMPING, 37);
    }

    #[test]
    fn test_ipv4_and_ipv6_pktinfo_distinct() {
        // PKTINFO uses different option numbers per family.
        assert_eq!(IP_PKTINFO, 8);
        assert_eq!(IPV6_PKTINFO, 50);
        assert_ne!(IP_PKTINFO, IPV6_PKTINFO);
        // Both RECVERRs are present.
        assert_eq!(IP_RECVERR, 11);
        assert_eq!(IPV6_RECVERR, 25);
    }

    #[test]
    fn test_msg_flags_each_a_distinct_bit() {
        let f = [
            MSG_CTRUNC,
            MSG_TRUNC,
            MSG_OOB,
            MSG_EOR,
            MSG_ERRQUEUE,
            MSG_CMSG_CLOEXEC,
        ];
        for v in f {
            assert!((v as u32).is_power_of_two());
        }
        // CMSG_CLOEXEC is the cloexec-on-received-fds bit (very high).
        assert_eq!(MSG_CMSG_CLOEXEC, 0x4000_0000);
    }

    #[test]
    fn test_cmsg_align_is_pointer_aligned() {
        // CMSG alignment is the natural word size of the host (4 or 8).
        assert!(CMSG_ALIGN_BYTES == 4 || CMSG_ALIGN_BYTES == 8);
        // align(0)=0, align(1)=8 on 64-bit, align(8)=8, align(9)=16.
        assert_eq!(cmsg_align(0), 0);
        assert_eq!(cmsg_align(1), CMSG_ALIGN_BYTES);
        assert_eq!(cmsg_align(CMSG_ALIGN_BYTES), CMSG_ALIGN_BYTES);
        assert_eq!(
            cmsg_align(CMSG_ALIGN_BYTES + 1),
            CMSG_ALIGN_BYTES * 2
        );
    }
}
