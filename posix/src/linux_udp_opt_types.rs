//! `<netinet/udp.h>` — UDP socket option constants.
//!
//! UDP socket options control checksum behavior, corking (batch
//! multiple sends into one datagram), and GRO (Generic Receive
//! Offload) for high-throughput UDP applications like QUIC and
//! media streaming.

// ---------------------------------------------------------------------------
// UDP socket options (level IPPROTO_UDP)
// ---------------------------------------------------------------------------

/// Cork (accumulate data, send as single datagram on uncork).
pub const UDP_CORK: u32 = 1;
/// Encapsulation type (for ESP-over-UDP, etc.).
pub const UDP_ENCAP: u32 = 100;
/// Do not generate UDP checksums for outgoing packets.
pub const UDP_NO_CHECK6_TX: u32 = 101;
/// Do not verify UDP checksums on incoming packets.
pub const UDP_NO_CHECK6_RX: u32 = 102;
/// Enable UDP segmentation offload (GSO).
pub const UDP_SEGMENT: u32 = 103;
/// Enable UDP GRO (receive offload, coalesced datagrams).
pub const UDP_GRO: u32 = 104;

// ---------------------------------------------------------------------------
// UDP encapsulation types (UDP_ENCAP values)
// ---------------------------------------------------------------------------

/// ESP in UDP encapsulation (IPsec NAT traversal).
pub const UDP_ENCAP_ESPINUDP: u32 = 2;
/// ESP in UDP with non-IKE marker.
pub const UDP_ENCAP_ESPINUDP_NON_IKE: u32 = 1;
/// L2TP over UDP.
pub const UDP_ENCAP_L2TPINUDP: u32 = 3;
/// GTP-U (GPRS Tunneling Protocol).
pub const UDP_ENCAP_GTP0: u32 = 4;
/// GTP-U version 1.
pub const UDP_ENCAP_GTP1U: u32 = 5;

// ---------------------------------------------------------------------------
// UDP-Lite socket options (level IPPROTO_UDPLITE)
// ---------------------------------------------------------------------------

/// Minimum checksum coverage for sending.
pub const UDPLITE_SEND_CSCOV: u32 = 10;
/// Minimum checksum coverage for receiving.
pub const UDPLITE_RECV_CSCOV: u32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_options_distinct() {
        let opts = [
            UDP_CORK, UDP_ENCAP, UDP_NO_CHECK6_TX,
            UDP_NO_CHECK6_RX, UDP_SEGMENT, UDP_GRO,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_encap_types_distinct() {
        let encaps = [
            UDP_ENCAP_ESPINUDP, UDP_ENCAP_ESPINUDP_NON_IKE,
            UDP_ENCAP_L2TPINUDP, UDP_ENCAP_GTP0, UDP_ENCAP_GTP1U,
        ];
        for i in 0..encaps.len() {
            for j in (i + 1)..encaps.len() {
                assert_ne!(encaps[i], encaps[j]);
            }
        }
    }

    #[test]
    fn test_udplite_options_distinct() {
        assert_ne!(UDPLITE_SEND_CSCOV, UDPLITE_RECV_CSCOV);
    }
}
