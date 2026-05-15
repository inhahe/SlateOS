//! `<net/route.h>` — routing table definitions.
//!
//! Defines routing table structures and constants used with
//! `AF_ROUTE` sockets and `SIOCADDRT`/`SIOCDELRT` ioctls.

// ---------------------------------------------------------------------------
// Routing table entry structure
// ---------------------------------------------------------------------------

/// Routing table entry (for `SIOCADDRT` / `SIOCDELRT` ioctls).
///
/// This is the Linux-compatible `struct rtentry` layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtEntry {
    /// Destination address.
    pub rt_dst: crate::socket::Sockaddr,
    /// Gateway address.
    pub rt_gateway: crate::socket::Sockaddr,
    /// Network mask.
    pub rt_genmask: crate::socket::Sockaddr,
    /// Flags (RTF_*).
    pub rt_flags: u16,
    /// Metric (cost).
    pub rt_metric: i16,
    /// Device name pointer (null-terminated).
    pub rt_dev: *mut u8,
    /// MTU (maximum transmission unit).
    pub rt_mtu: u32,
    /// Window size (TCP).
    pub rt_window: u32,
    /// Initial round-trip time (TCP).
    pub rt_irtt: u16,
}

/// Routing message header (for `AF_ROUTE` sockets, BSD-style).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtMsghdr {
    /// Length of this message, including header.
    pub rtm_msglen: u16,
    /// Message version (RTM_VERSION).
    pub rtm_version: u8,
    /// Message type (RTM_ADD, etc.).
    pub rtm_type: u8,
    /// Bitmask of addresses present.
    pub rtm_addrs: i32,
    /// Flags (RTF_*).
    pub rtm_flags: i32,
    /// Index of associated interface.
    pub rtm_index: u16,
    /// Padding.
    _rtm_pad: u16,
    /// Process ID of sender.
    pub rtm_pid: i32,
    /// Sequence number for sender to identify response.
    pub rtm_seq: i32,
    /// Error number from routing daemon.
    pub rtm_errno: i32,
    /// Metrics included.
    pub rtm_use: i32,
    /// Which metrics are valid.
    pub rtm_inits: u32,
}

// ---------------------------------------------------------------------------
// Routing flags (RTF_*)
// ---------------------------------------------------------------------------

/// Route is usable.
pub const RTF_UP: u16 = 0x0001;

/// Destination is a gateway.
pub const RTF_GATEWAY: u16 = 0x0002;

/// Host route (not network).
pub const RTF_HOST: u16 = 0x0004;

/// Created dynamically (by redirect).
pub const RTF_DYNAMIC: u16 = 0x0010;

/// Modified dynamically (by redirect).
pub const RTF_MODIFIED: u16 = 0x0020;

/// Route is up and usable.
pub const RTF_DONE: u16 = 0x0040;

/// Use network mask for route.
pub const RTF_MASK: u16 = 0x0080;

/// Reject route.
pub const RTF_REJECT: u16 = 0x0008;

/// Route has a static metric.
pub const RTF_STATIC: u16 = 0x0800;

/// Route was created by redirect.
pub const RTF_XRESOLVE: u16 = 0x0200;

/// Blackhole route (discard silently).
pub const RTF_BLACKHOLE: u16 = 0x1000;

/// Local route (interface route).
pub const RTF_LOCAL: u16 = 0x2000;

/// Broadcast route.
pub const RTF_BROADCAST: u16 = 0x4000;

/// Multicast route.
pub const RTF_MULTICAST: u16 = 0x8000u16;

// ---------------------------------------------------------------------------
// Routing message types (rtm_type)
// ---------------------------------------------------------------------------

/// Add a route.
pub const RTM_ADD: u8 = 1;

/// Delete a route.
pub const RTM_DELETE: u8 = 2;

/// Change a route.
pub const RTM_CHANGE: u8 = 3;

/// Route lookup.
pub const RTM_GET: u8 = 4;

/// Route entry lost.
pub const RTM_LOSING: u8 = 5;

/// Redirect received.
pub const RTM_REDIRECT: u8 = 6;

/// Told to use different route.
pub const RTM_MISS: u8 = 7;

/// Resolve needed (unused in most implementations).
pub const RTM_RESOLVE: u8 = 11;

/// Interface is going down.
pub const RTM_IFINFO: u8 = 14;

/// New address for interface.
pub const RTM_NEWADDR: u8 = 12;

/// Address removed from interface.
pub const RTM_DELADDR: u8 = 13;

/// Routing message version.
pub const RTM_VERSION: u8 = 5;

// ---------------------------------------------------------------------------
// Address bitmask values (rtm_addrs)
// ---------------------------------------------------------------------------

/// Destination sockaddr present.
pub const RTA_DST: i32 = 0x01;

/// Gateway sockaddr present.
pub const RTA_GATEWAY: i32 = 0x02;

/// Netmask sockaddr present.
pub const RTA_NETMASK: i32 = 0x04;

/// Cloning mask sockaddr present.
pub const RTA_GENMASK: i32 = 0x08;

/// Interface name sockaddr present.
pub const RTA_IFP: i32 = 0x10;

/// Interface address sockaddr present.
pub const RTA_IFA: i32 = 0x20;

/// Author of redirect.
pub const RTA_AUTHOR: i32 = 0x40;

/// Broadcast address sockaddr present.
pub const RTA_BRD: i32 = 0x80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Struct sizes
    // -----------------------------------------------------------------------

    #[test]
    fn test_rtentry_nonzero() {
        assert!(core::mem::size_of::<RtEntry>() > 0);
    }

    #[test]
    fn test_rtmsghdr_nonzero() {
        assert!(core::mem::size_of::<RtMsghdr>() > 0);
    }

    // -----------------------------------------------------------------------
    // Routing flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_rtf_up() {
        assert_eq!(RTF_UP, 0x0001);
    }

    #[test]
    fn test_rtf_gateway() {
        assert_eq!(RTF_GATEWAY, 0x0002);
    }

    #[test]
    fn test_rtf_host() {
        assert_eq!(RTF_HOST, 0x0004);
    }

    #[test]
    fn test_rtf_flags_bitmask() {
        // The first few flags should be powers of two (combinable).
        let basic_flags = [RTF_UP, RTF_GATEWAY, RTF_HOST, RTF_REJECT];
        for &f in &basic_flags {
            assert_ne!(f, 0);
        }
    }

    #[test]
    fn test_rtf_flags_distinct() {
        let flags = [
            RTF_UP, RTF_GATEWAY, RTF_HOST, RTF_REJECT, RTF_DYNAMIC,
            RTF_MODIFIED, RTF_DONE, RTF_MASK, RTF_STATIC,
            RTF_XRESOLVE, RTF_BLACKHOLE, RTF_LOCAL,
            RTF_BROADCAST, RTF_MULTICAST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(
                    flags[i], flags[j],
                    "RTF flags must be distinct"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Message types
    // -----------------------------------------------------------------------

    #[test]
    fn test_rtm_add() {
        assert_eq!(RTM_ADD, 1);
    }

    #[test]
    fn test_rtm_delete() {
        assert_eq!(RTM_DELETE, 2);
    }

    #[test]
    fn test_rtm_version() {
        assert_eq!(RTM_VERSION, 5);
    }

    #[test]
    fn test_rtm_types_distinct() {
        let types = [
            RTM_ADD, RTM_DELETE, RTM_CHANGE, RTM_GET, RTM_LOSING,
            RTM_REDIRECT, RTM_MISS, RTM_RESOLVE, RTM_IFINFO,
            RTM_NEWADDR, RTM_DELADDR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i], types[j],
                    "RTM types must be distinct"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Address mask values
    // -----------------------------------------------------------------------

    #[test]
    fn test_rta_bitmask_values() {
        assert_eq!(RTA_DST, 0x01);
        assert_eq!(RTA_GATEWAY, 0x02);
        assert_eq!(RTA_NETMASK, 0x04);
        assert_eq!(RTA_GENMASK, 0x08);
        assert_eq!(RTA_IFP, 0x10);
        assert_eq!(RTA_IFA, 0x20);
        assert_eq!(RTA_AUTHOR, 0x40);
        assert_eq!(RTA_BRD, 0x80);
    }

    #[test]
    fn test_rta_all_combined() {
        let all = RTA_DST | RTA_GATEWAY | RTA_NETMASK | RTA_GENMASK
                | RTA_IFP | RTA_IFA | RTA_AUTHOR | RTA_BRD;
        assert_eq!(all, 0xFF);
    }

    #[test]
    fn test_rta_powers_of_two() {
        let rtas = [
            RTA_DST, RTA_GATEWAY, RTA_NETMASK, RTA_GENMASK,
            RTA_IFP, RTA_IFA, RTA_AUTHOR, RTA_BRD,
        ];
        for &r in &rtas {
            assert!(r > 0);
            assert_eq!(r & (r - 1), 0, "RTA 0x{r:X} is not a power of two");
        }
    }

    // -----------------------------------------------------------------------
    // RtEntry field access
    // -----------------------------------------------------------------------

    #[test]
    fn test_rtentry_default_init() {
        // Verify we can zero-init an RtEntry.
        let entry = RtEntry {
            rt_dst: crate::socket::Sockaddr {
                sa_family: 0,
                sa_data: [0; 14],
            },
            rt_gateway: crate::socket::Sockaddr {
                sa_family: 0,
                sa_data: [0; 14],
            },
            rt_genmask: crate::socket::Sockaddr {
                sa_family: 0,
                sa_data: [0; 14],
            },
            rt_flags: RTF_UP | RTF_GATEWAY,
            rt_metric: 0,
            rt_dev: core::ptr::null_mut(),
            rt_mtu: 1500,
            rt_window: 0,
            rt_irtt: 0,
        };
        assert_eq!(entry.rt_flags, RTF_UP | RTF_GATEWAY);
        assert_eq!(entry.rt_mtu, 1500);
    }
}
