//! `<linux/netlink.h>` — core netlink ABI.
//!
//! Netlink is Linux's primary kernel↔userspace structured-message bus.
//! `iproute2`, `ip`, `tc`, NetworkManager, `audit`, `selinux`, and
//! every tool that doesn't predate Linux 2.0 uses one of the netlink
//! families to read or modify kernel state. This file defines the
//! shared message header, flags, and family numbers.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// `AF_NETLINK` / `PF_NETLINK`.
pub const AF_NETLINK: u32 = 16;

// ---------------------------------------------------------------------------
// Netlink protocol families (passed as the third argument to `socket(2)`)
// ---------------------------------------------------------------------------

pub const NETLINK_ROUTE: u32 = 0;
pub const NETLINK_UNUSED: u32 = 1;
pub const NETLINK_USERSOCK: u32 = 2;
pub const NETLINK_FIREWALL: u32 = 3;
pub const NETLINK_SOCK_DIAG: u32 = 4;
pub const NETLINK_NFLOG: u32 = 5;
pub const NETLINK_XFRM: u32 = 6;
pub const NETLINK_SELINUX: u32 = 7;
pub const NETLINK_ISCSI: u32 = 8;
pub const NETLINK_AUDIT: u32 = 9;
pub const NETLINK_FIB_LOOKUP: u32 = 10;
pub const NETLINK_CONNECTOR: u32 = 11;
pub const NETLINK_NETFILTER: u32 = 12;
pub const NETLINK_IP6_FW: u32 = 13;
pub const NETLINK_DNRTMSG: u32 = 14;
pub const NETLINK_KOBJECT_UEVENT: u32 = 15;
pub const NETLINK_GENERIC: u32 = 16;
pub const NETLINK_SCSITRANSPORT: u32 = 18;
pub const NETLINK_ECRYPTFS: u32 = 19;
pub const NETLINK_RDMA: u32 = 20;
pub const NETLINK_CRYPTO: u32 = 21;
pub const NETLINK_SMC: u32 = 22;

// ---------------------------------------------------------------------------
// `struct nlmsghdr.nlmsg_flags` — common
// ---------------------------------------------------------------------------

pub const NLM_F_REQUEST: u16 = 1 << 0;
pub const NLM_F_MULTI: u16 = 1 << 1;
pub const NLM_F_ACK: u16 = 1 << 2;
pub const NLM_F_ECHO: u16 = 1 << 3;
pub const NLM_F_DUMP_INTR: u16 = 1 << 4;
pub const NLM_F_DUMP_FILTERED: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// Modifiers for `GET` requests
// ---------------------------------------------------------------------------

pub const NLM_F_ROOT: u16 = 0x100;
pub const NLM_F_MATCH: u16 = 0x200;
pub const NLM_F_ATOMIC: u16 = 0x400;
pub const NLM_F_DUMP: u16 = NLM_F_ROOT | NLM_F_MATCH;

// ---------------------------------------------------------------------------
// Modifiers for `NEW` requests
// ---------------------------------------------------------------------------

pub const NLM_F_REPLACE: u16 = 0x100;
pub const NLM_F_EXCL: u16 = 0x200;
pub const NLM_F_CREATE: u16 = 0x400;
pub const NLM_F_APPEND: u16 = 0x800;

// ---------------------------------------------------------------------------
// Modifiers for `DELETE` requests
// ---------------------------------------------------------------------------

pub const NLM_F_NONREC: u16 = 0x100;
pub const NLM_F_BULK: u16 = 0x200;

// ---------------------------------------------------------------------------
// Modifiers for `ACK`
// ---------------------------------------------------------------------------

pub const NLM_F_CAPPED: u16 = 0x100;
pub const NLM_F_ACK_TLVS: u16 = 0x200;

// ---------------------------------------------------------------------------
// Standard message types
// ---------------------------------------------------------------------------

pub const NLMSG_NOOP: u16 = 1;
pub const NLMSG_ERROR: u16 = 2;
pub const NLMSG_DONE: u16 = 3;
pub const NLMSG_OVERRUN: u16 = 4;
pub const NLMSG_MIN_TYPE: u16 = 0x10;

// ---------------------------------------------------------------------------
// Alignment constants
// ---------------------------------------------------------------------------

/// All netlink-message fields are 4-byte aligned.
pub const NLMSG_ALIGNTO: usize = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_netlink() {
        assert_eq!(AF_NETLINK, 16);
    }

    #[test]
    fn test_family_numbers_dense_block() {
        // NETLINK_ROUTE..NETLINK_KOBJECT_UEVENT are 0..15 (dense).
        let f = [
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
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_common_flags_low_bits_single() {
        let c = [
            NLM_F_REQUEST,
            NLM_F_MULTI,
            NLM_F_ACK,
            NLM_F_ECHO,
            NLM_F_DUMP_INTR,
            NLM_F_DUMP_FILTERED,
        ];
        for v in c {
            assert!(v.is_power_of_two());
        }
        // Six dense bits 0..5.
        assert_eq!(c.iter().fold(0u16, |a, b| a | b), 0x3F);
    }

    #[test]
    fn test_dump_composite() {
        // DUMP is ROOT | MATCH.
        assert_eq!(NLM_F_DUMP, NLM_F_ROOT | NLM_F_MATCH);
    }

    #[test]
    fn test_high_modifier_bits_share_layout() {
        // Per-operation modifier bits all live in the high byte (0x100..0x800).
        for v in [
            NLM_F_ROOT,
            NLM_F_MATCH,
            NLM_F_ATOMIC,
            NLM_F_REPLACE,
            NLM_F_EXCL,
            NLM_F_CREATE,
            NLM_F_APPEND,
            NLM_F_NONREC,
            NLM_F_BULK,
            NLM_F_CAPPED,
            NLM_F_ACK_TLVS,
        ] {
            assert!(v >= 0x100);
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_standard_message_types() {
        // 0 is reserved; 1..4 are the standard types.
        assert_eq!(NLMSG_NOOP, 1);
        assert_eq!(NLMSG_ERROR, 2);
        assert_eq!(NLMSG_DONE, 3);
        assert_eq!(NLMSG_OVERRUN, 4);
        // User-defined types start at 0x10.
        assert_eq!(NLMSG_MIN_TYPE, 0x10);
        assert!(NLMSG_MIN_TYPE > NLMSG_OVERRUN);
    }

    #[test]
    fn test_align_to_is_4() {
        assert_eq!(NLMSG_ALIGNTO, 4);
        assert!(NLMSG_ALIGNTO.is_power_of_two());
    }
}
