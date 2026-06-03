//! `<linux/netfilter/nf_conntrack_common.h>` — conntrack ABI.
//!
//! Conntrack is the netfilter connection-tracking subsystem — the
//! engine behind stateful firewalls, NAT, and `conntrackd`. Every
//! tracked flow has a tuple, a status bitmask, and a state; the
//! constants below define the public ABI shared between iptables,
//! nftables, and `libnetfilter_conntrack`.

// ---------------------------------------------------------------------------
// Connection states (`enum ip_conntrack_info`)
// ---------------------------------------------------------------------------

pub const IP_CT_ESTABLISHED: u32 = 0;
pub const IP_CT_RELATED: u32 = 1;
pub const IP_CT_NEW: u32 = 2;
pub const IP_CT_IS_REPLY: u32 = 3;
pub const IP_CT_ESTABLISHED_REPLY: u32 = IP_CT_ESTABLISHED + IP_CT_IS_REPLY;
pub const IP_CT_RELATED_REPLY: u32 = IP_CT_RELATED + IP_CT_IS_REPLY;
pub const IP_CT_NUMBER: u32 = 4;

// ---------------------------------------------------------------------------
// Status flags (`enum ip_conntrack_status`)
// ---------------------------------------------------------------------------

pub const IPS_EXPECTED: u32 = 1 << 0;
pub const IPS_SEEN_REPLY: u32 = 1 << 1;
pub const IPS_ASSURED: u32 = 1 << 2;
pub const IPS_CONFIRMED: u32 = 1 << 3;
pub const IPS_SRC_NAT: u32 = 1 << 4;
pub const IPS_DST_NAT: u32 = 1 << 5;
pub const IPS_NAT_MASK: u32 = IPS_DST_NAT | IPS_SRC_NAT;
pub const IPS_SEQ_ADJUST: u32 = 1 << 6;
pub const IPS_SRC_NAT_DONE: u32 = 1 << 7;
pub const IPS_DST_NAT_DONE: u32 = 1 << 8;
pub const IPS_NAT_DONE_MASK: u32 = IPS_DST_NAT_DONE | IPS_SRC_NAT_DONE;
pub const IPS_DYING: u32 = 1 << 9;
pub const IPS_FIXED_TIMEOUT: u32 = 1 << 10;
pub const IPS_TEMPLATE: u32 = 1 << 11;
pub const IPS_UNTRACKED: u32 = 1 << 12;
pub const IPS_HELPER: u32 = 1 << 13;
pub const IPS_OFFLOAD: u32 = 1 << 14;
pub const IPS_HW_OFFLOAD: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Event-mask bits (`enum ip_conntrack_events`)
// ---------------------------------------------------------------------------

pub const IPCT_NEW: u32 = 0;
pub const IPCT_RELATED: u32 = 1;
pub const IPCT_DESTROY: u32 = 2;
pub const IPCT_REPLY: u32 = 3;
pub const IPCT_ASSURED: u32 = 4;
pub const IPCT_PROTOINFO: u32 = 5;
pub const IPCT_HELPER: u32 = 6;
pub const IPCT_MARK: u32 = 7;
pub const IPCT_SEQADJ: u32 = 8;
pub const IPCT_SECMARK: u32 = 9;
pub const IPCT_LABEL: u32 = 10;
pub const IPCT_SYNPROXY: u32 = 11;

// ---------------------------------------------------------------------------
// Expectation event types (`enum ip_conntrack_expect_events`)
// ---------------------------------------------------------------------------

pub const IPEXP_NEW: u32 = 0;
pub const IPEXP_DESTROY: u32 = 1;

// ---------------------------------------------------------------------------
// Sysctl interface paths
// ---------------------------------------------------------------------------

pub const SYSCTL_NF_CONNTRACK_MAX: &str = "/proc/sys/net/netfilter/nf_conntrack_max";
pub const SYSCTL_NF_CONNTRACK_COUNT: &str = "/proc/sys/net/netfilter/nf_conntrack_count";
pub const SYSCTL_NF_CONNTRACK_BUCKETS: &str = "/proc/sys/net/netfilter/nf_conntrack_buckets";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ct_info_states_dense() {
        // Forward-direction states are 0..3.
        let s = [IP_CT_ESTABLISHED, IP_CT_RELATED, IP_CT_NEW, IP_CT_IS_REPLY];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Reply variants are forward + IS_REPLY.
        assert_eq!(IP_CT_ESTABLISHED_REPLY, IP_CT_ESTABLISHED + IP_CT_IS_REPLY);
        assert_eq!(IP_CT_RELATED_REPLY, IP_CT_RELATED + IP_CT_IS_REPLY);
        // IP_CT_NUMBER counts the forward directions only.
        assert_eq!(IP_CT_NUMBER, 4);
    }

    #[test]
    fn test_status_bits_single_bit() {
        let s = [
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
        for v in s {
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_nat_mask_composites() {
        assert_eq!(IPS_NAT_MASK, IPS_SRC_NAT | IPS_DST_NAT);
        assert_eq!(IPS_NAT_DONE_MASK, IPS_SRC_NAT_DONE | IPS_DST_NAT_DONE);
        // Masks don't overlap.
        assert_eq!(IPS_NAT_MASK & IPS_NAT_DONE_MASK, 0);
    }

    #[test]
    fn test_event_bits_dense_0_to_11() {
        let e = [
            IPCT_NEW,
            IPCT_RELATED,
            IPCT_DESTROY,
            IPCT_REPLY,
            IPCT_ASSURED,
            IPCT_PROTOINFO,
            IPCT_HELPER,
            IPCT_MARK,
            IPCT_SEQADJ,
            IPCT_SECMARK,
            IPCT_LABEL,
            IPCT_SYNPROXY,
        ];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_expect_events_dense() {
        assert_eq!(IPEXP_NEW, 0);
        assert_eq!(IPEXP_DESTROY, 1);
    }

    #[test]
    fn test_sysctl_paths() {
        assert!(SYSCTL_NF_CONNTRACK_MAX.ends_with("nf_conntrack_max"));
        assert!(SYSCTL_NF_CONNTRACK_COUNT.ends_with("nf_conntrack_count"));
        assert!(SYSCTL_NF_CONNTRACK_BUCKETS.ends_with("nf_conntrack_buckets"));
    }
}
