//! `<linux/rtnetlink.h>` — Routing netlink message type constants.
//!
//! rtnetlink is the netlink family for managing the kernel's routing
//! tables, network interfaces, addresses, and neighbors. These
//! constants define the message types for CRUD operations on
//! routing objects.

// ---------------------------------------------------------------------------
// rtnetlink message types
// ---------------------------------------------------------------------------

/// New link (network interface).
pub const RTM_NEWLINK: u16 = 16;
/// Delete link.
pub const RTM_DELLINK: u16 = 17;
/// Get link info.
pub const RTM_GETLINK: u16 = 18;
/// Set link parameters.
pub const RTM_SETLINK: u16 = 19;

/// New address.
pub const RTM_NEWADDR: u16 = 20;
/// Delete address.
pub const RTM_DELADDR: u16 = 21;
/// Get address.
pub const RTM_GETADDR: u16 = 22;

/// New route.
pub const RTM_NEWROUTE: u16 = 24;
/// Delete route.
pub const RTM_DELROUTE: u16 = 25;
/// Get route.
pub const RTM_GETROUTE: u16 = 26;

/// New neighbor.
pub const RTM_NEWNEIGH: u16 = 28;
/// Delete neighbor.
pub const RTM_DELNEIGH: u16 = 29;
/// Get neighbor.
pub const RTM_GETNEIGH: u16 = 30;

/// New routing rule.
pub const RTM_NEWRULE: u16 = 32;
/// Delete routing rule.
pub const RTM_DELRULE: u16 = 33;
/// Get routing rule.
pub const RTM_GETRULE: u16 = 34;

/// New queuing discipline.
pub const RTM_NEWQDISC: u16 = 36;
/// Delete queuing discipline.
pub const RTM_DELQDISC: u16 = 37;
/// Get queuing discipline.
pub const RTM_GETQDISC: u16 = 38;

/// New traffic class.
pub const RTM_NEWTCLASS: u16 = 40;
/// Delete traffic class.
pub const RTM_DELTCLASS: u16 = 41;
/// Get traffic class.
pub const RTM_GETTCLASS: u16 = 42;

/// New traffic filter.
pub const RTM_NEWTFILTER: u16 = 44;
/// Delete traffic filter.
pub const RTM_DELTFILTER: u16 = 45;
/// Get traffic filter.
pub const RTM_GETTFILTER: u16 = 46;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_messages() {
        assert_eq!(RTM_NEWLINK, 16);
        assert_eq!(RTM_DELLINK, 17);
        assert_eq!(RTM_GETLINK, 18);
        assert_eq!(RTM_SETLINK, 19);
    }

    #[test]
    fn test_addr_messages() {
        assert_eq!(RTM_NEWADDR, 20);
        assert_eq!(RTM_DELADDR, 21);
        assert_eq!(RTM_GETADDR, 22);
    }

    #[test]
    fn test_route_messages() {
        assert_eq!(RTM_NEWROUTE, 24);
        assert_eq!(RTM_DELROUTE, 25);
        assert_eq!(RTM_GETROUTE, 26);
    }

    #[test]
    fn test_all_types_distinct() {
        let types = [
            RTM_NEWLINK, RTM_DELLINK, RTM_GETLINK, RTM_SETLINK,
            RTM_NEWADDR, RTM_DELADDR, RTM_GETADDR,
            RTM_NEWROUTE, RTM_DELROUTE, RTM_GETROUTE,
            RTM_NEWNEIGH, RTM_DELNEIGH, RTM_GETNEIGH,
            RTM_NEWRULE, RTM_DELRULE, RTM_GETRULE,
            RTM_NEWQDISC, RTM_DELQDISC, RTM_GETQDISC,
            RTM_NEWTCLASS, RTM_DELTCLASS, RTM_GETTCLASS,
            RTM_NEWTFILTER, RTM_DELTFILTER, RTM_GETTFILTER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_message_groups_aligned() {
        // Each group of 4 starts on a multiple of 4 boundary
        assert_eq!(RTM_NEWLINK % 4, 0);
        assert_eq!(RTM_NEWADDR % 4, 0);
        assert_eq!(RTM_NEWROUTE % 4, 0);
        assert_eq!(RTM_NEWNEIGH % 4, 0);
    }
}
