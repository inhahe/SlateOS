//! `<linux/rtnetlink.h>` — FIB (Forwarding Information Base) constants.
//!
//! FIB is the kernel's routing table implementation.  These
//! constants define route types, scopes, protocols, and table
//! IDs used by the routing subsystem.

// ---------------------------------------------------------------------------
// Route types (rtm_type in struct rtmsg)
// ---------------------------------------------------------------------------

/// Unspecified route type.
pub const RTN_UNSPEC: u32 = 0;
/// Unicast route (normal gateway route).
pub const RTN_UNICAST: u32 = 1;
/// Local interface route.
pub const RTN_LOCAL: u32 = 2;
/// Broadcast route.
pub const RTN_BROADCAST: u32 = 3;
/// Anycast route.
pub const RTN_ANYCAST: u32 = 4;
/// Multicast route.
pub const RTN_MULTICAST: u32 = 5;
/// Blackhole route (drop silently).
pub const RTN_BLACKHOLE: u32 = 6;
/// Unreachable (ICMP unreachable).
pub const RTN_UNREACHABLE: u32 = 7;
/// Prohibit (ICMP prohibited).
pub const RTN_PROHIBIT: u32 = 8;
/// Throw (continue route lookup in another table).
pub const RTN_THROW: u32 = 9;
/// NAT route (deprecated).
pub const RTN_NAT: u32 = 10;
/// External resolver.
pub const RTN_XRESOLVE: u32 = 11;

// ---------------------------------------------------------------------------
// Route scopes (rtm_scope)
// ---------------------------------------------------------------------------

/// Global scope (gateway route).
pub const RT_SCOPE_UNIVERSE: u32 = 0;
/// Site scope.
pub const RT_SCOPE_SITE: u32 = 200;
/// Link scope (directly attached).
pub const RT_SCOPE_LINK: u32 = 253;
/// Host scope (local interface).
pub const RT_SCOPE_HOST: u32 = 254;
/// Nowhere scope (invalid).
pub const RT_SCOPE_NOWHERE: u32 = 255;

// ---------------------------------------------------------------------------
// Route protocols (rtm_protocol — who installed the route)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const RTPROT_UNSPEC: u32 = 0;
/// ICMP redirect.
pub const RTPROT_REDIRECT: u32 = 1;
/// Kernel-generated route.
pub const RTPROT_KERNEL: u32 = 2;
/// Boot-time route.
pub const RTPROT_BOOT: u32 = 3;
/// Static route (administrator).
pub const RTPROT_STATIC: u32 = 4;
/// Zebra/Quagga.
pub const RTPROT_ZEBRA: u32 = 11;
/// BIRD.
pub const RTPROT_BIRD: u32 = 12;
/// DHCP client.
pub const RTPROT_DHCP: u32 = 16;

// ---------------------------------------------------------------------------
// Routing table IDs
// ---------------------------------------------------------------------------

/// Unspecified table.
pub const RT_TABLE_UNSPEC: u32 = 0;
/// Default table.
pub const RT_TABLE_DEFAULT: u32 = 253;
/// Main table.
pub const RT_TABLE_MAIN: u32 = 254;
/// Local table.
pub const RT_TABLE_LOCAL: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_types_distinct() {
        let types = [
            RTN_UNSPEC,
            RTN_UNICAST,
            RTN_LOCAL,
            RTN_BROADCAST,
            RTN_ANYCAST,
            RTN_MULTICAST,
            RTN_BLACKHOLE,
            RTN_UNREACHABLE,
            RTN_PROHIBIT,
            RTN_THROW,
            RTN_NAT,
            RTN_XRESOLVE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(RTN_UNSPEC, 0);
    }

    #[test]
    fn test_scopes_distinct() {
        let scopes = [
            RT_SCOPE_UNIVERSE,
            RT_SCOPE_SITE,
            RT_SCOPE_LINK,
            RT_SCOPE_HOST,
            RT_SCOPE_NOWHERE,
        ];
        for i in 0..scopes.len() {
            for j in (i + 1)..scopes.len() {
                assert_ne!(scopes[i], scopes[j]);
            }
        }
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            RTPROT_UNSPEC,
            RTPROT_REDIRECT,
            RTPROT_KERNEL,
            RTPROT_BOOT,
            RTPROT_STATIC,
            RTPROT_ZEBRA,
            RTPROT_BIRD,
            RTPROT_DHCP,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_tables_distinct() {
        let tables = [
            RT_TABLE_UNSPEC,
            RT_TABLE_DEFAULT,
            RT_TABLE_MAIN,
            RT_TABLE_LOCAL,
        ];
        for i in 0..tables.len() {
            for j in (i + 1)..tables.len() {
                assert_ne!(tables[i], tables[j]);
            }
        }
    }

    #[test]
    fn test_main_table_is_254() {
        assert_eq!(RT_TABLE_MAIN, 254);
    }
}
