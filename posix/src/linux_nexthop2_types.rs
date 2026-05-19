//! `<linux/nexthop.h>` — Additional nexthop constants.
//!
//! Supplementary nexthop routing constants covering attribute types,
//! group types, and nexthop flags.

// ---------------------------------------------------------------------------
// Nexthop attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const NHA_UNSPEC: u32 = 0;
/// Nexthop ID.
pub const NHA_ID: u32 = 1;
/// Nexthop group.
pub const NHA_GROUP: u32 = 2;
/// Group type.
pub const NHA_GROUP_TYPE: u32 = 3;
/// Blackhole.
pub const NHA_BLACKHOLE: u32 = 4;
/// Output interface.
pub const NHA_OIF: u32 = 5;
/// Gateway address.
pub const NHA_GATEWAY: u32 = 6;
/// Encap type.
pub const NHA_ENCAP_TYPE: u32 = 7;
/// Encap.
pub const NHA_ENCAP: u32 = 8;
/// Groups.
pub const NHA_GROUPS: u32 = 9;
/// Master.
pub const NHA_MASTER: u32 = 10;
/// FDB.
pub const NHA_FDB: u32 = 11;
/// Resilient group attributes.
pub const NHA_RES_GROUP: u32 = 12;
/// Resilient bucket attributes.
pub const NHA_RES_BUCKET: u32 = 13;

// ---------------------------------------------------------------------------
// Nexthop group types
// ---------------------------------------------------------------------------

/// Multipath group.
pub const NEXTHOP_GRP_TYPE_MPATH: u32 = 0;
/// Resilient group.
pub const NEXTHOP_GRP_TYPE_RES: u32 = 1;

// ---------------------------------------------------------------------------
// Nexthop flags
// ---------------------------------------------------------------------------

/// Onlink nexthop.
pub const RTNH_F_ONLINK: u32 = 0x04;
/// Dead nexthop.
pub const RTNH_F_DEAD: u32 = 0x01;
/// Pervasive nexthop.
pub const RTNH_F_PERVASIVE: u32 = 0x02;
/// Offload nexthop.
pub const RTNH_F_OFFLOAD: u32 = 0x08;
/// Linkdown nexthop.
pub const RTNH_F_LINKDOWN: u32 = 0x10;
/// Unresolved nexthop.
pub const RTNH_F_UNRESOLVED: u32 = 0x20;
/// Trap nexthop.
pub const RTNH_F_TRAP: u32 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            NHA_UNSPEC, NHA_ID, NHA_GROUP, NHA_GROUP_TYPE,
            NHA_BLACKHOLE, NHA_OIF, NHA_GATEWAY, NHA_ENCAP_TYPE,
            NHA_ENCAP, NHA_GROUPS, NHA_MASTER, NHA_FDB,
            NHA_RES_GROUP, NHA_RES_BUCKET,
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
    fn test_flags_no_overlap() {
        let flags = [
            RTNH_F_DEAD, RTNH_F_PERVASIVE, RTNH_F_ONLINK,
            RTNH_F_OFFLOAD, RTNH_F_LINKDOWN, RTNH_F_UNRESOLVED,
            RTNH_F_TRAP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            RTNH_F_DEAD, RTNH_F_PERVASIVE, RTNH_F_ONLINK,
            RTNH_F_OFFLOAD, RTNH_F_LINKDOWN, RTNH_F_UNRESOLVED,
            RTNH_F_TRAP,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not power of two", flag);
        }
    }
}
