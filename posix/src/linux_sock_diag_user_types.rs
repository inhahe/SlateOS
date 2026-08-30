//! `<linux/sock_diag.h>` — `NETLINK_SOCK_DIAG` ABI.
//!
//! `ss(8)` (the modern `netstat` replacement) queries the kernel's
//! socket tables via `NETLINK_SOCK_DIAG`. Each protocol family
//! defines its own diag request/response (TCP_DIAG, INET_DIAG,
//! UNIX_DIAG, NETLINK_DIAG, PACKET_DIAG, SMC_DIAG, MPTCP_DIAG).
//! The numbers here are the stable family ids and the
//! common-extension TLV ids.

// ---------------------------------------------------------------------------
// Netlink protocol id
// ---------------------------------------------------------------------------

pub const NETLINK_SOCK_DIAG: u32 = 4;

// ---------------------------------------------------------------------------
// Per-family diag protocol numbers (`SOCK_DIAG_BY_FAMILY` selector)
// ---------------------------------------------------------------------------

pub const TCPDIAG_GETSOCK: u16 = 18;
pub const DCCPDIAG_GETSOCK: u16 = 19;
pub const SOCK_DIAG_BY_FAMILY: u16 = 20;
pub const SOCK_DESTROY: u16 = 21;

// ---------------------------------------------------------------------------
// Common extension TLVs (`INET_DIAG_*`)
// ---------------------------------------------------------------------------

pub const INET_DIAG_NONE: u16 = 0;
pub const INET_DIAG_MEMINFO: u16 = 1;
pub const INET_DIAG_INFO: u16 = 2;
pub const INET_DIAG_VEGASINFO: u16 = 3;
pub const INET_DIAG_CONG: u16 = 4;
pub const INET_DIAG_TOS: u16 = 5;
pub const INET_DIAG_TCLASS: u16 = 6;
pub const INET_DIAG_SKMEMINFO: u16 = 7;
pub const INET_DIAG_SHUTDOWN: u16 = 8;
pub const INET_DIAG_DCTCPINFO: u16 = 9;
pub const INET_DIAG_PROTOCOL: u16 = 10;
pub const INET_DIAG_SKV6ONLY: u16 = 11;
pub const INET_DIAG_LOCALS: u16 = 12;
pub const INET_DIAG_PEERS: u16 = 13;
pub const INET_DIAG_PAD: u16 = 14;
pub const INET_DIAG_MARK: u16 = 15;
pub const INET_DIAG_BBRINFO: u16 = 16;
pub const INET_DIAG_CLASS_ID: u16 = 17;
pub const INET_DIAG_MD5SIG: u16 = 18;
pub const INET_DIAG_ULP_INFO: u16 = 19;
pub const INET_DIAG_SK_BPF_STORAGES: u16 = 20;
pub const INET_DIAG_CGROUP_ID: u16 = 21;
pub const INET_DIAG_SOCKOPT: u16 = 22;

// ---------------------------------------------------------------------------
// UDP / packet / unix diag selectors (per-family bitmap)
// ---------------------------------------------------------------------------

pub const UDIAG_SHOW_NAME: u32 = 1 << 0;
pub const UDIAG_SHOW_VFS: u32 = 1 << 1;
pub const UDIAG_SHOW_PEER: u32 = 1 << 2;
pub const UDIAG_SHOW_ICONS: u32 = 1 << 3;
pub const UDIAG_SHOW_RQLEN: u32 = 1 << 4;
pub const UDIAG_SHOW_MEMINFO: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_netlink_sock_diag_is_4() {
        // NETLINK_SOCK_DIAG was originally NETLINK_INET_DIAG (4).
        assert_eq!(NETLINK_SOCK_DIAG, 4);
    }

    #[test]
    fn test_diag_message_ids_dense() {
        // Message types 18..21 form a small dense block.
        assert_eq!(TCPDIAG_GETSOCK, 18);
        assert_eq!(DCCPDIAG_GETSOCK, 19);
        assert_eq!(SOCK_DIAG_BY_FAMILY, 20);
        assert_eq!(SOCK_DESTROY, 21);
    }

    #[test]
    fn test_inet_diag_tlvs_dense_0_to_22() {
        let t = [
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
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_udiag_show_low_6_bits_dense() {
        let u = [
            UDIAG_SHOW_NAME,
            UDIAG_SHOW_VFS,
            UDIAG_SHOW_PEER,
            UDIAG_SHOW_ICONS,
            UDIAG_SHOW_RQLEN,
            UDIAG_SHOW_MEMINFO,
        ];
        let mut or = 0u32;
        for (i, v) in u.iter().enumerate() {
            assert_eq!(*v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0x3F);
    }
}
