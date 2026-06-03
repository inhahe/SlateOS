//! `<linux/netfilter.h>` — top-level netfilter ABI shared by nftables and iptables.
//!
//! Netfilter is the kernel framework underlying iptables, nftables,
//! conntrack, and NAT. This file collects the constants that are
//! shared across all netfilter clients: hook names, verdicts, and
//! the address-family enumeration.

// ---------------------------------------------------------------------------
// Verdicts returned from netfilter hooks
// ---------------------------------------------------------------------------

/// Drop the packet silently.
pub const NF_DROP: u32 = 0;
/// Accept the packet — continue traversal.
pub const NF_ACCEPT: u32 = 1;
/// Steal the packet — caller takes ownership.
pub const NF_STOLEN: u32 = 2;
/// Queue the packet to userspace via `nfnetlink_queue`.
pub const NF_QUEUE: u32 = 3;
/// Re-traverse the hook (used by NAT and helpers).
pub const NF_REPEAT: u32 = 4;
/// Bypass all subsequent hooks.
pub const NF_STOP: u32 = 5;
/// Number of defined verdicts.
pub const NF_MAX_VERDICT: u32 = NF_STOP;

// ---------------------------------------------------------------------------
// Hook points (per family)
// ---------------------------------------------------------------------------

pub const NF_INET_PRE_ROUTING: u32 = 0;
pub const NF_INET_LOCAL_IN: u32 = 1;
pub const NF_INET_FORWARD: u32 = 2;
pub const NF_INET_LOCAL_OUT: u32 = 3;
pub const NF_INET_POST_ROUTING: u32 = 4;
pub const NF_INET_NUMHOOKS: u32 = 5;

// ---------------------------------------------------------------------------
// Standard hook priorities (signed; lower runs first)
// ---------------------------------------------------------------------------

pub const NF_IP_PRI_FIRST: i32 = i32::MIN;
pub const NF_IP_PRI_RAW_BEFORE_DEFRAG: i32 = -450;
pub const NF_IP_PRI_CONNTRACK_DEFRAG: i32 = -400;
pub const NF_IP_PRI_RAW: i32 = -300;
pub const NF_IP_PRI_SELINUX_FIRST: i32 = -225;
pub const NF_IP_PRI_CONNTRACK: i32 = -200;
pub const NF_IP_PRI_MANGLE: i32 = -150;
pub const NF_IP_PRI_NAT_DST: i32 = -100;
pub const NF_IP_PRI_FILTER: i32 = 0;
pub const NF_IP_PRI_SECURITY: i32 = 50;
pub const NF_IP_PRI_NAT_SRC: i32 = 100;
pub const NF_IP_PRI_SELINUX_LAST: i32 = 225;
pub const NF_IP_PRI_CONNTRACK_HELPER: i32 = 300;
pub const NF_IP_PRI_LAST: i32 = i32::MAX;

// ---------------------------------------------------------------------------
// Netfilter protocol families (`nfproto_*`)
// ---------------------------------------------------------------------------

pub const NFPROTO_UNSPEC: u8 = 0;
pub const NFPROTO_INET: u8 = 1;
pub const NFPROTO_IPV4: u8 = 2;
pub const NFPROTO_ARP: u8 = 3;
pub const NFPROTO_NETDEV: u8 = 5;
pub const NFPROTO_BRIDGE: u8 = 7;
pub const NFPROTO_IPV6: u8 = 10;
pub const NFPROTO_DECNET: u8 = 12;
pub const NFPROTO_NUMPROTO: u8 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdicts_dense_0_to_5() {
        let v = [
            NF_DROP, NF_ACCEPT, NF_STOLEN, NF_QUEUE, NF_REPEAT, NF_STOP,
        ];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
        assert_eq!(NF_MAX_VERDICT, NF_STOP);
        // DROP being zero is load-bearing — many helpers default-return 0.
        assert_eq!(NF_DROP, 0);
    }

    #[test]
    fn test_hooks_dense_0_to_4_and_count_correct() {
        let h = [
            NF_INET_PRE_ROUTING,
            NF_INET_LOCAL_IN,
            NF_INET_FORWARD,
            NF_INET_LOCAL_OUT,
            NF_INET_POST_ROUTING,
        ];
        for (i, &v) in h.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(NF_INET_NUMHOOKS, h.len() as u32);
    }

    #[test]
    fn test_priorities_strictly_monotone() {
        let p = [
            NF_IP_PRI_FIRST,
            NF_IP_PRI_RAW_BEFORE_DEFRAG,
            NF_IP_PRI_CONNTRACK_DEFRAG,
            NF_IP_PRI_RAW,
            NF_IP_PRI_SELINUX_FIRST,
            NF_IP_PRI_CONNTRACK,
            NF_IP_PRI_MANGLE,
            NF_IP_PRI_NAT_DST,
            NF_IP_PRI_FILTER,
            NF_IP_PRI_SECURITY,
            NF_IP_PRI_NAT_SRC,
            NF_IP_PRI_SELINUX_LAST,
            NF_IP_PRI_CONNTRACK_HELPER,
            NF_IP_PRI_LAST,
        ];
        for w in p.windows(2) {
            assert!(w[0] < w[1]);
        }
        // FILTER is the documented "zero" anchor.
        assert_eq!(NF_IP_PRI_FILTER, 0);
    }

    #[test]
    fn test_nfproto_values_distinct_and_in_range() {
        let f = [
            NFPROTO_UNSPEC,
            NFPROTO_INET,
            NFPROTO_IPV4,
            NFPROTO_ARP,
            NFPROTO_NETDEV,
            NFPROTO_BRIDGE,
            NFPROTO_IPV6,
            NFPROTO_DECNET,
        ];
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
            // All families fit in a u8.
            assert!(f[i] < NFPROTO_NUMPROTO);
        }
    }
}
