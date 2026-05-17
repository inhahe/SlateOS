//! `<linux/net_namespace.h>` — Network namespace constants.
//!
//! Network namespaces provide isolated network stacks: each namespace
//! has its own network interfaces, IP addresses, routing tables,
//! firewall rules, /proc/net, and socket port spaces. Processes in
//! different network namespaces cannot communicate via networking
//! unless explicitly connected (veth pairs, bridges). This is the
//! foundation for container networking: each container gets its own
//! network namespace with its own loopback, eth0, routing, etc.

// ---------------------------------------------------------------------------
// Network namespace clone/unshare flag
// ---------------------------------------------------------------------------

/// Clone flag for creating a new network namespace.
pub const CLONE_NEWNET: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// Network namespace states
// ---------------------------------------------------------------------------

/// Namespace is being set up (devices being created).
pub const NETNS_STATE_SETUP: u32 = 0;
/// Namespace is active and operational.
pub const NETNS_STATE_ACTIVE: u32 = 1;
/// Namespace is being torn down (devices being removed).
pub const NETNS_STATE_DYING: u32 = 2;

// ---------------------------------------------------------------------------
// Network namespace default interface indices
// ---------------------------------------------------------------------------

/// Loopback interface index (always 1 in every netns).
pub const NETNS_LOOPBACK_IFINDEX: u32 = 1;

// ---------------------------------------------------------------------------
// Network namespace subsystem IDs (for per-netns subsystem state)
// ---------------------------------------------------------------------------

/// IPv4 subsystem.
pub const NETNS_SUBSYS_IPV4: u32 = 0;
/// IPv6 subsystem.
pub const NETNS_SUBSYS_IPV6: u32 = 1;
/// Netfilter (firewall) subsystem.
pub const NETNS_SUBSYS_NFNL: u32 = 2;
/// Unix domain sockets subsystem.
pub const NETNS_SUBSYS_UNIX: u32 = 3;
/// Packet (raw) sockets subsystem.
pub const NETNS_SUBSYS_PACKET: u32 = 4;
/// Netlink subsystem.
pub const NETNS_SUBSYS_NETLINK: u32 = 5;
/// Network device subsystem.
pub const NETNS_SUBSYS_NETDEV: u32 = 6;

// ---------------------------------------------------------------------------
// Network namespace flags
// ---------------------------------------------------------------------------

/// Namespace has loopback device configured.
pub const NETNS_FLAG_LOOPBACK_UP: u32 = 0x0001;
/// Namespace allows forwarding between interfaces.
pub const NETNS_FLAG_IP_FORWARD: u32 = 0x0002;
/// Namespace is the initial (host) network namespace.
pub const NETNS_FLAG_INITIAL: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_flag() {
        assert!(CLONE_NEWNET.is_power_of_two());
        assert_ne!(CLONE_NEWNET, 0);
    }

    #[test]
    fn test_states_distinct() {
        let states = [NETNS_STATE_SETUP, NETNS_STATE_ACTIVE, NETNS_STATE_DYING];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_subsys_ids_distinct() {
        let ids = [
            NETNS_SUBSYS_IPV4, NETNS_SUBSYS_IPV6, NETNS_SUBSYS_NFNL,
            NETNS_SUBSYS_UNIX, NETNS_SUBSYS_PACKET,
            NETNS_SUBSYS_NETLINK, NETNS_SUBSYS_NETDEV,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            NETNS_FLAG_LOOPBACK_UP, NETNS_FLAG_IP_FORWARD,
            NETNS_FLAG_INITIAL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_loopback_always_first() {
        assert_eq!(NETNS_LOOPBACK_IFINDEX, 1);
    }
}
