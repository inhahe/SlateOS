//! `<linux/igmp.h>` — Internet Group Management Protocol (IPv4 multicast).
//!
//! IGMP carries multicast-group membership for IPv4: `pimd`, mDNS
//! responders (Avahi), every kernel multicast router, and the
//! `setsockopt(IP_ADD_MEMBERSHIP)` path emit IGMPv1/v2/v3 packets
//! using the type codes and group addresses defined here.

// ---------------------------------------------------------------------------
// IGMP message types (struct igmphdr.type)
// ---------------------------------------------------------------------------

/// IGMPv1/v2 Membership Query.
pub const IGMP_HOST_MEMBERSHIP_QUERY: u8 = 0x11;
/// IGMPv1 Membership Report.
pub const IGMP_HOST_MEMBERSHIP_REPORT: u8 = 0x12;
/// DVMRP.
pub const IGMP_DVMRP: u8 = 0x13;
/// PIMv1.
pub const IGMP_PIM: u8 = 0x14;
/// Cisco IGMP-trace.
pub const IGMP_TRACE: u8 = 0x15;
/// IGMPv2 Membership Report.
pub const IGMP_HOST_NEW_MEMBERSHIP_REPORT: u8 = 0x16;
/// IGMPv2 Leave Group.
pub const IGMP_HOST_LEAVE_MESSAGE: u8 = 0x17;
/// MTrace request.
pub const IGMP_MTRACE_RESP: u8 = 0x1E;
pub const IGMP_MTRACE: u8 = 0x1F;
/// IGMPv3 Membership Report.
pub const IGMPV3_HOST_MEMBERSHIP_REPORT: u8 = 0x22;

// ---------------------------------------------------------------------------
// Well-known multicast addresses (host byte order)
// ---------------------------------------------------------------------------

/// 224.0.0.1 — all-systems group.
pub const IGMP_ALL_HOSTS: u32 = 0xE000_0001;
/// 224.0.0.2 — all-routers.
pub const IGMP_ALL_ROUTER: u32 = 0xE000_0002;
/// 224.0.0.22 — IGMPv3 reports destination.
pub const IGMPV3_ALL_MCR: u32 = 0xE000_0016;
/// 224.0.0.13 — PIM all-routers.
pub const IGMP_PIM_ROUTERS: u32 = 0xE000_000D;
/// 224.0.0.0/4 — multicast block start.
pub const IGMP_MIN_LOCAL_GROUP: u32 = 0xE000_0000;
/// 224.0.0.255 — end of local-scope block.
pub const IGMP_LOCAL_GROUP_END: u32 = 0xE000_00FF;

// ---------------------------------------------------------------------------
// IGMPv3 record types (struct igmpv3_grec.grec_type)
// ---------------------------------------------------------------------------

pub const IGMPV3_MODE_IS_INCLUDE: u8 = 1;
pub const IGMPV3_MODE_IS_EXCLUDE: u8 = 2;
pub const IGMPV3_CHANGE_TO_INCLUDE: u8 = 3;
pub const IGMPV3_CHANGE_TO_EXCLUDE: u8 = 4;
pub const IGMPV3_ALLOW_NEW_SOURCES: u8 = 5;
pub const IGMPV3_BLOCK_OLD_SOURCES: u8 = 6;

// ---------------------------------------------------------------------------
// Timer defaults (jiffies / centisecond units depending on field)
// ---------------------------------------------------------------------------

/// Default Max Response Time for v1/v2 queries (1/10 sec).
pub const IGMP_MAX_HOST_REPORT_DELAY: u32 = 10;
/// Default Query Interval (seconds).
pub const IGMP_QUERY_INTERVAL: u32 = 125;
/// Default Robustness Variable.
pub const IGMP_ROBUSTNESS_VAR: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_types_distinct() {
        let m = [
            IGMP_HOST_MEMBERSHIP_QUERY,
            IGMP_HOST_MEMBERSHIP_REPORT,
            IGMP_DVMRP,
            IGMP_PIM,
            IGMP_TRACE,
            IGMP_HOST_NEW_MEMBERSHIP_REPORT,
            IGMP_HOST_LEAVE_MESSAGE,
            IGMP_MTRACE_RESP,
            IGMP_MTRACE,
            IGMPV3_HOST_MEMBERSHIP_REPORT,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_well_known_addresses_in_multicast_block() {
        // 224.0.0.0/4 — high nibble = 0xE.
        for &addr in &[
            IGMP_ALL_HOSTS,
            IGMP_ALL_ROUTER,
            IGMPV3_ALL_MCR,
            IGMP_PIM_ROUTERS,
            IGMP_MIN_LOCAL_GROUP,
            IGMP_LOCAL_GROUP_END,
        ] {
            assert_eq!(addr >> 28, 0xE);
        }
        // 224.0.0.0..224.0.0.255 — local-scope subnet.
        assert_eq!(IGMP_LOCAL_GROUP_END - IGMP_MIN_LOCAL_GROUP, 0xFF);
    }

    #[test]
    fn test_igmpv3_record_types_dense_1_to_6() {
        let r = [
            IGMPV3_MODE_IS_INCLUDE,
            IGMPV3_MODE_IS_EXCLUDE,
            IGMPV3_CHANGE_TO_INCLUDE,
            IGMPV3_CHANGE_TO_EXCLUDE,
            IGMPV3_ALLOW_NEW_SOURCES,
            IGMPV3_BLOCK_OLD_SOURCES,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_defaults_match_rfc() {
        // RFC 3376 §8.2: default robustness = 2, query interval = 125 s.
        assert_eq!(IGMP_QUERY_INTERVAL, 125);
        assert_eq!(IGMP_ROBUSTNESS_VAR, 2);
    }
}
