//! `<linux/netconf.h>` — Additional netconf constants.
//!
//! Supplementary netconf constants covering attribute types
//! and forwarding modes.

// ---------------------------------------------------------------------------
// Netconf attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const NETCONFA_UNSPEC: u32 = 0;
/// Interface index.
pub const NETCONFA_IFINDEX: u32 = 1;
/// Forwarding.
pub const NETCONFA_FORWARDING: u32 = 2;
/// RP filter.
pub const NETCONFA_RP_FILTER: u32 = 3;
/// MC forwarding.
pub const NETCONFA_MC_FORWARDING: u32 = 4;
/// Proxy NDP.
pub const NETCONFA_PROXY_NEIGH: u32 = 5;
/// Ignore routes with linkdown.
pub const NETCONFA_IGNORE_ROUTES_WITH_LINKDOWN: u32 = 6;
/// Input.
pub const NETCONFA_INPUT: u32 = 7;
/// BC forwarding.
pub const NETCONFA_BC_FORWARDING: u32 = 8;

// ---------------------------------------------------------------------------
// Netconf interface indices
// ---------------------------------------------------------------------------

/// All interfaces.
pub const NETCONFA_IFINDEX_ALL: i32 = -1;
/// Default.
pub const NETCONFA_IFINDEX_DEFAULT: i32 = -2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            NETCONFA_UNSPEC,
            NETCONFA_IFINDEX,
            NETCONFA_FORWARDING,
            NETCONFA_RP_FILTER,
            NETCONFA_MC_FORWARDING,
            NETCONFA_PROXY_NEIGH,
            NETCONFA_IGNORE_ROUTES_WITH_LINKDOWN,
            NETCONFA_INPUT,
            NETCONFA_BC_FORWARDING,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_special_ifindex_distinct() {
        assert_ne!(NETCONFA_IFINDEX_ALL, NETCONFA_IFINDEX_DEFAULT);
    }

    #[test]
    fn test_special_ifindex_negative() {
        assert!(NETCONFA_IFINDEX_ALL < 0);
        assert!(NETCONFA_IFINDEX_DEFAULT < 0);
    }
}
