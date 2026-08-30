//! `<linux/netfilter/nf_conntrack_common.h>` — Connection tracking constants.
//!
//! Netfilter connection tracking (conntrack) is the stateful packet
//! inspection engine in the Linux kernel. It tracks TCP, UDP, ICMP,
//! and other protocol connections, enabling stateful firewall rules,
//! NAT, and connection-based load balancing.

// ---------------------------------------------------------------------------
// Connection states (IP_CT_*)
// ---------------------------------------------------------------------------

/// New connection (first packet).
pub const IP_CT_NEW: u8 = 0;
/// Established connection (bidirectional traffic seen).
pub const IP_CT_ESTABLISHED: u8 = 1;
/// Related connection (e.g., FTP data channel).
pub const IP_CT_RELATED: u8 = 2;

// ---------------------------------------------------------------------------
// Connection status bits (IPS_*)
// ---------------------------------------------------------------------------

/// Expected connection (from helper).
pub const IPS_EXPECTED: u32 = 1 << 0;
/// Seen reply packets.
pub const IPS_SEEN_REPLY: u32 = 1 << 1;
/// Connection is assured (bidirectional confirmed).
pub const IPS_ASSURED: u32 = 1 << 2;
/// Connection confirmed (committed to table).
pub const IPS_CONFIRMED: u32 = 1 << 3;
/// Source NAT applied.
pub const IPS_SRC_NAT: u32 = 1 << 4;
/// Destination NAT applied.
pub const IPS_DST_NAT: u32 = 1 << 5;
/// NAT done in both directions.
pub const IPS_NAT_MASK: u32 = (1 << 4) | (1 << 5);
/// Sequence number adjustment needed.
pub const IPS_SEQ_ADJUST: u32 = 1 << 6;
/// Source NAT done (original direction).
pub const IPS_SRC_NAT_DONE: u32 = 1 << 7;
/// Destination NAT done (original direction).
pub const IPS_DST_NAT_DONE: u32 = 1 << 8;
/// Connection is dying (being removed).
pub const IPS_DYING: u32 = 1 << 9;
/// Fixed timeout (no extension).
pub const IPS_FIXED_TIMEOUT: u32 = 1 << 10;
/// Template entry (not a real connection).
pub const IPS_TEMPLATE: u32 = 1 << 11;
/// Untracked connection.
pub const IPS_UNTRACKED: u32 = 1 << 12;
/// Helper assigned.
pub const IPS_HELPER: u32 = 1 << 13;
/// Offloaded to hardware.
pub const IPS_OFFLOAD: u32 = 1 << 14;
/// Hardware offload reply direction.
pub const IPS_HW_OFFLOAD: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Connection tracking events
// ---------------------------------------------------------------------------

/// New connection event.
pub const IPCT_NEW: u32 = 1 << 0;
/// Related connection event.
pub const IPCT_RELATED: u32 = 1 << 1;
/// Destroy event.
pub const IPCT_DESTROY: u32 = 1 << 2;
/// Status changed.
pub const IPCT_STATUS: u32 = 1 << 3;
/// Protocol info changed.
pub const IPCT_PROTOINFO: u32 = 1 << 4;
/// Helper info changed.
pub const IPCT_HELPER: u32 = 1 << 5;
/// Mark changed.
pub const IPCT_MARK: u32 = 1 << 6;
/// Sequence adjustment changed.
pub const IPCT_SEQADJ: u32 = 1 << 7;
/// Label changed.
pub const IPCT_LABEL: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Conntrack tuple direction
// ---------------------------------------------------------------------------

/// Original direction (initiator → responder).
pub const IP_CT_DIR_ORIGINAL: u8 = 0;
/// Reply direction (responder → initiator).
pub const IP_CT_DIR_REPLY: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [IP_CT_NEW, IP_CT_ESTABLISHED, IP_CT_RELATED];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_status_bits_selected_no_overlap() {
        // Check that individual bits don't overlap (excluding NAT_MASK which is composite)
        let bits = [
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
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_nat_mask() {
        assert_eq!(IPS_NAT_MASK, IPS_SRC_NAT | IPS_DST_NAT);
    }

    #[test]
    fn test_events_no_overlap() {
        let events = [
            IPCT_NEW,
            IPCT_RELATED,
            IPCT_DESTROY,
            IPCT_STATUS,
            IPCT_PROTOINFO,
            IPCT_HELPER,
            IPCT_MARK,
            IPCT_SEQADJ,
            IPCT_LABEL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_eq!(events[i] & events[j], 0);
            }
        }
    }

    #[test]
    fn test_directions_distinct() {
        assert_ne!(IP_CT_DIR_ORIGINAL, IP_CT_DIR_REPLY);
    }
}
