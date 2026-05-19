//! `<linux/netfilter/nf_conntrack_common.h>` — Additional conntrack constants.
//!
//! Supplementary connection tracking constants covering status flags,
//! event types, and expectation flags.

// ---------------------------------------------------------------------------
// Conntrack status flags
// ---------------------------------------------------------------------------

/// Expected connection.
pub const IPS_EXPECTED: u32 = 1 << 0;
/// Seen reply.
pub const IPS_SEEN_REPLY: u32 = 1 << 1;
/// Assured.
pub const IPS_ASSURED: u32 = 1 << 2;
/// Confirmed.
pub const IPS_CONFIRMED: u32 = 1 << 3;
/// Source NAT.
pub const IPS_SRC_NAT: u32 = 1 << 4;
/// Destination NAT.
pub const IPS_DST_NAT: u32 = 1 << 5;
/// Sequence number adjust.
pub const IPS_SEQ_ADJUST: u32 = 1 << 6;
/// Not source NAT done.
pub const IPS_SRC_NAT_DONE: u32 = 1 << 7;
/// Not dest NAT done.
pub const IPS_DST_NAT_DONE: u32 = 1 << 8;
/// Dying.
pub const IPS_DYING: u32 = 1 << 9;
/// Fixed timeout.
pub const IPS_FIXED_TIMEOUT: u32 = 1 << 10;
/// Template.
pub const IPS_TEMPLATE: u32 = 1 << 11;
/// Untracked.
pub const IPS_UNTRACKED: u32 = 1 << 12;
/// Helper.
pub const IPS_HELPER: u32 = 1 << 13;
/// Offload.
pub const IPS_OFFLOAD: u32 = 1 << 14;
/// Hardware offload.
pub const IPS_HW_OFFLOAD: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Conntrack events
// ---------------------------------------------------------------------------

/// New conntrack.
pub const IPCT_NEW: u32 = 0;
/// Related conntrack.
pub const IPCT_RELATED: u32 = 1;
/// Destroy conntrack.
pub const IPCT_DESTROY: u32 = 2;
/// Reply.
pub const IPCT_REPLY: u32 = 3;
/// Assured.
pub const IPCT_ASSURED: u32 = 4;
/// Proto info.
pub const IPCT_PROTOINFO: u32 = 5;
/// Helper.
pub const IPCT_HELPER: u32 = 6;
/// Mark.
pub const IPCT_MARK: u32 = 7;
/// Seq adj.
pub const IPCT_SEQADJ: u32 = 8;
/// NAT seq adj.
pub const IPCT_NATSEQADJ: u32 = IPCT_SEQADJ;
/// Secmark.
pub const IPCT_SECMARK: u32 = 9;
/// Label.
pub const IPCT_LABEL: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_flags_no_overlap() {
        let flags = [
            IPS_EXPECTED, IPS_SEEN_REPLY, IPS_ASSURED,
            IPS_CONFIRMED, IPS_SRC_NAT, IPS_DST_NAT,
            IPS_SEQ_ADJUST, IPS_SRC_NAT_DONE, IPS_DST_NAT_DONE,
            IPS_DYING, IPS_FIXED_TIMEOUT, IPS_TEMPLATE,
            IPS_UNTRACKED, IPS_HELPER, IPS_OFFLOAD, IPS_HW_OFFLOAD,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_status_flags_power_of_two() {
        let flags = [
            IPS_EXPECTED, IPS_SEEN_REPLY, IPS_ASSURED,
            IPS_CONFIRMED, IPS_SRC_NAT, IPS_DST_NAT,
            IPS_SEQ_ADJUST, IPS_SRC_NAT_DONE, IPS_DST_NAT_DONE,
            IPS_DYING, IPS_FIXED_TIMEOUT, IPS_TEMPLATE,
            IPS_UNTRACKED, IPS_HELPER, IPS_OFFLOAD, IPS_HW_OFFLOAD,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not power of two", flag);
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            IPCT_NEW, IPCT_RELATED, IPCT_DESTROY,
            IPCT_REPLY, IPCT_ASSURED, IPCT_PROTOINFO,
            IPCT_HELPER, IPCT_MARK, IPCT_SEQADJ,
            IPCT_SECMARK, IPCT_LABEL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_natseqadj_alias() {
        assert_eq!(IPCT_NATSEQADJ, IPCT_SEQADJ);
    }
}
