//! `<linux/skbuff.h>` — Socket buffer (sk_buff) constants.
//!
//! The sk_buff (socket buffer) is the fundamental data structure for
//! network packet handling in Linux. Every packet — incoming, outgoing,
//! or forwarded — is wrapped in an sk_buff that tracks the packet's
//! data, headers at each protocol layer, routing information, and
//! metadata. sk_buffs can be cloned (shared data, separate metadata),
//! expanded, trimmed, and fragmented.

// ---------------------------------------------------------------------------
// sk_buff packet types (pkt_type field)
// ---------------------------------------------------------------------------

/// Packet is addressed to us (unicast to our MAC).
pub const PACKET_HOST: u32 = 0;
/// Broadcast packet (sent to all hosts on segment).
pub const PACKET_BROADCAST: u32 = 1;
/// Multicast packet (sent to a group we joined).
pub const PACKET_MULTICAST: u32 = 2;
/// Packet for another host (we're in promiscuous mode).
pub const PACKET_OTHERHOST: u32 = 3;
/// Packet originated from us (loopback/local delivery).
pub const PACKET_OUTGOING: u32 = 4;
/// Loopback packet.
pub const PACKET_LOOPBACK: u32 = 5;
/// User-space originated packet (AF_PACKET).
pub const PACKET_USER: u32 = 6;
/// Kernel-originated packet.
pub const PACKET_KERNEL: u32 = 7;

// ---------------------------------------------------------------------------
// sk_buff checksum types
// ---------------------------------------------------------------------------

/// No checksum assistance (software must compute).
pub const CHECKSUM_NONE: u32 = 0;
/// Checksum not needed (local-originated, trusted).
pub const CHECKSUM_UNNECESSARY: u32 = 1;
/// Hardware completed full checksum.
pub const CHECKSUM_COMPLETE: u32 = 2;
/// Hardware should compute checksum on transmit.
pub const CHECKSUM_PARTIAL: u32 = 3;

// ---------------------------------------------------------------------------
// sk_buff clone/copy flags
// ---------------------------------------------------------------------------

/// Clone: share data, separate metadata.
pub const SKB_CLONE_DATA_SHARED: u32 = 0x01;
/// Fastclone: pre-allocated clone header.
pub const SKB_CLONE_FAST: u32 = 0x02;
/// Clone inherits the frag list.
pub const SKB_CLONE_FRAG_LIST: u32 = 0x04;

// ---------------------------------------------------------------------------
// sk_buff allocation priorities
// ---------------------------------------------------------------------------

/// Normal priority (can sleep, common path).
pub const SKB_ALLOC_NORMAL: u32 = 0;
/// Receive path (from NAPI, cannot sleep).
pub const SKB_ALLOC_RX: u32 = 1;
/// Fast-path clone (pre-allocated).
pub const SKB_ALLOC_FCLONE: u32 = 2;

// ---------------------------------------------------------------------------
// GSO (Generic Segmentation Offload) types
// ---------------------------------------------------------------------------

/// TCP segmentation offload.
pub const SKB_GSO_TCPV4: u32 = 0x0001;
/// UDP fragmentation offload.
pub const SKB_GSO_UDP: u32 = 0x0002;
/// TCP ECN segmentation.
pub const SKB_GSO_TCP_ECN: u32 = 0x0008;
/// TCPv6 segmentation.
pub const SKB_GSO_TCPV6: u32 = 0x0010;
/// UDP tunnel segmentation.
pub const SKB_GSO_UDP_TUNNEL: u32 = 0x0020;
/// GRE tunnel segmentation.
pub const SKB_GSO_GRE: u32 = 0x0040;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_types_distinct() {
        let types = [
            PACKET_HOST, PACKET_BROADCAST, PACKET_MULTICAST,
            PACKET_OTHERHOST, PACKET_OUTGOING, PACKET_LOOPBACK,
            PACKET_USER, PACKET_KERNEL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_checksum_types_distinct() {
        let types = [
            CHECKSUM_NONE, CHECKSUM_UNNECESSARY,
            CHECKSUM_COMPLETE, CHECKSUM_PARTIAL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_gso_types_no_overlap() {
        let types = [
            SKB_GSO_TCPV4, SKB_GSO_UDP, SKB_GSO_TCP_ECN,
            SKB_GSO_TCPV6, SKB_GSO_UDP_TUNNEL, SKB_GSO_GRE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }
}
