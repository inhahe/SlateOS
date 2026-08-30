//! `<linux/netlink.h>` — Netlink protocol and message header constants.
//!
//! Netlink is the primary kernel↔userspace communication mechanism
//! for network configuration. These constants define protocol
//! families, message header flags, and standard message types.

// ---------------------------------------------------------------------------
// Netlink protocol families
// ---------------------------------------------------------------------------

/// Routing/device hook.
pub const NETLINK_ROUTE: u32 = 0;
/// Unused (formerly USERSOCK).
pub const NETLINK_UNUSED: u32 = 1;
/// Firewall hook (ip_queue).
pub const NETLINK_FIREWALL: u32 = 3;
/// Socket monitoring (ss/netstat).
pub const NETLINK_SOCK_DIAG: u32 = 4;
/// Netfilter/iptables.
pub const NETLINK_NETFILTER: u32 = 12;
/// Kernel ↔ iscsi (open-iscsi).
pub const NETLINK_ISCSI: u32 = 8;
/// Audit subsystem.
pub const NETLINK_AUDIT: u32 = 9;
/// Connector (process events, etc.).
pub const NETLINK_CONNECTOR: u32 = 11;
/// Generic netlink (family multiplexer).
pub const NETLINK_GENERIC: u32 = 16;
/// SCSI transport events.
pub const NETLINK_SCSITRANSPORT: u32 = 18;
/// Crypto API events.
pub const NETLINK_CRYPTO: u32 = 21;
/// Kernel event notifications.
pub const NETLINK_KOBJECT_UEVENT: u32 = 15;

// ---------------------------------------------------------------------------
// Netlink message header flags (nlmsg_flags)
// ---------------------------------------------------------------------------

/// Request message (from user).
pub const NLM_F_REQUEST: u16 = 0x01;
/// Multipart message.
pub const NLM_F_MULTI: u16 = 0x02;
/// Reply with ACK on success.
pub const NLM_F_ACK: u16 = 0x04;
/// Echo the request back.
pub const NLM_F_ECHO: u16 = 0x08;
/// Dump was inconsistent (interrupted).
pub const NLM_F_DUMP_INTR: u16 = 0x10;
/// Dump was filtered.
pub const NLM_F_DUMP_FILTERED: u16 = 0x20;

// ---------------------------------------------------------------------------
// GET request flags
// ---------------------------------------------------------------------------

/// Return all matching entries.
pub const NLM_F_ROOT: u16 = 0x100;
/// Return all entries in table.
pub const NLM_F_MATCH: u16 = 0x200;
/// Return results atomically.
pub const NLM_F_ATOMIC: u16 = 0x400;
/// Dump convenience (ROOT | MATCH).
pub const NLM_F_DUMP: u16 = NLM_F_ROOT | NLM_F_MATCH;

// ---------------------------------------------------------------------------
// NEW request flags
// ---------------------------------------------------------------------------

/// Replace existing entry.
pub const NLM_F_REPLACE: u16 = 0x100;
/// Don't replace if exists.
pub const NLM_F_EXCL: u16 = 0x200;
/// Create entry if not exists.
pub const NLM_F_CREATE: u16 = 0x400;
/// Append to end of list.
pub const NLM_F_APPEND: u16 = 0x800;

// ---------------------------------------------------------------------------
// Standard message types
// ---------------------------------------------------------------------------

/// Nothing (ignore).
pub const NLMSG_NOOP: u16 = 0x1;
/// Error/ACK response.
pub const NLMSG_ERROR: u16 = 0x2;
/// End of multipart dump.
pub const NLMSG_DONE: u16 = 0x3;
/// Overrun notification.
pub const NLMSG_OVERRUN: u16 = 0x4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_families_distinct() {
        let protos = [
            NETLINK_ROUTE,
            NETLINK_UNUSED,
            NETLINK_FIREWALL,
            NETLINK_SOCK_DIAG,
            NETLINK_ISCSI,
            NETLINK_AUDIT,
            NETLINK_CONNECTOR,
            NETLINK_NETFILTER,
            NETLINK_KOBJECT_UEVENT,
            NETLINK_GENERIC,
            NETLINK_SCSITRANSPORT,
            NETLINK_CRYPTO,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_route_is_zero() {
        assert_eq!(NETLINK_ROUTE, 0);
    }

    #[test]
    fn test_header_flags_lower() {
        let flags = [
            NLM_F_REQUEST,
            NLM_F_MULTI,
            NLM_F_ACK,
            NLM_F_ECHO,
            NLM_F_DUMP_INTR,
            NLM_F_DUMP_FILTERED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dump_combination() {
        assert_eq!(NLM_F_DUMP, NLM_F_ROOT | NLM_F_MATCH);
    }

    #[test]
    fn test_standard_msg_types() {
        assert_eq!(NLMSG_NOOP, 1);
        assert_eq!(NLMSG_ERROR, 2);
        assert_eq!(NLMSG_DONE, 3);
        assert_eq!(NLMSG_OVERRUN, 4);
    }
}
