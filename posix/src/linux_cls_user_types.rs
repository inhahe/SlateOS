//! `<linux/pkt_cls.h>` — Generic classifier attributes (TCA_*).
//!
//! Every tc classifier shares a common attribute envelope: kind name,
//! parent, prio, options blob, statistics, etc. These are dispatched
//! through the netlink `TC_NEWTFILTER`/`TC_DELTFILTER`/`TC_GETTFILTER`
//! commands.

// ---------------------------------------------------------------------------
// Top-level TCA_* attribute IDs
// ---------------------------------------------------------------------------

pub const TCA_UNSPEC: u32 = 0;
pub const TCA_KIND: u32 = 1;
pub const TCA_OPTIONS: u32 = 2;
pub const TCA_STATS: u32 = 3;
pub const TCA_XSTATS: u32 = 4;
pub const TCA_RATE: u32 = 5;
pub const TCA_FCNT: u32 = 6;
pub const TCA_STATS2: u32 = 7;
pub const TCA_STAB: u32 = 8;
pub const TCA_PAD: u32 = 9;
pub const TCA_DUMP_INVISIBLE: u32 = 10;
pub const TCA_CHAIN: u32 = 11;
pub const TCA_HW_OFFLOAD: u32 = 12;
pub const TCA_INGRESS_BLOCK: u32 = 13;
pub const TCA_EGRESS_BLOCK: u32 = 14;
pub const TCA_DUMP_FLAGS: u32 = 15;
pub const TCA_EXT_WARN_MSG: u32 = 16;

pub const TCA_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// Netlink message types for tc
// ---------------------------------------------------------------------------

pub const RTM_NEWTFILTER: u32 = 44;
pub const RTM_DELTFILTER: u32 = 45;
pub const RTM_GETTFILTER: u32 = 46;
pub const RTM_NEWQDISC: u32 = 36;
pub const RTM_DELQDISC: u32 = 37;
pub const RTM_GETQDISC: u32 = 38;
pub const RTM_NEWTCLASS: u32 = 40;
pub const RTM_DELTCLASS: u32 = 41;
pub const RTM_GETTCLASS: u32 = 42;

// ---------------------------------------------------------------------------
// TCA_KIND name length (max characters including NUL)
// ---------------------------------------------------------------------------

pub const TCA_KIND_NAME_MAX: usize = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tca_attrs_dense_0_to_16() {
        let a = [
            TCA_UNSPEC,
            TCA_KIND,
            TCA_OPTIONS,
            TCA_STATS,
            TCA_XSTATS,
            TCA_RATE,
            TCA_FCNT,
            TCA_STATS2,
            TCA_STAB,
            TCA_PAD,
            TCA_DUMP_INVISIBLE,
            TCA_CHAIN,
            TCA_HW_OFFLOAD,
            TCA_INGRESS_BLOCK,
            TCA_EGRESS_BLOCK,
            TCA_DUMP_FLAGS,
            TCA_EXT_WARN_MSG,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_tca_max_matches_last_attr() {
        assert_eq!(TCA_MAX, TCA_EXT_WARN_MSG);
    }

    #[test]
    fn test_rtm_commands_pattern() {
        // Each tc object kind (qdisc/tclass/tfilter) has NEW/DEL/GET
        // forming a dense 3-tuple.
        assert_eq!(RTM_DELQDISC - RTM_NEWQDISC, 1);
        assert_eq!(RTM_GETQDISC - RTM_NEWQDISC, 2);
        assert_eq!(RTM_DELTCLASS - RTM_NEWTCLASS, 1);
        assert_eq!(RTM_GETTCLASS - RTM_NEWTCLASS, 2);
        assert_eq!(RTM_DELTFILTER - RTM_NEWTFILTER, 1);
        assert_eq!(RTM_GETTFILTER - RTM_NEWTFILTER, 2);
    }

    #[test]
    fn test_rtm_object_kinds_4_apart() {
        // qdisc(36)..tclass(40)..tfilter(44) — each kind block is 4 numbers.
        assert_eq!(RTM_NEWTCLASS - RTM_NEWQDISC, 4);
        assert_eq!(RTM_NEWTFILTER - RTM_NEWTCLASS, 4);
    }

    #[test]
    fn test_kind_name_max_is_16() {
        // TC_NAME_MAX in kernel = 16. Names like "htb", "fq_codel" fit.
        assert_eq!(TCA_KIND_NAME_MAX, 16);
    }
}
