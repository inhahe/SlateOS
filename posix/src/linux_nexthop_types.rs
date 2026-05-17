//! `<linux/nexthop.h>` — Nexthop object netlink constants.
//!
//! Nexthop objects (Linux 5.3+) decouple next-hop definitions from
//! routes. Multiple routes can reference the same nexthop or nexthop
//! group, enabling efficient ECMP (Equal-Cost Multi-Path) and
//! resilient hashing. Changes to a nexthop (link failure, weight
//! adjustment) automatically affect all referencing routes without
//! updating each route individually. Used by FRR, BIRD, and other
//! routing daemons for scalable route management.

// ---------------------------------------------------------------------------
// Nexthop netlink commands (RTM_*)
// ---------------------------------------------------------------------------

/// New nexthop.
pub const RTM_NEWNEXTHOP: u32 = 104;
/// Delete nexthop.
pub const RTM_DELNEXTHOP: u32 = 105;
/// Get nexthop.
pub const RTM_GETNEXTHOP: u32 = 106;
/// New nexthop bucket (resilient group).
pub const RTM_NEWNEXTHOPBUCKET: u32 = 116;
/// Delete nexthop bucket.
pub const RTM_DELNEXTHOPBUCKET: u32 = 117;
/// Get nexthop bucket.
pub const RTM_GETNEXTHOPBUCKET: u32 = 118;

// ---------------------------------------------------------------------------
// Nexthop attributes (NHA_*)
// ---------------------------------------------------------------------------

/// Nexthop ID.
pub const NHA_ID: u32 = 1;
/// Nexthop group (list of weighted members).
pub const NHA_GROUP: u32 = 2;
/// Group type (multipath, resilient, etc.).
pub const NHA_GROUP_TYPE: u32 = 3;
/// Blackhole nexthop (drop packets).
pub const NHA_BLACKHOLE: u32 = 4;
/// Output interface index.
pub const NHA_OIF: u32 = 5;
/// Gateway address (IPv4/IPv6).
pub const NHA_GATEWAY: u32 = 6;
/// Encapsulation type (for MPLS, VXLAN, etc.).
pub const NHA_ENCAP_TYPE: u32 = 7;
/// Encapsulation data.
pub const NHA_ENCAP: u32 = 8;
/// Nexthop groups list.
pub const NHA_GROUPS: u32 = 9;
/// Master device (VRF).
pub const NHA_MASTER: u32 = 10;
/// FDB nexthop (for bridge).
pub const NHA_FDB: u32 = 11;
/// Resilient nexthop group data.
pub const NHA_RES_GROUP: u32 = 12;
/// Resilient bucket.
pub const NHA_RES_BUCKET: u32 = 13;
/// Hardware stats.
pub const NHA_HW_STATS: u32 = 14;

// ---------------------------------------------------------------------------
// Nexthop group types
// ---------------------------------------------------------------------------

/// Multipath group (hash-based ECMP).
pub const NEXTHOP_GRP_TYPE_MPATH: u32 = 0;
/// Resilient group (consistent hashing, minimal disruption on changes).
pub const NEXTHOP_GRP_TYPE_RES: u32 = 1;

// ---------------------------------------------------------------------------
// Nexthop flags (NHF_*)
// ---------------------------------------------------------------------------

/// Nexthop is for FIB (routing table).
pub const NHF_FIB: u32 = 0;

// ---------------------------------------------------------------------------
// Resilient group attributes (NHA_RES_GROUP_*)
// ---------------------------------------------------------------------------

/// Number of hash buckets.
pub const NHA_RES_GROUP_BUCKETS: u32 = 1;
/// Idle timer (seconds).
pub const NHA_RES_GROUP_IDLE_TIMER: u32 = 2;
/// Unbalanced timer (seconds).
pub const NHA_RES_GROUP_UNBALANCED_TIMER: u32 = 3;
/// Unbalanced time remaining.
pub const NHA_RES_GROUP_UNBALANCED_TIME: u32 = 4;

// ---------------------------------------------------------------------------
// Resilient bucket attributes (NHA_RES_BUCKET_*)
// ---------------------------------------------------------------------------

/// Bucket index.
pub const NHA_RES_BUCKET_INDEX: u32 = 1;
/// Bucket idle time.
pub const NHA_RES_BUCKET_IDLE_TIME: u32 = 2;
/// Bucket nexthop ID.
pub const NHA_RES_BUCKET_NH_ID: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtm_commands_distinct() {
        let cmds = [
            RTM_NEWNEXTHOP, RTM_DELNEXTHOP, RTM_GETNEXTHOP,
            RTM_NEWNEXTHOPBUCKET, RTM_DELNEXTHOPBUCKET, RTM_GETNEXTHOPBUCKET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_nha_attrs_distinct() {
        let attrs = [
            NHA_ID, NHA_GROUP, NHA_GROUP_TYPE, NHA_BLACKHOLE,
            NHA_OIF, NHA_GATEWAY, NHA_ENCAP_TYPE, NHA_ENCAP,
            NHA_GROUPS, NHA_MASTER, NHA_FDB,
            NHA_RES_GROUP, NHA_RES_BUCKET, NHA_HW_STATS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_group_types_distinct() {
        assert_ne!(NEXTHOP_GRP_TYPE_MPATH, NEXTHOP_GRP_TYPE_RES);
    }

    #[test]
    fn test_res_group_attrs_distinct() {
        let attrs = [
            NHA_RES_GROUP_BUCKETS, NHA_RES_GROUP_IDLE_TIMER,
            NHA_RES_GROUP_UNBALANCED_TIMER, NHA_RES_GROUP_UNBALANCED_TIME,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_res_bucket_attrs_distinct() {
        let attrs = [
            NHA_RES_BUCKET_INDEX, NHA_RES_BUCKET_IDLE_TIME,
            NHA_RES_BUCKET_NH_ID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
