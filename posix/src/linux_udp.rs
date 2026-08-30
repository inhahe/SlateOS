//! `<linux/udp.h>` — UDP protocol header and options.
//!
//! Provides the UDP header structure and UDP socket options.

pub use crate::socket::IPPROTO_UDP;

// ---------------------------------------------------------------------------
// UDP header
// ---------------------------------------------------------------------------

/// UDP datagram header (8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Udphdr {
    /// Source port.
    pub source: u16,
    /// Destination port.
    pub dest: u16,
    /// Datagram length (header + data).
    pub len: u16,
    /// Checksum.
    pub check: u16,
}

// ---------------------------------------------------------------------------
// UDP socket options (SOL_UDP / IPPROTO_UDP level)
// ---------------------------------------------------------------------------

/// Enable UDP cork mode (accumulate data before sending).
pub const UDP_CORK: i32 = 1;
/// Enable UDP encapsulation.
pub const UDP_ENCAP: i32 = 100;
/// Disable sending checksum.
pub const UDP_NO_CHECK6_TX: i32 = 101;
/// Disable receiving checksum check.
pub const UDP_NO_CHECK6_RX: i32 = 102;
/// Enable GRO (Generic Receive Offload).
pub const UDP_GRO: i32 = 104;
/// UDP segment size for GSO.
pub const UDP_SEGMENT: i32 = 103;

// ---------------------------------------------------------------------------
// UDP encapsulation types
// ---------------------------------------------------------------------------

/// L2TP over UDP.
pub const UDP_ENCAP_L2TPINUDP: i32 = 3;
/// ESP over UDP (NAT traversal).
pub const UDP_ENCAP_ESPINUDP_NON_IKE: i32 = 1;
/// ESP over UDP with non-ESP marker.
pub const UDP_ENCAP_ESPINUDP: i32 = 2;
/// GTP-U over UDP.
pub const UDP_ENCAP_GTP0: i32 = 4;
/// GTP-U v1 over UDP.
pub const UDP_ENCAP_GTP1U: i32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udphdr_size() {
        assert_eq!(core::mem::size_of::<Udphdr>(), 8);
    }

    #[test]
    fn test_udp_options_distinct() {
        let opts = [
            UDP_CORK,
            UDP_ENCAP,
            UDP_NO_CHECK6_TX,
            UDP_NO_CHECK6_RX,
            UDP_SEGMENT,
            UDP_GRO,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_encap_types_distinct() {
        let types = [
            UDP_ENCAP_ESPINUDP_NON_IKE,
            UDP_ENCAP_ESPINUDP,
            UDP_ENCAP_L2TPINUDP,
            UDP_ENCAP_GTP0,
            UDP_ENCAP_GTP1U,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ipproto_udp() {
        assert_eq!(IPPROTO_UDP, 17);
    }
}
