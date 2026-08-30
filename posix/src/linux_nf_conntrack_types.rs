//! `<linux/netfilter/nf_conntrack_common.h>` — connection tracking state codes.
//!
//! Connection tracking (conntrack) is the netfilter subsystem that
//! tracks stateful network connections. It identifies whether a
//! packet belongs to a new, established, or related connection,
//! enabling stateful firewalling. Conntrack entries have a status
//! bitmask and direction that influence NAT and packet matching.

// ---------------------------------------------------------------------------
// Connection tracking states (enum ip_conntrack_info)
// ---------------------------------------------------------------------------

/// New connection (first packet seen).
pub const IP_CT_ESTABLISHED: u32 = 0;
/// Related connection (spawned by an existing connection, e.g. FTP data).
pub const IP_CT_RELATED: u32 = 1;
/// New connection attempt.
pub const IP_CT_NEW: u32 = 2;
/// Established, reply direction.
pub const IP_CT_ESTABLISHED_REPLY: u32 = 3;
/// Related, reply direction.
pub const IP_CT_RELATED_REPLY: u32 = 4;
/// New, reply direction (shouldn't normally occur).
pub const IP_CT_NEW_REPLY: u32 = 5;
/// Number of conntrack info values.
pub const IP_CT_NUMBER: u32 = 6;

// ---------------------------------------------------------------------------
// Connection tracking status bits (nf_ct_status_bits)
// ---------------------------------------------------------------------------

/// Connection is expected (related to another).
pub const IPS_EXPECTED_BIT: u32 = 0;
/// Connection has seen reply packets.
pub const IPS_SEEN_REPLY_BIT: u32 = 1;
/// Connection is assured (bidirectional traffic seen).
pub const IPS_ASSURED_BIT: u32 = 2;
/// Connection is confirmed (committed to hash table).
pub const IPS_CONFIRMED_BIT: u32 = 3;
/// Source NAT is active.
pub const IPS_SRC_NAT_BIT: u32 = 4;
/// Destination NAT is active.
pub const IPS_DST_NAT_BIT: u32 = 5;
/// Sequence number adjustment needed.
pub const IPS_SEQ_ADJUST_BIT: u32 = 6;
/// Source NAT has been done.
pub const IPS_SRC_NAT_DONE_BIT: u32 = 7;
/// Destination NAT has been done.
pub const IPS_DST_NAT_DONE_BIT: u32 = 8;
/// Connection is dying (being removed).
pub const IPS_DYING_BIT: u32 = 9;
/// Connection is fixed-timeout.
pub const IPS_FIXED_TIMEOUT_BIT: u32 = 10;
/// Connection is a template.
pub const IPS_TEMPLATE_BIT: u32 = 11;
/// Connection is untracked.
pub const IPS_UNTRACKED_BIT: u32 = 12;
/// Connection is a helper.
pub const IPS_HELPER_BIT: u32 = 13;
/// Connection is offloaded to hardware.
pub const IPS_OFFLOAD_BIT: u32 = 14;
/// Connection is HW offloaded.
pub const IPS_HW_OFFLOAD_BIT: u32 = 15;

// ---------------------------------------------------------------------------
// Status masks
// ---------------------------------------------------------------------------

/// Expected connection.
pub const IPS_EXPECTED: u32 = 1 << IPS_EXPECTED_BIT;
/// Seen reply.
pub const IPS_SEEN_REPLY: u32 = 1 << IPS_SEEN_REPLY_BIT;
/// Assured connection.
pub const IPS_ASSURED: u32 = 1 << IPS_ASSURED_BIT;
/// Confirmed connection.
pub const IPS_CONFIRMED: u32 = 1 << IPS_CONFIRMED_BIT;
/// Source NAT active.
pub const IPS_SRC_NAT: u32 = 1 << IPS_SRC_NAT_BIT;
/// Destination NAT active.
pub const IPS_DST_NAT: u32 = 1 << IPS_DST_NAT_BIT;
/// Any NAT active.
pub const IPS_NAT_MASK: u32 = IPS_SRC_NAT | IPS_DST_NAT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ct_states_distinct() {
        let states = [
            IP_CT_ESTABLISHED,
            IP_CT_RELATED,
            IP_CT_NEW,
            IP_CT_ESTABLISHED_REPLY,
            IP_CT_RELATED_REPLY,
            IP_CT_NEW_REPLY,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_ct_number() {
        assert_eq!(IP_CT_NUMBER, 6);
    }

    #[test]
    fn test_status_bits_ordered() {
        assert!(IPS_EXPECTED_BIT < IPS_SEEN_REPLY_BIT);
        assert!(IPS_SEEN_REPLY_BIT < IPS_ASSURED_BIT);
        assert!(IPS_ASSURED_BIT < IPS_CONFIRMED_BIT);
    }

    #[test]
    fn test_status_masks_from_bits() {
        assert_eq!(IPS_EXPECTED, 1 << 0);
        assert_eq!(IPS_SEEN_REPLY, 1 << 1);
        assert_eq!(IPS_ASSURED, 1 << 2);
        assert_eq!(IPS_CONFIRMED, 1 << 3);
        assert_eq!(IPS_SRC_NAT, 1 << 4);
        assert_eq!(IPS_DST_NAT, 1 << 5);
    }

    #[test]
    fn test_nat_mask() {
        assert_eq!(IPS_NAT_MASK, IPS_SRC_NAT | IPS_DST_NAT);
        assert_ne!(IPS_NAT_MASK & IPS_SRC_NAT, 0);
        assert_ne!(IPS_NAT_MASK & IPS_DST_NAT, 0);
    }
}
