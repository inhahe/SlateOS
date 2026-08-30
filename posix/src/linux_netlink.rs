//! `<linux/netlink.h>` — netlink socket protocol.
//!
//! Netlink is the primary mechanism for kernel↔userspace communication
//! on Linux: routing table management, interface configuration, firewall
//! rules (nftables), audit events, connector notifications, and more.

// ---------------------------------------------------------------------------
// Netlink protocol families
// ---------------------------------------------------------------------------

/// Routing/link updates.
pub const NETLINK_ROUTE: i32 = 0;
/// Unused / reserved.
pub const NETLINK_UNUSED: i32 = 1;
/// Userspace socket diagnostics.
pub const NETLINK_USERSOCK: i32 = 2;
/// Firewall (ip_queue, obsolete).
pub const NETLINK_FIREWALL: i32 = 3;
/// Socket monitoring (ss, netstat).
pub const NETLINK_SOCK_DIAG: i32 = 4;
/// Netfilter/iptables (nflog, nfqueue).
pub const NETLINK_NFLOG: i32 = 5;
/// IPsec/XFRM.
pub const NETLINK_XFRM: i32 = 6;
/// SELinux event notifications.
pub const NETLINK_SELINUX: i32 = 7;
/// iSCSI subsystem.
pub const NETLINK_ISCSI: i32 = 8;
/// Kernel audit subsystem.
pub const NETLINK_AUDIT: i32 = 9;
/// FIB lookup.
pub const NETLINK_FIB_LOOKUP: i32 = 10;
/// Kernel connector.
pub const NETLINK_CONNECTOR: i32 = 11;
/// Netfilter subsystem.
pub const NETLINK_NETFILTER: i32 = 12;
/// IPv6 firewall.
pub const NETLINK_IP6_FW: i32 = 13;
/// DECnet routing.
pub const NETLINK_DNRTMSG: i32 = 14;
/// Kernel message to userspace.
pub const NETLINK_KOBJECT_UEVENT: i32 = 15;
/// Generic netlink.
pub const NETLINK_GENERIC: i32 = 16;
/// SCSI transport.
pub const NETLINK_SCSITRANSPORT: i32 = 18;
/// ECRYPTFS.
pub const NETLINK_ECRYPTFS: i32 = 19;
/// RDMA/InfiniBand.
pub const NETLINK_RDMA: i32 = 20;
/// Crypto API.
pub const NETLINK_CRYPTO: i32 = 21;
/// SMC monitoring.
pub const NETLINK_SMC: i32 = 22;

// ---------------------------------------------------------------------------
// Netlink message flags (nlmsg_flags)
// ---------------------------------------------------------------------------

/// Must be set on all requests.
pub const NLM_F_REQUEST: u16 = 0x01;
/// Multipart message, terminated by NLMSG_DONE.
pub const NLM_F_MULTI: u16 = 0x02;
/// Reply with ACK on success.
pub const NLM_F_ACK: u16 = 0x04;
/// Echo this request back.
pub const NLM_F_ECHO: u16 = 0x08;
/// Dump was inconsistent (interrupted).
pub const NLM_F_DUMP_INTR: u16 = 0x10;
/// Dump was filtered (partial).
pub const NLM_F_DUMP_FILTERED: u16 = 0x20;

// GET request flags
/// Return the whole table (dump).
pub const NLM_F_ROOT: u16 = 0x100;
/// Return matching entries.
pub const NLM_F_MATCH: u16 = 0x200;
/// Atomic dump (deprecated).
pub const NLM_F_ATOMIC: u16 = 0x400;
/// Dump convenience macro (ROOT | MATCH).
pub const NLM_F_DUMP: u16 = NLM_F_ROOT | NLM_F_MATCH;

// NEW request flags
/// Replace existing matching.
pub const NLM_F_REPLACE: u16 = 0x100;
/// Don't replace if it exists.
pub const NLM_F_EXCL: u16 = 0x200;
/// Create if it doesn't exist.
pub const NLM_F_CREATE: u16 = 0x400;
/// Add to end of list.
pub const NLM_F_APPEND: u16 = 0x800;

// ---------------------------------------------------------------------------
// Standard netlink message types
// ---------------------------------------------------------------------------

/// Nothing.
pub const NLMSG_NOOP: u16 = 0x1;
/// Error.
pub const NLMSG_ERROR: u16 = 0x2;
/// End of multipart dump.
pub const NLMSG_DONE: u16 = 0x3;
/// Data lost.
pub const NLMSG_OVERRUN: u16 = 0x4;
/// Start of user message types.
pub const NLMSG_MIN_TYPE: u16 = 0x10;

// ---------------------------------------------------------------------------
// Netlink message header
// ---------------------------------------------------------------------------

/// Netlink message header (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Nlmsghdr {
    /// Length of message including header.
    pub nlmsg_len: u32,
    /// Message type.
    pub nlmsg_type: u16,
    /// Additional flags.
    pub nlmsg_flags: u16,
    /// Sequence number.
    pub nlmsg_seq: u32,
    /// Sending process port ID.
    pub nlmsg_pid: u32,
}

impl Nlmsghdr {
    /// Create a zeroed netlink message header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Netlink socket address (12 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrNl {
    /// Address family (AF_NETLINK).
    pub nl_family: u16,
    /// Padding.
    _pad: u16,
    /// Port ID (PID).
    pub nl_pid: u32,
    /// Multicast group mask.
    pub nl_groups: u32,
}

impl SockaddrNl {
    /// Create a zeroed netlink socket address.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------

/// Netlink message alignment (4 bytes).
pub const NLMSG_ALIGNTO: usize = 4;

/// Align a length to NLMSG_ALIGNTO boundary.
pub const fn nlmsg_align(len: usize) -> usize {
    // NLMSG_ALIGNTO is a small constant power of two, so `- 1` cannot
    // underflow.  The `+ NLMSG_ALIGNTO - 1` would only wrap for
    // pathological `len` near `usize::MAX`; use wrapping_add for the
    // const-context "round-up" trick.
    len.wrapping_add(NLMSG_ALIGNTO.wrapping_sub(1)) & !NLMSG_ALIGNTO.wrapping_sub(1)
}

/// Size of the netlink message header.
pub const NLMSG_HDRLEN: usize = nlmsg_align(core::mem::size_of::<Nlmsghdr>());

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nlmsghdr_size() {
        assert_eq!(core::mem::size_of::<Nlmsghdr>(), 16);
    }

    #[test]
    fn test_sockaddr_nl_size() {
        assert_eq!(core::mem::size_of::<SockaddrNl>(), 12);
    }

    #[test]
    fn test_nlmsghdr_zeroed() {
        let hdr = Nlmsghdr::zeroed();
        assert_eq!(hdr.nlmsg_len, 0);
        assert_eq!(hdr.nlmsg_type, 0);
        assert_eq!(hdr.nlmsg_flags, 0);
        assert_eq!(hdr.nlmsg_seq, 0);
        assert_eq!(hdr.nlmsg_pid, 0);
    }

    #[test]
    fn test_netlink_protocols_distinct() {
        let protos = [
            NETLINK_ROUTE,
            NETLINK_UNUSED,
            NETLINK_USERSOCK,
            NETLINK_FIREWALL,
            NETLINK_SOCK_DIAG,
            NETLINK_NFLOG,
            NETLINK_XFRM,
            NETLINK_SELINUX,
            NETLINK_ISCSI,
            NETLINK_AUDIT,
            NETLINK_FIB_LOOKUP,
            NETLINK_CONNECTOR,
            NETLINK_NETFILTER,
            NETLINK_IP6_FW,
            NETLINK_DNRTMSG,
            NETLINK_KOBJECT_UEVENT,
            NETLINK_GENERIC,
            NETLINK_SCSITRANSPORT,
            NETLINK_ECRYPTFS,
            NETLINK_RDMA,
            NETLINK_CRYPTO,
            NETLINK_SMC,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_nlm_flags() {
        assert_eq!(NLM_F_REQUEST, 0x01);
        assert_eq!(NLM_F_MULTI, 0x02);
        assert_eq!(NLM_F_ACK, 0x04);
        assert_eq!(NLM_F_DUMP, NLM_F_ROOT | NLM_F_MATCH);
    }

    #[test]
    fn test_nlmsg_types() {
        assert_eq!(NLMSG_NOOP, 1);
        assert_eq!(NLMSG_ERROR, 2);
        assert_eq!(NLMSG_DONE, 3);
        assert_eq!(NLMSG_OVERRUN, 4);
        assert_eq!(NLMSG_MIN_TYPE, 0x10);
    }

    #[test]
    fn test_nlmsg_align() {
        assert_eq!(nlmsg_align(1), 4);
        assert_eq!(nlmsg_align(4), 4);
        assert_eq!(nlmsg_align(5), 8);
        assert_eq!(nlmsg_align(16), 16);
        assert_eq!(nlmsg_align(17), 20);
    }

    #[test]
    fn test_nlmsg_hdrlen() {
        assert_eq!(NLMSG_HDRLEN, 16);
    }
}
