//! `<linux/netfilter/nf_conntrack_common.h>` — connection tracking states.
//!
//! Netfilter's conntrack subsystem tracks every L3/L4 flow as a "tuple"
//! plus a state machine. The IP_CT_* state values appear in ctmark/
//! ctstate match modules and via /proc/net/nf_conntrack.

// ---------------------------------------------------------------------------
// Connection states (IP_CT_*)
// ---------------------------------------------------------------------------

pub const IP_CT_ESTABLISHED: u32 = 0;
pub const IP_CT_RELATED: u32 = 1;
pub const IP_CT_NEW: u32 = 2;
pub const IP_CT_IS_REPLY: u32 = 3;

/// Mask for the per-direction reply bit.
pub const IP_CT_DIR_MASK: u32 = 0x80;
/// Combined "established + reply" state.
pub const IP_CT_ESTABLISHED_REPLY: u32 = IP_CT_ESTABLISHED + IP_CT_IS_REPLY;
/// Combined "related + reply" state.
pub const IP_CT_RELATED_REPLY: u32 = IP_CT_RELATED + IP_CT_IS_REPLY;

// ---------------------------------------------------------------------------
// Conntrack status flags (low bits of `status`)
// ---------------------------------------------------------------------------

pub const IPS_EXPECTED: u32 = 1 << 0;
pub const IPS_SEEN_REPLY: u32 = 1 << 1;
pub const IPS_ASSURED: u32 = 1 << 2;
pub const IPS_CONFIRMED: u32 = 1 << 3;
pub const IPS_SRC_NAT: u32 = 1 << 4;
pub const IPS_DST_NAT: u32 = 1 << 5;
pub const IPS_NAT_MASK: u32 = IPS_SRC_NAT | IPS_DST_NAT;
pub const IPS_SEQ_ADJUST: u32 = 1 << 6;
pub const IPS_SRC_NAT_DONE: u32 = 1 << 7;
pub const IPS_DST_NAT_DONE: u32 = 1 << 8;
pub const IPS_DYING: u32 = 1 << 9;
pub const IPS_FIXED_TIMEOUT: u32 = 1 << 10;
pub const IPS_TEMPLATE: u32 = 1 << 11;
pub const IPS_UNTRACKED: u32 = 1 << 12;
pub const IPS_HELPER: u32 = 1 << 13;
pub const IPS_OFFLOAD: u32 = 1 << 14;
pub const IPS_HW_OFFLOAD: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// /proc/net file paths
// ---------------------------------------------------------------------------

pub const PROC_NET_NF_CONNTRACK: &str = "/proc/net/nf_conntrack";
pub const PROC_NET_NF_CONNTRACK_EXPECT: &str = "/proc/net/nf_conntrack_expect";
pub const PROC_SYS_NF_CONNTRACK_MAX: &str = "/proc/sys/net/netfilter/nf_conntrack_max";

// ---------------------------------------------------------------------------
// Default conntrack table size hint (kernel scales by RAM)
// ---------------------------------------------------------------------------

/// 65 536 is the historic kernel default for low-memory systems.
pub const NF_CONNTRACK_DEFAULT_MAX: u32 = 65_536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ip_ct_states_dense_0_to_3() {
        let s = [IP_CT_ESTABLISHED, IP_CT_RELATED, IP_CT_NEW, IP_CT_IS_REPLY];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_combined_states_derive_correctly() {
        // ESTABLISHED_REPLY = 0 + 3 = 3.
        assert_eq!(IP_CT_ESTABLISHED_REPLY, 3);
        // RELATED_REPLY = 1 + 3 = 4.
        assert_eq!(IP_CT_RELATED_REPLY, 4);
    }

    #[test]
    fn test_status_flags_distinct_single_bit() {
        let f = [
            IPS_EXPECTED,
            IPS_SEEN_REPLY,
            IPS_ASSURED,
            IPS_CONFIRMED,
            IPS_SRC_NAT,
            IPS_DST_NAT,
            IPS_SEQ_ADJUST,
            IPS_SRC_NAT_DONE,
            IPS_DST_NAT_DONE,
            IPS_DYING,
            IPS_FIXED_TIMEOUT,
            IPS_TEMPLATE,
            IPS_UNTRACKED,
            IPS_HELPER,
            IPS_OFFLOAD,
            IPS_HW_OFFLOAD,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_nat_mask_is_src_or_dst() {
        assert_eq!(IPS_NAT_MASK, IPS_SRC_NAT | IPS_DST_NAT);
        assert_eq!(IPS_NAT_MASK.count_ones(), 2);
    }

    #[test]
    fn test_proc_paths_well_formed() {
        assert!(PROC_NET_NF_CONNTRACK.starts_with("/proc/net/"));
        assert!(PROC_SYS_NF_CONNTRACK_MAX.starts_with("/proc/sys/"));
        assert!(PROC_NET_NF_CONNTRACK_EXPECT.contains("expect"));
    }

    #[test]
    fn test_default_max_is_65k() {
        assert_eq!(NF_CONNTRACK_DEFAULT_MAX, 65_536);
        assert!(NF_CONNTRACK_DEFAULT_MAX.is_power_of_two());
    }
}
