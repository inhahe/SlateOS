//! `<linux/netfilter/nf_conntrack_common.h>` — Connection tracking constants.
//!
//! Conntrack tracks network connections passing through
//! netfilter.  These constants define connection states,
//! status flags, and event types.

// ---------------------------------------------------------------------------
// Connection tracking states (ctinfo)
// ---------------------------------------------------------------------------

/// Established (original direction).
pub const IP_CT_ESTABLISHED: u32 = 0;
/// Related (original direction).
pub const IP_CT_RELATED: u32 = 1;
/// New connection.
pub const IP_CT_NEW: u32 = 2;
/// Established (reply direction).
pub const IP_CT_ESTABLISHED_REPLY: u32 = 3;
/// Related (reply direction).
pub const IP_CT_RELATED_REPLY: u32 = 4;
/// Untracked connection.
pub const IP_CT_UNTRACKED: u32 = 7;

// ---------------------------------------------------------------------------
// Connection tracking status flags (IPS_*)
// ---------------------------------------------------------------------------

/// Connection has seen traffic in both directions.
pub const IPS_SEEN_REPLY: u32 = 1 << 1;
/// Connection is assured (confirmed with reply).
pub const IPS_ASSURED: u32 = 1 << 2;
/// Connection is confirmed (committed to table).
pub const IPS_CONFIRMED: u32 = 1 << 3;
/// Source NAT applied.
pub const IPS_SRC_NAT: u32 = 1 << 4;
/// Destination NAT applied.
pub const IPS_DST_NAT: u32 = 1 << 5;
/// Combined NAT mask.
pub const IPS_NAT_MASK: u32 = IPS_SRC_NAT | IPS_DST_NAT;
/// Sequence number adjustment needed.
pub const IPS_SEQ_ADJUST: u32 = 1 << 6;
/// Source NAT done.
pub const IPS_SRC_NAT_DONE: u32 = 1 << 7;
/// Destination NAT done.
pub const IPS_DST_NAT_DONE: u32 = 1 << 8;
/// Connection is dying (being destroyed).
pub const IPS_DYING: u32 = 1 << 9;
/// Connection is fixed timeout.
pub const IPS_FIXED_TIMEOUT: u32 = 1 << 10;
/// Template connection.
pub const IPS_TEMPLATE: u32 = 1 << 11;
/// Untracked.
pub const IPS_UNTRACKED: u32 = 1 << 12;
/// Helper assigned.
pub const IPS_HELPER: u32 = 1 << 13;
/// Offloaded connection.
pub const IPS_OFFLOAD: u32 = 1 << 14;
/// Hardware offloaded.
pub const IPS_HW_OFFLOAD: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Connection tracking events (IPCT_*)
// ---------------------------------------------------------------------------

/// New connection.
pub const IPCT_NEW: u32 = 0;
/// Related connection.
pub const IPCT_RELATED: u32 = 1;
/// Connection destroyed.
pub const IPCT_DESTROY: u32 = 2;
/// Reply received.
pub const IPCT_REPLY: u32 = 3;
/// Connection assured.
pub const IPCT_ASSURED: u32 = 4;
/// Protocol info updated.
pub const IPCT_PROTOINFO: u32 = 5;
/// Helper info updated.
pub const IPCT_HELPER: u32 = 6;
/// Mark updated.
pub const IPCT_MARK: u32 = 7;
/// NAT sequence adjust.
pub const IPCT_NATSEQADJ: u32 = 8;
/// Security context.
pub const IPCT_SECMARK: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            IP_CT_ESTABLISHED, IP_CT_RELATED, IP_CT_NEW,
            IP_CT_ESTABLISHED_REPLY, IP_CT_RELATED_REPLY,
            IP_CT_UNTRACKED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_established_is_zero() {
        assert_eq!(IP_CT_ESTABLISHED, 0);
    }

    #[test]
    fn test_status_flags_powers_of_two() {
        let flags = [
            IPS_SEEN_REPLY, IPS_ASSURED, IPS_CONFIRMED,
            IPS_SRC_NAT, IPS_DST_NAT, IPS_SEQ_ADJUST,
            IPS_SRC_NAT_DONE, IPS_DST_NAT_DONE, IPS_DYING,
            IPS_FIXED_TIMEOUT, IPS_TEMPLATE, IPS_UNTRACKED,
            IPS_HELPER, IPS_OFFLOAD, IPS_HW_OFFLOAD,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_nat_mask() {
        assert_eq!(IPS_NAT_MASK, IPS_SRC_NAT | IPS_DST_NAT);
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            IPCT_NEW, IPCT_RELATED, IPCT_DESTROY, IPCT_REPLY,
            IPCT_ASSURED, IPCT_PROTOINFO, IPCT_HELPER,
            IPCT_MARK, IPCT_NATSEQADJ, IPCT_SECMARK,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_new_event_is_zero() {
        assert_eq!(IPCT_NEW, 0);
    }
}
