//! `<linux/netfilter/nfnetlink.h>` — Netfilter netlink constants.
//!
//! Nfnetlink is the netlink-based transport for configuring the
//! kernel's Netfilter framework (nftables, conntrack, logging, etc.).
//! Each Netfilter subsystem registers a handler for its own nfnetlink
//! subsystem ID and communicates via typed messages.

// ---------------------------------------------------------------------------
// Nfnetlink subsystem IDs
// ---------------------------------------------------------------------------

/// No subsystem (unused).
pub const NFNL_SUBSYS_NONE: u32 = 0;
/// Conntrack events and queries.
pub const NFNL_SUBSYS_CTNETLINK: u32 = 1;
/// Conntrack expect entries.
pub const NFNL_SUBSYS_CTNETLINK_EXP: u32 = 2;
/// Packet queuing (NFQUEUE).
pub const NFNL_SUBSYS_QUEUE: u32 = 3;
/// Userspace logging (NFLOG).
pub const NFNL_SUBSYS_ULOG: u32 = 4;
/// iptables compatibility.
pub const NFNL_SUBSYS_OSF: u32 = 5;
/// IP sets (ipset).
pub const NFNL_SUBSYS_IPSET: u32 = 6;
/// Connection tracking timeout policies.
pub const NFNL_SUBSYS_ACCT: u32 = 7;
/// Conntrack timeout tuning.
pub const NFNL_SUBSYS_CTNETLINK_TIMEOUT: u32 = 8;
/// Conntrack helpers.
pub const NFNL_SUBSYS_CTHELPER: u32 = 9;
/// nftables core.
pub const NFNL_SUBSYS_NFTABLES: u32 = 10;
/// nf_tables compatibility layer.
pub const NFNL_SUBSYS_NFT_COMPAT: u32 = 11;
/// Hook subsystem.
pub const NFNL_SUBSYS_HOOK: u32 = 12;
/// Maximum subsystem ID (for bounds checking).
pub const NFNL_SUBSYS_COUNT: u32 = 13;

// ---------------------------------------------------------------------------
// Nfnetlink message flags (in addition to standard NLM_F_*)
// ---------------------------------------------------------------------------

/// Batch begin marker (nftables atomic updates).
pub const NFNL_MSG_BATCH_BEGIN: u32 = 0x0010;
/// Batch end marker.
pub const NFNL_MSG_BATCH_END: u32 = 0x0011;

// ---------------------------------------------------------------------------
// Netfilter protocol families
// ---------------------------------------------------------------------------

/// Unspecified (any protocol).
pub const NFPROTO_UNSPEC: u32 = 0;
/// IPv4 (INET).
pub const NFPROTO_INET: u32 = 1;
/// IPv4.
pub const NFPROTO_IPV4: u32 = 2;
/// ARP.
pub const NFPROTO_ARP: u32 = 3;
/// Netdev (ingress/egress).
pub const NFPROTO_NETDEV: u32 = 5;
/// Bridge (ebtables).
pub const NFPROTO_BRIDGE: u32 = 7;
/// IPv6.
pub const NFPROTO_IPV6: u32 = 10;

// ---------------------------------------------------------------------------
// Netfilter hook points
// ---------------------------------------------------------------------------

/// Pre-routing hook.
pub const NF_INET_PRE_ROUTING: u32 = 0;
/// Local-in hook.
pub const NF_INET_LOCAL_IN: u32 = 1;
/// Forward hook.
pub const NF_INET_FORWARD: u32 = 2;
/// Local-out hook.
pub const NF_INET_LOCAL_OUT: u32 = 3;
/// Post-routing hook.
pub const NF_INET_POST_ROUTING: u32 = 4;
/// Number of hook points.
pub const NF_INET_NUMHOOKS: u32 = 5;

// ---------------------------------------------------------------------------
// Netfilter verdicts
// ---------------------------------------------------------------------------

/// Drop the packet.
pub const NF_DROP: u32 = 0;
/// Accept the packet.
pub const NF_ACCEPT: u32 = 1;
/// Stolen (handler consumed the packet).
pub const NF_STOLEN: u32 = 2;
/// Queue to userspace.
pub const NF_QUEUE: u32 = 3;
/// Repeat the hook.
pub const NF_REPEAT: u32 = 4;
/// Stop processing (accept + skip rest of chain).
pub const NF_STOP: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subsys_ids_sequential() {
        assert_eq!(NFNL_SUBSYS_NONE, 0);
        assert_eq!(NFNL_SUBSYS_CTNETLINK, 1);
        assert_eq!(NFNL_SUBSYS_NFTABLES, 10);
        assert!(NFNL_SUBSYS_COUNT > NFNL_SUBSYS_HOOK);
    }

    #[test]
    fn test_subsys_ids_distinct() {
        let ids = [
            NFNL_SUBSYS_NONE,
            NFNL_SUBSYS_CTNETLINK,
            NFNL_SUBSYS_CTNETLINK_EXP,
            NFNL_SUBSYS_QUEUE,
            NFNL_SUBSYS_ULOG,
            NFNL_SUBSYS_OSF,
            NFNL_SUBSYS_IPSET,
            NFNL_SUBSYS_ACCT,
            NFNL_SUBSYS_CTNETLINK_TIMEOUT,
            NFNL_SUBSYS_CTHELPER,
            NFNL_SUBSYS_NFTABLES,
            NFNL_SUBSYS_NFT_COMPAT,
            NFNL_SUBSYS_HOOK,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_batch_msgs_distinct() {
        assert_ne!(NFNL_MSG_BATCH_BEGIN, NFNL_MSG_BATCH_END);
    }

    #[test]
    fn test_protocol_families_distinct() {
        let protos = [
            NFPROTO_UNSPEC,
            NFPROTO_INET,
            NFPROTO_IPV4,
            NFPROTO_ARP,
            NFPROTO_NETDEV,
            NFPROTO_BRIDGE,
            NFPROTO_IPV6,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_hooks_sequential() {
        assert_eq!(NF_INET_PRE_ROUTING, 0);
        assert_eq!(NF_INET_LOCAL_IN, 1);
        assert_eq!(NF_INET_FORWARD, 2);
        assert_eq!(NF_INET_LOCAL_OUT, 3);
        assert_eq!(NF_INET_POST_ROUTING, 4);
        assert_eq!(NF_INET_NUMHOOKS, 5);
    }

    #[test]
    fn test_verdicts_distinct() {
        let verdicts = [NF_DROP, NF_ACCEPT, NF_STOLEN, NF_QUEUE, NF_REPEAT, NF_STOP];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
    }
}
