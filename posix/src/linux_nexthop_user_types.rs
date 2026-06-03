//! `<linux/nexthop.h>` — netlink nexthop ABI.
//!
//! Nexthop objects let multiple routes share a single forwarding
//! decision — added in Linux 5.3 to reduce route-update overhead in
//! BGP-heavy deployments. FRR, GoBGP, and `iproute2`'s `ip nexthop`
//! use the `RTM_*NEXTHOP` messages to create, update, and delete
//! these objects.

// ---------------------------------------------------------------------------
// Nexthop netlink attributes
// ---------------------------------------------------------------------------

pub const NHA_UNSPEC: u32 = 0;
pub const NHA_ID: u32 = 1;
pub const NHA_GROUP: u32 = 2;
pub const NHA_GROUP_TYPE: u32 = 3;
pub const NHA_BLACKHOLE: u32 = 4;
pub const NHA_OIF: u32 = 5;
pub const NHA_GATEWAY: u32 = 6;
pub const NHA_ENCAP_TYPE: u32 = 7;
pub const NHA_ENCAP: u32 = 8;
pub const NHA_GROUPS: u32 = 9;
pub const NHA_MASTER: u32 = 10;
pub const NHA_FDB: u32 = 11;
pub const NHA_RES_GROUP: u32 = 12;
pub const NHA_RES_BUCKET: u32 = 13;
pub const NHA_OP_FLAGS: u32 = 14;
pub const NHA_GROUP_STATS: u32 = 15;
pub const NHA_HW_STATS_ENABLE: u32 = 16;
pub const NHA_HW_STATS_USED: u32 = 17;

// ---------------------------------------------------------------------------
// `NHA_GROUP_TYPE` values
// ---------------------------------------------------------------------------

pub const NEXTHOP_GRP_TYPE_MPATH: u32 = 0;
pub const NEXTHOP_GRP_TYPE_RES: u32 = 1;

// ---------------------------------------------------------------------------
// `NHA_RES_GROUP` sub-attributes
// ---------------------------------------------------------------------------

pub const NHA_RES_GROUP_UNSPEC: u32 = 0;
pub const NHA_RES_GROUP_PAD: u32 = 1;
pub const NHA_RES_GROUP_BUCKETS: u32 = 2;
pub const NHA_RES_GROUP_IDLE_TIMER: u32 = 3;
pub const NHA_RES_GROUP_UNBALANCED_TIMER: u32 = 4;
pub const NHA_RES_GROUP_UNBALANCED_TIME: u32 = 5;

// ---------------------------------------------------------------------------
// `NHA_RES_BUCKET` sub-attributes
// ---------------------------------------------------------------------------

pub const NHA_RES_BUCKET_UNSPEC: u32 = 0;
pub const NHA_RES_BUCKET_INDEX: u32 = 1;
pub const NHA_RES_BUCKET_IDLE_TIME: u32 = 2;
pub const NHA_RES_BUCKET_NH_ID: u32 = 3;

// ---------------------------------------------------------------------------
// Dump filter flags (`NHA_OP_FLAGS`)
// ---------------------------------------------------------------------------

pub const NHA_OP_FLAG_DUMP_STATS: u32 = 1 << 0;
pub const NHA_OP_FLAG_DUMP_HW_STATS: u32 = 1 << 1;
pub const NHA_OP_FLAG_RESP_GRP_RESVD_0: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nh_attributes_dense_0_to_17() {
        let a = [
            NHA_UNSPEC,
            NHA_ID,
            NHA_GROUP,
            NHA_GROUP_TYPE,
            NHA_BLACKHOLE,
            NHA_OIF,
            NHA_GATEWAY,
            NHA_ENCAP_TYPE,
            NHA_ENCAP,
            NHA_GROUPS,
            NHA_MASTER,
            NHA_FDB,
            NHA_RES_GROUP,
            NHA_RES_BUCKET,
            NHA_OP_FLAGS,
            NHA_GROUP_STATS,
            NHA_HW_STATS_ENABLE,
            NHA_HW_STATS_USED,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_group_types_dense() {
        assert_eq!(NEXTHOP_GRP_TYPE_MPATH, 0);
        assert_eq!(NEXTHOP_GRP_TYPE_RES, 1);
    }

    #[test]
    fn test_res_group_subattrs_dense_0_to_5() {
        let s = [
            NHA_RES_GROUP_UNSPEC,
            NHA_RES_GROUP_PAD,
            NHA_RES_GROUP_BUCKETS,
            NHA_RES_GROUP_IDLE_TIMER,
            NHA_RES_GROUP_UNBALANCED_TIMER,
            NHA_RES_GROUP_UNBALANCED_TIME,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_res_bucket_subattrs_dense_0_to_3() {
        let s = [
            NHA_RES_BUCKET_UNSPEC,
            NHA_RES_BUCKET_INDEX,
            NHA_RES_BUCKET_IDLE_TIME,
            NHA_RES_BUCKET_NH_ID,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_op_flag_layout() {
        // Dense low bits for the public flags...
        assert!(NHA_OP_FLAG_DUMP_STATS.is_power_of_two());
        assert!(NHA_OP_FLAG_DUMP_HW_STATS.is_power_of_two());
        assert_eq!(NHA_OP_FLAG_DUMP_STATS | NHA_OP_FLAG_DUMP_HW_STATS, 0x3);
        // ...and the high bit reserved for kernel-internal use.
        assert_eq!(NHA_OP_FLAG_RESP_GRP_RESVD_0, 1 << 31);
    }
}
