//! `<linux/udp.h>` — UDP protocol constants.
//!
//! UDP (User Datagram Protocol) provides connectionless, unreliable
//! datagram delivery. Socket options control checksum behavior, cork
//! mode (aggregate small writes), GRO/GSO for hardware offload, and
//! encapsulation for tunneling protocols (VXLAN, WireGuard, etc.).

// ---------------------------------------------------------------------------
// UDP socket options (SOL_UDP level)
// ---------------------------------------------------------------------------

/// Cork output (hold packets until uncorked).
pub const UDP_CORK: u32 = 1;
/// Enable/disable UDP checksum offload.
pub const UDP_NO_CHECK6_TX: u32 = 101;
/// Accept packets with bad checksums.
pub const UDP_NO_CHECK6_RX: u32 = 102;
/// Enable UDP segmentation offload (GSO).
pub const UDP_SEGMENT: u32 = 103;
/// Enable UDP GRO (Generic Receive Offload).
pub const UDP_GRO: u32 = 104;
/// Set encapsulation type.
pub const UDP_ENCAP: u32 = 100;

// ---------------------------------------------------------------------------
// UDP encapsulation types (UDP_ENCAP option values)
// ---------------------------------------------------------------------------

/// ESP in UDP encapsulation (IPsec NAT-T).
pub const UDP_ENCAP_ESPINUDP_NON_IKE: u32 = 1;
/// ESP in UDP encapsulation (IKE).
pub const UDP_ENCAP_ESPINUDP: u32 = 2;
/// L2TP encapsulation.
pub const UDP_ENCAP_L2TPINUDP: u32 = 3;
/// GTP (GPRS Tunneling Protocol).
pub const UDP_ENCAP_GTP0: u32 = 4;
/// GTP v1.
pub const UDP_ENCAP_GTP1U: u32 = 5;

// ---------------------------------------------------------------------------
// Well-known UDP ports
// ---------------------------------------------------------------------------

/// DNS.
pub const UDP_PORT_DNS: u16 = 53;
/// DHCP server.
pub const UDP_PORT_DHCP_SERVER: u16 = 67;
/// DHCP client.
pub const UDP_PORT_DHCP_CLIENT: u16 = 68;
/// NTP.
pub const UDP_PORT_NTP: u16 = 123;
/// SNMP.
pub const UDP_PORT_SNMP: u16 = 161;
/// Syslog.
pub const UDP_PORT_SYSLOG: u16 = 514;
/// VXLAN.
pub const UDP_PORT_VXLAN: u16 = 4789;
/// WireGuard (default).
pub const UDP_PORT_WIREGUARD: u16 = 51820;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            UDP_CORK, UDP_NO_CHECK6_TX, UDP_NO_CHECK6_RX,
            UDP_SEGMENT, UDP_GRO, UDP_ENCAP,
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
            UDP_ENCAP_ESPINUDP_NON_IKE, UDP_ENCAP_ESPINUDP,
            UDP_ENCAP_L2TPINUDP, UDP_ENCAP_GTP0, UDP_ENCAP_GTP1U,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ports_distinct() {
        let ports = [
            UDP_PORT_DNS, UDP_PORT_DHCP_SERVER, UDP_PORT_DHCP_CLIENT,
            UDP_PORT_NTP, UDP_PORT_SNMP, UDP_PORT_SYSLOG,
            UDP_PORT_VXLAN, UDP_PORT_WIREGUARD,
        ];
        for i in 0..ports.len() {
            for j in (i + 1)..ports.len() {
                assert_ne!(ports[i], ports[j]);
            }
        }
    }

    #[test]
    fn test_ports_nonzero() {
        assert!(UDP_PORT_DNS > 0);
        assert!(UDP_PORT_WIREGUARD > 0);
    }
}
