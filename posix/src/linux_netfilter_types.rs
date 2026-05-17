//! `<linux/netfilter.h>` — Netfilter hooks and verdict constants.
//!
//! Netfilter is the kernel's packet filtering framework. Packets
//! traverse a series of hooks at defined points in the network stack
//! (PREROUTING, INPUT, FORWARD, OUTPUT, POSTROUTING). At each hook,
//! registered callbacks examine the packet and return a verdict
//! (ACCEPT, DROP, QUEUE, etc.). iptables, nftables, and eBPF all
//! use netfilter hooks to implement firewalls, NAT, and traffic
//! shaping.

// ---------------------------------------------------------------------------
// Netfilter hook points
// ---------------------------------------------------------------------------

/// Before routing decision (incoming packets).
pub const NF_INET_PRE_ROUTING: u32 = 0;
/// After routing, destined for local delivery.
pub const NF_INET_LOCAL_IN: u32 = 1;
/// Packet is being forwarded (not for us).
pub const NF_INET_FORWARD: u32 = 2;
/// Locally-generated outgoing packet.
pub const NF_INET_LOCAL_OUT: u32 = 3;
/// After routing, about to leave the machine.
pub const NF_INET_POST_ROUTING: u32 = 4;
/// Number of hook points.
pub const NF_INET_NUMHOOKS: u32 = 5;

// ---------------------------------------------------------------------------
// Netfilter verdicts
// ---------------------------------------------------------------------------

/// Accept the packet (continue processing).
pub const NF_DROP: u32 = 0;
/// Accept the packet.
pub const NF_ACCEPT: u32 = 1;
/// Packet was stolen (handler took ownership).
pub const NF_STOLEN: u32 = 2;
/// Queue packet to userspace (nfqueue).
pub const NF_QUEUE: u32 = 3;
/// Call the next hook (iterate).
pub const NF_REPEAT: u32 = 4;
/// Stop processing (deprecated).
pub const NF_STOP: u32 = 5;

// ---------------------------------------------------------------------------
// Netfilter protocol families
// ---------------------------------------------------------------------------

/// IPv4 family.
pub const NFPROTO_IPV4: u32 = 2;
/// IPv6 family.
pub const NFPROTO_IPV6: u32 = 10;
/// ARP family.
pub const NFPROTO_ARP: u32 = 3;
/// Bridge family (Ethernet bridging).
pub const NFPROTO_BRIDGE: u32 = 7;
/// Unspecified (all families).
pub const NFPROTO_UNSPEC: u32 = 0;
/// Inet (dual-stack IPv4+IPv6).
pub const NFPROTO_INET: u32 = 1;
/// Netdev (ingress/egress on device).
pub const NFPROTO_NETDEV: u32 = 5;

// ---------------------------------------------------------------------------
// Hook priority ranges (lower = earlier execution)
// ---------------------------------------------------------------------------

/// First priority (before everything).
pub const NF_IP_PRI_FIRST: i32 = -400;
/// Connection tracking priority.
pub const NF_IP_PRI_CONNTRACK: i32 = -200;
/// Mangle table priority.
pub const NF_IP_PRI_MANGLE: i32 = -150;
/// NAT destination priority.
pub const NF_IP_PRI_NAT_DST: i32 = -100;
/// Filter table priority (default).
pub const NF_IP_PRI_FILTER: i32 = 0;
/// Security table priority.
pub const NF_IP_PRI_SECURITY: i32 = 50;
/// NAT source priority.
pub const NF_IP_PRI_NAT_SRC: i32 = 100;
/// Last priority (after everything).
pub const NF_IP_PRI_LAST: i32 = 400;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hooks_distinct() {
        let hooks = [
            NF_INET_PRE_ROUTING, NF_INET_LOCAL_IN, NF_INET_FORWARD,
            NF_INET_LOCAL_OUT, NF_INET_POST_ROUTING,
        ];
        assert_eq!(hooks.len(), NF_INET_NUMHOOKS as usize);
        for i in 0..hooks.len() {
            for j in (i + 1)..hooks.len() {
                assert_ne!(hooks[i], hooks[j]);
            }
        }
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

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            NFPROTO_UNSPEC, NFPROTO_INET, NFPROTO_IPV4,
            NFPROTO_ARP, NFPROTO_NETDEV, NFPROTO_BRIDGE,
            NFPROTO_IPV6,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_priorities_ordered() {
        assert!(NF_IP_PRI_FIRST < NF_IP_PRI_CONNTRACK);
        assert!(NF_IP_PRI_CONNTRACK < NF_IP_PRI_MANGLE);
        assert!(NF_IP_PRI_MANGLE < NF_IP_PRI_FILTER);
        assert!(NF_IP_PRI_FILTER < NF_IP_PRI_NAT_SRC);
        assert!(NF_IP_PRI_NAT_SRC < NF_IP_PRI_LAST);
    }
}
