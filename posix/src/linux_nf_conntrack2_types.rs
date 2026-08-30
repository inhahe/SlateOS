//! `<linux/netfilter/nf_conntrack_common.h>` — Additional conntrack constants.
//!
//! Supplementary conntrack constants covering connection status bits,
//! conntrack event types, and expectation flags.

// ---------------------------------------------------------------------------
// Connection status bits (IPS_*)
// ---------------------------------------------------------------------------

/// Connection has been seen in both directions.
pub const IPS_SEEN_REPLY: u32 = 1 << 1;
/// Connection is assured (seen reply).
pub const IPS_ASSURED: u32 = 1 << 2;
/// Connection is confirmed.
pub const IPS_CONFIRMED: u32 = 1 << 3;
/// Source NAT applied.
pub const IPS_SRC_NAT: u32 = 1 << 4;
/// Destination NAT applied.
pub const IPS_DST_NAT: u32 = 1 << 5;
/// NAT mask (src | dst).
pub const IPS_NAT_MASK: u32 = (1 << 4) | (1 << 5);
/// Sequence number adjustment needed.
pub const IPS_SEQ_ADJUST: u32 = 1 << 6;
/// Source NAT done.
pub const IPS_SRC_NAT_DONE: u32 = 1 << 7;
/// Destination NAT done.
pub const IPS_DST_NAT_DONE: u32 = 1 << 8;
/// NAT done mask.
pub const IPS_NAT_DONE_MASK: u32 = (1 << 7) | (1 << 8);
/// Connection is dying.
pub const IPS_DYING: u32 = 1 << 9;
/// Connection has a fixed timeout.
pub const IPS_FIXED_TIMEOUT: u32 = 1 << 10;
/// Template connection.
pub const IPS_TEMPLATE: u32 = 1 << 11;
/// Untracked connection.
pub const IPS_UNTRACKED: u32 = 1 << 12;
/// Helper assigned.
pub const IPS_HELPER: u32 = 1 << 13;
/// Offloaded connection.
pub const IPS_OFFLOAD: u32 = 1 << 14;
/// HW offloaded connection.
pub const IPS_HW_OFFLOAD: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Conntrack event types (IPCT_*)
// ---------------------------------------------------------------------------

/// New connection.
pub const IPCT_NEW: u32 = 0;
/// Related connection.
pub const IPCT_RELATED: u32 = 1;
/// Destroy connection.
pub const IPCT_DESTROY: u32 = 2;
/// Reply direction seen.
pub const IPCT_REPLY: u32 = 3;
/// Assured state reached.
pub const IPCT_ASSURED: u32 = 4;
/// Protoinfo updated.
pub const IPCT_PROTOINFO: u32 = 5;
/// Helper info updated.
pub const IPCT_HELPER: u32 = 6;
/// Mark changed.
pub const IPCT_MARK: u32 = 7;
/// Sequence adjust changed.
pub const IPCT_SEQADJ: u32 = 8;
/// NAT sequence adjust.
pub const IPCT_NATSEQADJ: u32 = 9;
/// Secmark changed.
pub const IPCT_SECMARK: u32 = 10;
/// Label changed.
pub const IPCT_LABEL: u32 = 11;

// ---------------------------------------------------------------------------
// Expectation flags (NF_CT_EXPECT_*)
// ---------------------------------------------------------------------------

/// Permanent expectation.
pub const NF_CT_EXPECT_PERMANENT: u32 = 1 << 0;
/// Inactive expectation.
pub const NF_CT_EXPECT_INACTIVE: u32 = 1 << 1;
/// Userspace expectation.
pub const NF_CT_EXPECT_USERSPACE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bits_power_of_two() {
        let bits = [
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
        for b in &bits {
            assert!(b.is_power_of_two(), "0x{:04x} not power of two", b);
        }
    }

    #[test]
    fn test_nat_mask() {
        assert_eq!(IPS_NAT_MASK, IPS_SRC_NAT | IPS_DST_NAT);
    }

    #[test]
    fn test_nat_done_mask() {
        assert_eq!(IPS_NAT_DONE_MASK, IPS_SRC_NAT_DONE | IPS_DST_NAT_DONE);
    }

    #[test]
    fn test_event_types_distinct() {
        let events = [
            IPCT_NEW,
            IPCT_RELATED,
            IPCT_DESTROY,
            IPCT_REPLY,
            IPCT_ASSURED,
            IPCT_PROTOINFO,
            IPCT_HELPER,
            IPCT_MARK,
            IPCT_SEQADJ,
            IPCT_NATSEQADJ,
            IPCT_SECMARK,
            IPCT_LABEL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_expect_flags_power_of_two() {
        let flags = [
            NF_CT_EXPECT_PERMANENT,
            NF_CT_EXPECT_INACTIVE,
            NF_CT_EXPECT_USERSPACE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
        }
    }

    #[test]
    fn test_expect_flags_no_overlap() {
        let flags = [
            NF_CT_EXPECT_PERMANENT,
            NF_CT_EXPECT_INACTIVE,
            NF_CT_EXPECT_USERSPACE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
