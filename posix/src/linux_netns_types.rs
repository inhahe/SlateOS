//! `<linux/net_namespace.h>` — Network namespace constants.
//!
//! Network namespaces provide isolated network stacks — each has
//! its own interfaces, routing tables, firewall rules, and socket
//! bindings. They are the basis for containers' network isolation
//! and are managed via ip-netns, RTNetlink, or nsfd operations.

// ---------------------------------------------------------------------------
// Network namespace netlink operations (RTM_*)
// ---------------------------------------------------------------------------

/// Create a new network namespace (via CLONE_NEWNET).
pub const RTM_NEWNSID: u32 = 88;
/// Delete a network namespace ID mapping.
pub const RTM_DELNSID: u32 = 89;
/// Get network namespace ID mapping.
pub const RTM_GETNSID: u32 = 90;

// ---------------------------------------------------------------------------
// NETNSA attribute types (netlink attrs for nsid messages)
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const NETNSA_NONE: u32 = 0;
/// Network namespace ID (integer).
pub const NETNSA_NSID: u32 = 1;
/// PID of a process in the target namespace.
pub const NETNSA_PID: u32 = 2;
/// File descriptor of the target namespace.
pub const NETNSA_FD: u32 = 3;
/// Target network device ID.
pub const NETNSA_TARGET_NSID: u32 = 4;
/// Current namespace ID.
pub const NETNSA_CURRENT_NSID: u32 = 5;

// ---------------------------------------------------------------------------
// Network namespace special values
// ---------------------------------------------------------------------------

/// Unknown/unassigned namespace ID.
pub const NETNSA_NSID_NOT_ASSIGNED: i32 = -1;

// ---------------------------------------------------------------------------
// veth (virtual ethernet) link types for netns connectivity
// ---------------------------------------------------------------------------

/// VETH_INFO_PEER attribute (peer device in other netns).
pub const VETH_INFO_PEER: u32 = 1;

// ---------------------------------------------------------------------------
// Network namespace limits
// ---------------------------------------------------------------------------

/// Maximum number of network namespaces (soft limit, sysctl).
pub const NETNS_MAX_DEFAULT: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtm_operations_distinct() {
        assert_ne!(RTM_NEWNSID, RTM_DELNSID);
        assert_ne!(RTM_DELNSID, RTM_GETNSID);
        assert_ne!(RTM_NEWNSID, RTM_GETNSID);
    }

    #[test]
    fn test_netnsa_attrs_distinct() {
        let attrs = [
            NETNSA_NONE, NETNSA_NSID, NETNSA_PID,
            NETNSA_FD, NETNSA_TARGET_NSID, NETNSA_CURRENT_NSID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_not_assigned() {
        assert_eq!(NETNSA_NSID_NOT_ASSIGNED, -1);
    }

    #[test]
    fn test_netns_max() {
        assert!(NETNS_MAX_DEFAULT > 0);
    }
}
