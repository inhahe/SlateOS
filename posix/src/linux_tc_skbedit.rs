//! `<linux/tc_act/tc_skbedit.h>` — TC skb edit action constants.
//!
//! The skbedit action modifies socket buffer (skb) metadata fields:
//! priority, queue mapping, mark, ptype, and other fields that
//! affect packet processing but don't modify the packet data itself.

// ---------------------------------------------------------------------------
// Skbedit flags (what to edit)
// ---------------------------------------------------------------------------

/// Edit priority.
pub const TCA_SKBEDIT_F_PRIORITY: u32 = 1 << 0;
/// Edit queue mapping.
pub const TCA_SKBEDIT_F_QUEUE_MAPPING: u32 = 1 << 1;
/// Edit skb mark.
pub const TCA_SKBEDIT_F_MARK: u32 = 1 << 2;
/// Edit packet type.
pub const TCA_SKBEDIT_F_PTYPE: u32 = 1 << 3;
/// Edit skb->hash.
pub const TCA_SKBEDIT_F_HASH: u32 = 1 << 4;
/// Inherit classid.
pub const TCA_SKBEDIT_F_INHERITDSFIELD: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Skbedit netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_SKBEDIT_UNSPEC: u16 = 0;
/// Timer info.
pub const TCA_SKBEDIT_TM: u16 = 1;
/// Parameters.
pub const TCA_SKBEDIT_PARMS: u16 = 2;
/// Priority value.
pub const TCA_SKBEDIT_PRIORITY: u16 = 3;
/// Queue mapping value.
pub const TCA_SKBEDIT_QUEUE_MAPPING: u16 = 4;
/// Mark value.
pub const TCA_SKBEDIT_MARK: u16 = 5;
/// Padding.
pub const TCA_SKBEDIT_PAD: u16 = 6;
/// Packet type value.
pub const TCA_SKBEDIT_PTYPE: u16 = 7;
/// Mask for mark.
pub const TCA_SKBEDIT_MASK: u16 = 8;
/// Flags.
pub const TCA_SKBEDIT_FLAGS: u16 = 9;
/// Queue mapping max.
pub const TCA_SKBEDIT_QUEUE_MAPPING_MAX: u16 = 10;

// ---------------------------------------------------------------------------
// Packet types (for PTYPE editing)
// ---------------------------------------------------------------------------

/// Packet addressed to local host.
pub const PACKET_HOST: u8 = 0;
/// Broadcast packet.
pub const PACKET_BROADCAST: u8 = 1;
/// Multicast packet.
pub const PACKET_MULTICAST: u8 = 2;
/// Packet addressed to other host (promiscuous).
pub const PACKET_OTHERHOST: u8 = 3;
/// Outgoing packet (loopback).
pub const PACKET_OUTGOING: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            TCA_SKBEDIT_F_PRIORITY,
            TCA_SKBEDIT_F_QUEUE_MAPPING,
            TCA_SKBEDIT_F_MARK,
            TCA_SKBEDIT_F_PTYPE,
            TCA_SKBEDIT_F_HASH,
            TCA_SKBEDIT_F_INHERITDSFIELD,
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
            TCA_SKBEDIT_F_PRIORITY,
            TCA_SKBEDIT_F_QUEUE_MAPPING,
            TCA_SKBEDIT_F_MARK,
            TCA_SKBEDIT_F_PTYPE,
            TCA_SKBEDIT_F_HASH,
            TCA_SKBEDIT_F_INHERITDSFIELD,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_SKBEDIT_UNSPEC,
            TCA_SKBEDIT_TM,
            TCA_SKBEDIT_PARMS,
            TCA_SKBEDIT_PRIORITY,
            TCA_SKBEDIT_QUEUE_MAPPING,
            TCA_SKBEDIT_MARK,
            TCA_SKBEDIT_PAD,
            TCA_SKBEDIT_PTYPE,
            TCA_SKBEDIT_MASK,
            TCA_SKBEDIT_FLAGS,
            TCA_SKBEDIT_QUEUE_MAPPING_MAX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_packet_types_distinct() {
        let types = [
            PACKET_HOST,
            PACKET_BROADCAST,
            PACKET_MULTICAST,
            PACKET_OTHERHOST,
            PACKET_OUTGOING,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
