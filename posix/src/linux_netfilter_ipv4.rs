//! `<linux/netfilter_ipv4.h>` — IPv4-specific netfilter constants.
//!
//! Re-exports common netfilter constants and adds IPv4-specific
//! hook names and priority values.

// ---------------------------------------------------------------------------
// Re-exports from linux_netfilter
// ---------------------------------------------------------------------------

pub use crate::linux_netfilter::NF_ACCEPT;
pub use crate::linux_netfilter::NF_DROP;
pub use crate::linux_netfilter::NF_QUEUE;
pub use crate::linux_netfilter::NF_REPEAT;
pub use crate::linux_netfilter::NF_STOLEN;
pub use crate::linux_netfilter::NF_STOP;
pub use crate::linux_netfilter::NFPROTO_IPV4;

// ---------------------------------------------------------------------------
// IPv4 hook names (aliases)
// ---------------------------------------------------------------------------

/// Pre-routing hook (same as NF_INET_PRE_ROUTING).
pub const NF_IP_PRE_ROUTING: u32 = 0;
/// Local input hook.
pub const NF_IP_LOCAL_IN: u32 = 1;
/// Forwarding hook.
pub const NF_IP_FORWARD: u32 = 2;
/// Local output hook.
pub const NF_IP_LOCAL_OUT: u32 = 3;
/// Post-routing hook.
pub const NF_IP_POST_ROUTING: u32 = 4;
/// Number of hooks.
pub const NF_IP_NUMHOOKS: u32 = 5;

// ---------------------------------------------------------------------------
// Hook priorities
// ---------------------------------------------------------------------------

/// First priority.
pub const NF_IP_PRI_FIRST: i32 = i32::MIN;
/// Connection tracking (in).
pub const NF_IP_PRI_CONNTRACK_DEFRAG: i32 = -400;
/// Raw table.
pub const NF_IP_PRI_RAW: i32 = -300;
/// SELinux first.
pub const NF_IP_PRI_SELINUX_FIRST: i32 = -225;
/// Connection tracking.
pub const NF_IP_PRI_CONNTRACK: i32 = -200;
/// Mangle table.
pub const NF_IP_PRI_MANGLE: i32 = -150;
/// NAT (destination).
pub const NF_IP_PRI_NAT_DST: i32 = -100;
/// Filter table.
pub const NF_IP_PRI_FILTER: i32 = 0;
/// Security table.
pub const NF_IP_PRI_SECURITY: i32 = 50;
/// NAT (source).
pub const NF_IP_PRI_NAT_SRC: i32 = 100;
/// SELinux last.
pub const NF_IP_PRI_SELINUX_LAST: i32 = 225;
/// Connection tracking (helper).
pub const NF_IP_PRI_CONNTRACK_HELPER: i32 = 300;
/// Connection tracking (confirm).
pub const NF_IP_PRI_CONNTRACK_CONFIRM: i32 = i32::MAX;
/// Last priority.
pub const NF_IP_PRI_LAST: i32 = i32::MAX;

// ---------------------------------------------------------------------------
// Socket options
// ---------------------------------------------------------------------------

/// Base for iptables socket options.
pub const SO_ORIGINAL_DST: i32 = 80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_names_match_inet() {
        assert_eq!(
            NF_IP_PRE_ROUTING,
            crate::linux_netfilter::NF_INET_PRE_ROUTING
        );
        assert_eq!(NF_IP_LOCAL_IN, crate::linux_netfilter::NF_INET_LOCAL_IN);
        assert_eq!(NF_IP_FORWARD, crate::linux_netfilter::NF_INET_FORWARD);
        assert_eq!(NF_IP_LOCAL_OUT, crate::linux_netfilter::NF_INET_LOCAL_OUT);
        assert_eq!(
            NF_IP_POST_ROUTING,
            crate::linux_netfilter::NF_INET_POST_ROUTING
        );
    }

    #[test]
    fn test_priorities_ordered() {
        assert!(NF_IP_PRI_FIRST < NF_IP_PRI_CONNTRACK_DEFRAG);
        assert!(NF_IP_PRI_CONNTRACK_DEFRAG < NF_IP_PRI_RAW);
        assert!(NF_IP_PRI_RAW < NF_IP_PRI_CONNTRACK);
        assert!(NF_IP_PRI_CONNTRACK < NF_IP_PRI_MANGLE);
        assert!(NF_IP_PRI_MANGLE < NF_IP_PRI_NAT_DST);
        assert!(NF_IP_PRI_NAT_DST < NF_IP_PRI_FILTER);
        assert!(NF_IP_PRI_FILTER < NF_IP_PRI_SECURITY);
        assert!(NF_IP_PRI_SECURITY < NF_IP_PRI_NAT_SRC);
        assert!(NF_IP_PRI_NAT_SRC < NF_IP_PRI_CONNTRACK_CONFIRM);
    }

    #[test]
    fn test_verdicts() {
        assert_eq!(NF_DROP, 0);
        assert_eq!(NF_ACCEPT, 1);
    }

    #[test]
    fn test_nfproto_ipv4() {
        assert_eq!(NFPROTO_IPV4, 2);
    }
}
