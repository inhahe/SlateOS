//! `<linux/igmp.h>` — IGMP (Internet Group Management Protocol) constants.
//!
//! IGMP manages multicast group membership on IPv4 networks. Hosts
//! use IGMP to join/leave multicast groups; routers use it to discover
//! which groups have local members. IGMPv3 adds source-specific
//! multicast (SSM) — hosts can specify which sources they want to
//! receive from. The kernel's multicast subsystem tracks group
//! memberships per interface and sends/processes IGMP messages.

// ---------------------------------------------------------------------------
// IGMP message types
// ---------------------------------------------------------------------------

/// Membership Query (router → hosts).
pub const IGMP_MEMBERSHIP_QUERY: u32 = 0x11;
/// IGMPv1 Membership Report (host → router).
pub const IGMPV1_MEMBERSHIP_REPORT: u32 = 0x12;
/// IGMPv2 Membership Report.
pub const IGMPV2_MEMBERSHIP_REPORT: u32 = 0x16;
/// IGMPv2 Leave Group.
pub const IGMP_LEAVE_GROUP: u32 = 0x17;
/// IGMPv3 Membership Report.
pub const IGMPV3_MEMBERSHIP_REPORT: u32 = 0x22;

// ---------------------------------------------------------------------------
// IGMPv3 group record types
// ---------------------------------------------------------------------------

/// Mode is Include (listen to listed sources).
pub const IGMPV3_MODE_IS_INCLUDE: u32 = 1;
/// Mode is Exclude (listen to all except listed).
pub const IGMPV3_MODE_IS_EXCLUDE: u32 = 2;
/// Change to Include mode.
pub const IGMPV3_CHANGE_TO_INCLUDE: u32 = 3;
/// Change to Exclude mode.
pub const IGMPV3_CHANGE_TO_EXCLUDE: u32 = 4;
/// Allow new sources.
pub const IGMPV3_ALLOW_NEW_SOURCES: u32 = 5;
/// Block old sources.
pub const IGMPV3_BLOCK_OLD_SOURCES: u32 = 6;

// ---------------------------------------------------------------------------
// IGMP timers and limits
// ---------------------------------------------------------------------------

/// Robustness variable (default, how many reports before timeout).
pub const IGMP_ROBUSTNESS_DEFAULT: u32 = 2;
/// Query interval (seconds, default).
pub const IGMP_QUERY_INTERVAL: u32 = 125;
/// Max response time for query (tenths of seconds).
pub const IGMP_MAX_RESPONSE_TIME: u32 = 100;
/// Last member query interval (tenths of seconds).
pub const IGMP_LAST_MEMBER_QUERY_INTERVAL: u32 = 10;
/// Maximum number of multicast groups per socket.
pub const IGMP_MAX_MEMBERSHIPS: u32 = 20;

// ---------------------------------------------------------------------------
// Multicast filter modes
// ---------------------------------------------------------------------------

/// Include mode (receive from listed sources only).
pub const MCAST_INCLUDE: u32 = 0;
/// Exclude mode (receive from all except listed sources).
pub const MCAST_EXCLUDE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_types_distinct() {
        let types = [
            IGMP_MEMBERSHIP_QUERY,
            IGMPV1_MEMBERSHIP_REPORT,
            IGMPV2_MEMBERSHIP_REPORT,
            IGMP_LEAVE_GROUP,
            IGMPV3_MEMBERSHIP_REPORT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_record_types_distinct() {
        let records = [
            IGMPV3_MODE_IS_INCLUDE,
            IGMPV3_MODE_IS_EXCLUDE,
            IGMPV3_CHANGE_TO_INCLUDE,
            IGMPV3_CHANGE_TO_EXCLUDE,
            IGMPV3_ALLOW_NEW_SOURCES,
            IGMPV3_BLOCK_OLD_SOURCES,
        ];
        for i in 0..records.len() {
            for j in (i + 1)..records.len() {
                assert_ne!(records[i], records[j]);
            }
        }
    }

    #[test]
    fn test_timers_positive() {
        assert!(IGMP_ROBUSTNESS_DEFAULT > 0);
        assert!(IGMP_QUERY_INTERVAL > 0);
        assert!(IGMP_MAX_RESPONSE_TIME > 0);
        assert!(IGMP_LAST_MEMBER_QUERY_INTERVAL > 0);
    }

    #[test]
    fn test_filter_modes_distinct() {
        assert_ne!(MCAST_INCLUDE, MCAST_EXCLUDE);
    }
}
