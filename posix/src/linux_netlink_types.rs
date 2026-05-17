//! `<linux/netlink.h>` — Netlink protocol family constants.
//!
//! Netlink is a socket-based IPC mechanism between kernel and userspace
//! (and between userspace processes). It's used for network configuration
//! (iproute2), firewall rules (nftables), audit events, SELinux
//! notifications, connector callbacks, and more. Each netlink family
//! handles a specific subsystem.

// ---------------------------------------------------------------------------
// Netlink protocol families (socket(AF_NETLINK, ..., protocol))
// ---------------------------------------------------------------------------

/// Routing and link updates (iproute2).
pub const NETLINK_ROUTE: u32 = 0;
/// Unused (was ip_queue).
pub const NETLINK_UNUSED: u32 = 1;
/// Userspace socket diagnostics.
pub const NETLINK_SOCK_DIAG: u32 = 4;
/// Netfilter (nftables, conntrack).
pub const NETLINK_NETFILTER: u32 = 12;
/// XFRM (IPsec) events.
pub const NETLINK_XFRM: u32 = 6;
/// SELinux notifications.
pub const NETLINK_SELINUX: u32 = 7;
/// Audit events.
pub const NETLINK_AUDIT: u32 = 9;
/// Kernel connector.
pub const NETLINK_CONNECTOR: u32 = 11;
/// Generic netlink (multiplex).
pub const NETLINK_GENERIC: u32 = 16;
/// Kobject uevent (udev events).
pub const NETLINK_KOBJECT_UEVENT: u32 = 15;
/// Crypto API.
pub const NETLINK_CRYPTO: u32 = 21;

// ---------------------------------------------------------------------------
// Netlink message types (nlmsghdr.nlmsg_type)
// ---------------------------------------------------------------------------

/// No-op message.
pub const NLMSG_NOOP: u16 = 0x1;
/// Error/ack message.
pub const NLMSG_ERROR: u16 = 0x2;
/// End of multi-part message.
pub const NLMSG_DONE: u16 = 0x3;
/// Data lost (overrun).
pub const NLMSG_OVERRUN: u16 = 0x4;

// ---------------------------------------------------------------------------
// Netlink message flags (nlmsghdr.nlmsg_flags)
// ---------------------------------------------------------------------------

/// Must reply with ack.
pub const NLM_F_REQUEST: u16 = 0x0001;
/// Multi-part message.
pub const NLM_F_MULTI: u16 = 0x0002;
/// Reply with ack.
pub const NLM_F_ACK: u16 = 0x0004;
/// Echo this request back.
pub const NLM_F_ECHO: u16 = 0x0008;
/// Dump all entries (GET).
pub const NLM_F_DUMP: u16 = 0x0300;
/// Return the root of the tree.
pub const NLM_F_ROOT: u16 = 0x0100;
/// Return all entries matching criteria.
pub const NLM_F_MATCH: u16 = 0x0200;
/// Atomic snapshot (GET).
pub const NLM_F_ATOMIC: u16 = 0x0400;
/// Replace existing (NEW).
pub const NLM_F_REPLACE: u16 = 0x0100;
/// Don't replace if exists (NEW).
pub const NLM_F_EXCL: u16 = 0x0200;
/// Create if not exists (NEW).
pub const NLM_F_CREATE: u16 = 0x0400;
/// Add to end (NEW).
pub const NLM_F_APPEND: u16 = 0x0800;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_families_distinct() {
        let families = [
            NETLINK_ROUTE, NETLINK_UNUSED, NETLINK_SOCK_DIAG,
            NETLINK_XFRM, NETLINK_SELINUX, NETLINK_AUDIT,
            NETLINK_CONNECTOR, NETLINK_NETFILTER,
            NETLINK_KOBJECT_UEVENT, NETLINK_GENERIC, NETLINK_CRYPTO,
        ];
        for i in 0..families.len() {
            for j in (i + 1)..families.len() {
                assert_ne!(families[i], families[j]);
            }
        }
    }

    #[test]
    fn test_msg_types_distinct() {
        let types = [NLMSG_NOOP, NLMSG_ERROR, NLMSG_DONE, NLMSG_OVERRUN];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_request_flags() {
        // REQUEST, MULTI, ACK, ECHO are low bits and don't overlap
        assert_eq!(NLM_F_REQUEST & NLM_F_MULTI, 0);
        assert_eq!(NLM_F_REQUEST & NLM_F_ACK, 0);
        assert_eq!(NLM_F_MULTI & NLM_F_ACK, 0);
        assert_eq!(NLM_F_ACK & NLM_F_ECHO, 0);
    }
}
