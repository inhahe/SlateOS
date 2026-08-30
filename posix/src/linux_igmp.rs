//! `<linux/igmp.h>` — Internet Group Management Protocol.
//!
//! IGMP is used for IPv4 multicast group membership management.
//! Hosts use it to report multicast group membership to routers.

// ---------------------------------------------------------------------------
// IGMP message types
// ---------------------------------------------------------------------------

/// Membership query (v1/v2/v3).
pub const IGMP_HOST_MEMBERSHIP_QUERY: u8 = 0x11;
/// Membership report (IGMPv1).
pub const IGMP_HOST_MEMBERSHIP_REPORT: u8 = 0x12;
/// Leave group (IGMPv2).
pub const IGMP_HOST_LEAVE_MESSAGE: u8 = 0x17;
/// Membership report (IGMPv2).
pub const IGMPV2_HOST_MEMBERSHIP_REPORT: u8 = 0x16;
/// Membership report (IGMPv3).
pub const IGMPV3_HOST_MEMBERSHIP_REPORT: u8 = 0x22;
/// Distance Vector Multicast Routing Protocol.
pub const IGMP_DVMRP: u8 = 0x13;
/// PIM (Protocol Independent Multicast).
pub const IGMP_PIM: u8 = 0x14;
/// Multicast Traceroute response.
pub const IGMP_TRACE: u8 = 0x15;
/// MRINFO request.
pub const IGMP_MRINFO: u8 = 0x1F;

// ---------------------------------------------------------------------------
// IGMP header
// ---------------------------------------------------------------------------

/// IGMP packet header (8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Igmphdr {
    /// Message type.
    pub type_: u8,
    /// Max response time (in 1/10 seconds, IGMPv2+).
    pub code: u8,
    /// Checksum.
    pub csum: u16,
    /// Multicast group address.
    pub group: u32,
}

impl Igmphdr {
    /// Create a zeroed IGMP header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// IGMPv3 group record types
// ---------------------------------------------------------------------------

/// Mode is Include.
pub const IGMPV3_MODE_IS_INCLUDE: u8 = 1;
/// Mode is Exclude.
pub const IGMPV3_MODE_IS_EXCLUDE: u8 = 2;
/// Change to Include mode.
pub const IGMPV3_CHANGE_TO_INCLUDE: u8 = 3;
/// Change to Exclude mode.
pub const IGMPV3_CHANGE_TO_EXCLUDE: u8 = 4;
/// Allow new sources.
pub const IGMPV3_ALLOW_NEW_SOURCES: u8 = 5;
/// Block old sources.
pub const IGMPV3_BLOCK_OLD_SOURCES: u8 = 6;

// ---------------------------------------------------------------------------
// IGMP constants
// ---------------------------------------------------------------------------

/// All-hosts multicast (224.0.0.1).
pub const IGMP_ALL_HOSTS: u32 = 0xE0000001_u32.to_be();
/// All-routers multicast (224.0.0.2).
pub const IGMP_ALL_ROUTER: u32 = 0xE0000002_u32.to_be();
/// IGMPv3 report destination (224.0.0.22).
pub const IGMPV3_ALL_MCR: u32 = 0xE0000016_u32.to_be();

/// Maximum response time for query (10 seconds in 1/10s units).
pub const IGMP_MAX_HOST_REPORT_DELAY: u8 = 100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_igmphdr_size() {
        assert_eq!(core::mem::size_of::<Igmphdr>(), 8);
    }

    #[test]
    fn test_igmphdr_zeroed() {
        let hdr = Igmphdr::zeroed();
        assert_eq!(hdr.type_, 0);
        assert_eq!(hdr.code, 0);
        assert_eq!(hdr.csum, 0);
        assert_eq!(hdr.group, 0);
    }

    #[test]
    fn test_message_types_distinct() {
        let types = [
            IGMP_HOST_MEMBERSHIP_QUERY,
            IGMP_HOST_MEMBERSHIP_REPORT,
            IGMPV2_HOST_MEMBERSHIP_REPORT,
            IGMP_HOST_LEAVE_MESSAGE,
            IGMPV3_HOST_MEMBERSHIP_REPORT,
            IGMP_DVMRP,
            IGMP_PIM,
            IGMP_TRACE,
            IGMP_MRINFO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_igmpv3_record_types_sequential() {
        assert_eq!(IGMPV3_MODE_IS_INCLUDE, 1);
        assert_eq!(IGMPV3_MODE_IS_EXCLUDE, 2);
        assert_eq!(IGMPV3_CHANGE_TO_INCLUDE, 3);
        assert_eq!(IGMPV3_CHANGE_TO_EXCLUDE, 4);
        assert_eq!(IGMPV3_ALLOW_NEW_SOURCES, 5);
        assert_eq!(IGMPV3_BLOCK_OLD_SOURCES, 6);
    }

    #[test]
    fn test_max_report_delay() {
        assert_eq!(IGMP_MAX_HOST_REPORT_DELAY, 100);
    }
}
