//! Linux network namespace constants.
//!
//! Network namespaces isolate network stacks: each namespace has
//! its own interfaces, routing tables, firewall rules, sockets,
//! and /proc/net. Used by containers to provide independent
//! networking per tenant.

// ---------------------------------------------------------------------------
// Clone flags
// ---------------------------------------------------------------------------

/// Create new network namespace.
pub const CLONE_NEWNET: u64 = 0x40000000;

// ---------------------------------------------------------------------------
// /proc interface
// ---------------------------------------------------------------------------

/// Network namespace proc link.
pub const PROC_NS_NET: &str = "ns/net";
/// Network namespace runtime directory.
pub const NETNS_RUN_DIR: &str = "/var/run/netns";

// ---------------------------------------------------------------------------
// Default interfaces
// ---------------------------------------------------------------------------

/// Loopback interface name.
pub const NETNS_LO_IFNAME: &str = "lo";
/// Loopback interface index (always 1 in any netns).
pub const NETNS_LO_IFINDEX: u32 = 1;

// ---------------------------------------------------------------------------
// Network namespace sysctl paths
// ---------------------------------------------------------------------------

/// IPv4 forwarding sysctl.
pub const SYSCTL_IP_FORWARD: &str = "net.ipv4.ip_forward";
/// IPv6 forwarding sysctl.
pub const SYSCTL_IPV6_FORWARD: &str = "net.ipv6.conf.all.forwarding";
/// Default TTL sysctl.
pub const SYSCTL_IP_DEFAULT_TTL: &str = "net.ipv4.ip_default_ttl";

// ---------------------------------------------------------------------------
// Namespace management (iproute2 style)
// ---------------------------------------------------------------------------

/// Named network namespace file prefix.
pub const NETNS_PREFIX: &str = "netns/";

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum network namespace name length.
pub const NETNS_NAME_MAX: usize = 256;

/// Default IPv4 TTL for new namespaces.
pub const NETNS_DEFAULT_TTL: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_newnet() {
        assert_eq!(CLONE_NEWNET, 0x40000000);
        assert!((CLONE_NEWNET as u64).is_power_of_two());
    }

    #[test]
    fn test_clone_flags_distinct() {
        // Should not collide with other namespace clone flags
        let other_ns_flags: &[u64] = &[
            0x10000000, // CLONE_NEWUSER
            0x20000000, // CLONE_NEWPID
            0x00020000, // CLONE_NEWNS
        ];
        for flag in other_ns_flags {
            assert_ne!(CLONE_NEWNET, *flag);
        }
    }

    #[test]
    fn test_proc_path() {
        assert_eq!(PROC_NS_NET, "ns/net");
    }

    #[test]
    fn test_lo_interface() {
        assert_eq!(NETNS_LO_IFNAME, "lo");
        assert_eq!(NETNS_LO_IFINDEX, 1);
    }

    #[test]
    fn test_sysctl_paths_distinct() {
        let paths = [SYSCTL_IP_FORWARD, SYSCTL_IPV6_FORWARD, SYSCTL_IP_DEFAULT_TTL];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn test_name_max() {
        assert!(NETNS_NAME_MAX > 0);
    }

    #[test]
    fn test_default_ttl() {
        assert_eq!(NETNS_DEFAULT_TTL, 64);
    }
}
