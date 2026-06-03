//! `<netinet/udp.h>` / `<linux/udp.h>` — UDP socket options.
//!
//! UDP sockopts let userspace control corking (buffering one big
//! datagram from many `send()` calls), segmentation offload (split a
//! large `send()` into multiple datagrams in the NIC), receive
//! coalescing (GRO), and the encapsulation type for tunneling.

// ---------------------------------------------------------------------------
// `setsockopt`/`getsockopt` level
// ---------------------------------------------------------------------------

pub const IPPROTO_UDP: u32 = 17;
pub const SOL_UDP: u32 = IPPROTO_UDP;

pub const IPPROTO_UDPLITE: u32 = 136;
pub const SOL_UDPLITE: u32 = IPPROTO_UDPLITE;

// ---------------------------------------------------------------------------
// `UDP_*` socket options
// ---------------------------------------------------------------------------

pub const UDP_CORK: u32 = 1;
pub const UDP_ENCAP: u32 = 100;
pub const UDP_NO_CHECK6_TX: u32 = 101;
pub const UDP_NO_CHECK6_RX: u32 = 102;
pub const UDP_SEGMENT: u32 = 103;
pub const UDP_GRO: u32 = 104;

// ---------------------------------------------------------------------------
// `UDPLITE_*` socket options — controls partial-checksum coverage
// ---------------------------------------------------------------------------

pub const UDPLITE_SEND_CSCOV: u32 = 10;
pub const UDPLITE_RECV_CSCOV: u32 = 11;

// ---------------------------------------------------------------------------
// `UDP_ENCAP` encapsulation types
// ---------------------------------------------------------------------------

pub const UDP_ENCAP_ESPINUDP_NON_IKE: u32 = 1;
pub const UDP_ENCAP_ESPINUDP: u32 = 2;
pub const UDP_ENCAP_L2TPINUDP: u32 = 3;
pub const UDP_ENCAP_GTP0: u32 = 4;
pub const UDP_ENCAP_GTP1U: u32 = 5;
pub const UDP_ENCAP_RXRPC: u32 = 6;
pub const UDP_ENCAP_GENEVE: u32 = 7;
pub const UDP_ENCAP_VXLAN: u32 = 9;

// ---------------------------------------------------------------------------
// Header sizes
// ---------------------------------------------------------------------------

/// `struct udphdr` is 8 bytes: src(2) + dst(2) + len(2) + check(2).
pub const UDP_HDR_LEN: usize = 8;

/// Maximum UDP payload over IPv4 (65535 - 20 IP - 8 UDP).
pub const UDP_MAX_PAYLOAD_V4: usize = 65_507;

/// Maximum UDP payload over IPv6 (65535 - 40 IP6 - 8 UDP).
pub const UDP_MAX_PAYLOAD_V6: usize = 65_487;

// ---------------------------------------------------------------------------
// Well-known UDP ports
// ---------------------------------------------------------------------------

pub const PORT_DNS: u16 = 53;
pub const PORT_DHCP_SERVER: u16 = 67;
pub const PORT_DHCP_CLIENT: u16 = 68;
pub const PORT_TFTP: u16 = 69;
pub const PORT_NTP: u16 = 123;
pub const PORT_SNMP: u16 = 161;
pub const PORT_SYSLOG: u16 = 514;
pub const PORT_QUIC: u16 = 443;
pub const PORT_MDNS: u16 = 5353;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_numbers() {
        // UDP = 17 (RFC 768), UDP-Lite = 136 (RFC 3828).
        assert_eq!(IPPROTO_UDP, 17);
        assert_eq!(SOL_UDP, IPPROTO_UDP);
        assert_eq!(IPPROTO_UDPLITE, 136);
        assert_eq!(SOL_UDPLITE, IPPROTO_UDPLITE);
    }

    #[test]
    fn test_udp_sockopts_named_block() {
        // UDP_CORK = 1 (predates the rest).
        assert_eq!(UDP_CORK, 1);
        // UDP_ENCAP family starts at 100 to leave room for new compat sockopts.
        let e = [UDP_ENCAP, UDP_NO_CHECK6_TX, UDP_NO_CHECK6_RX, UDP_SEGMENT, UDP_GRO];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v, 100 + i as u32);
        }
    }

    #[test]
    fn test_udplite_csccov_pair_adjacent() {
        // Send/Recv coverage sockopts are adjacent 10/11.
        assert_eq!(UDPLITE_SEND_CSCOV, 10);
        assert_eq!(UDPLITE_RECV_CSCOV, UDPLITE_SEND_CSCOV + 1);
    }

    #[test]
    fn test_encap_types_distinct() {
        let e = [
            UDP_ENCAP_ESPINUDP_NON_IKE,
            UDP_ENCAP_ESPINUDP,
            UDP_ENCAP_L2TPINUDP,
            UDP_ENCAP_GTP0,
            UDP_ENCAP_GTP1U,
            UDP_ENCAP_RXRPC,
            UDP_ENCAP_GENEVE,
            UDP_ENCAP_VXLAN,
        ];
        // All distinct, all ≤ 9 (no gaps except #8 which was retired).
        for a in 0..e.len() {
            for b in (a + 1)..e.len() {
                assert_ne!(e[a], e[b]);
            }
        }
        assert!(UDP_ENCAP_VXLAN <= 9);
    }

    #[test]
    fn test_header_and_payload_math() {
        assert_eq!(UDP_HDR_LEN, 8);
        // 65535 - 20 (IP header) - 8 (UDP header) = 65507.
        assert_eq!(UDP_MAX_PAYLOAD_V4, 65_535 - 20 - UDP_HDR_LEN);
        // 65535 - 40 (IPv6 header) - 8 (UDP header) = 65487.
        assert_eq!(UDP_MAX_PAYLOAD_V6, 65_535 - 40 - UDP_HDR_LEN);
    }

    #[test]
    fn test_well_known_ports() {
        // DHCP server is 67, client is 68 — adjacent by spec.
        assert_eq!(PORT_DHCP_CLIENT, PORT_DHCP_SERVER + 1);
        assert_eq!(PORT_DNS, 53);
        assert_eq!(PORT_NTP, 123);
        assert_eq!(PORT_MDNS, 5353);
    }
}
