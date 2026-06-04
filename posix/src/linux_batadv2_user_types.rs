//! `<linux/batadv_packet.h>` — B.A.T.M.A.N.-Adv on-wire packet format.
//!
//! Continuation of `linux_batadv_user_types`: this module covers the
//! per-packet header fields and the EtherType identifier used by the
//! `batman-adv` driver on the wire.

// ---------------------------------------------------------------------------
// EtherType allocated to BATMAN-Adv
// ---------------------------------------------------------------------------

/// IEEE-assigned EtherType for BATMAN-Adv mesh frames.
pub const ETH_P_BATMAN: u16 = 0x4305;

// ---------------------------------------------------------------------------
// Compat-version field (`batadv_header.version`)
// ---------------------------------------------------------------------------

/// On-wire compat version. Bumped each time the wire format changes
/// incompatibly. Currently 15.
pub const BATADV_COMPAT_VERSION: u8 = 15;

// ---------------------------------------------------------------------------
// Packet type identifiers (`batadv_header.packet_type`)
// ---------------------------------------------------------------------------

pub const BATADV_IV_OGM: u8 = 0x00;
pub const BATADV_BCAST: u8 = 0x01;
pub const BATADV_CODED: u8 = 0x02;
pub const BATADV_ELP: u8 = 0x03;
pub const BATADV_OGM2: u8 = 0x04;
pub const BATADV_MCAST: u8 = 0x05;

pub const BATADV_UNICAST: u8 = 0x40;
pub const BATADV_UNICAST_FRAG: u8 = 0x41;
pub const BATADV_UNICAST_4ADDR: u8 = 0x42;
pub const BATADV_ICMP: u8 = 0x43;
pub const BATADV_UNICAST_TVLV: u8 = 0x44;

/// Bit mask: low 6 bits select packet type; bit 6 separates
/// broadcast-class (0x00..0x3F) from unicast-class (0x40..0x7F).
pub const BATADV_PACKET_TYPE_MASK: u8 = 0x7F;
pub const BATADV_UNICAST_CLASS_BIT: u8 = 0x40;

// ---------------------------------------------------------------------------
// ICMP sub-types (`batadv_icmp.msg_type`)
// ---------------------------------------------------------------------------

pub const BATADV_ECHO_REPLY: u8 = 0;
pub const BATADV_DESTINATION_UNREACHABLE: u8 = 3;
pub const BATADV_ECHO_REQUEST: u8 = 8;
pub const BATADV_TTL_EXCEEDED: u8 = 11;
pub const BATADV_PARAMETER_PROBLEM: u8 = 12;
pub const BATADV_TP: u8 = 15;

// ---------------------------------------------------------------------------
// TVLV (type-length-value) container type identifiers
// ---------------------------------------------------------------------------

pub const BATADV_TVLV_GW: u8 = 0x01;
pub const BATADV_TVLV_DAT: u8 = 0x02;
pub const BATADV_TVLV_NC: u8 = 0x03;
pub const BATADV_TVLV_TT: u8 = 0x04;
pub const BATADV_TVLV_ROAM: u8 = 0x05;
pub const BATADV_TVLV_MCAST: u8 = 0x06;
pub const BATADV_TVLV_MCAST_TRACKER: u8 = 0x07;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethertype_is_iana_assigned() {
        // IEEE registry: 0x4305 = "B.A.T.M.A.N. Advanced".
        assert_eq!(ETH_P_BATMAN, 0x4305);
        // Always above 0x0600 per IEEE 802 length/type rule.
        assert!(ETH_P_BATMAN >= 0x0600);
    }

    #[test]
    fn test_compat_version() {
        assert_eq!(BATADV_COMPAT_VERSION, 15);
    }

    #[test]
    fn test_broadcast_class_dense_0_to_5() {
        let b = [
            BATADV_IV_OGM,
            BATADV_BCAST,
            BATADV_CODED,
            BATADV_ELP,
            BATADV_OGM2,
            BATADV_MCAST,
        ];
        for (i, &v) in b.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // None set the unicast-class bit.
        for &v in &b {
            assert_eq!(v & BATADV_UNICAST_CLASS_BIT, 0);
        }
    }

    #[test]
    fn test_unicast_class_dense_0x40_to_0x44() {
        let u = [
            BATADV_UNICAST,
            BATADV_UNICAST_FRAG,
            BATADV_UNICAST_4ADDR,
            BATADV_ICMP,
            BATADV_UNICAST_TVLV,
        ];
        for (i, &v) in u.iter().enumerate() {
            assert_eq!(v as usize, 0x40 + i);
        }
        // All carry the unicast-class bit.
        for &v in &u {
            assert_eq!(v & BATADV_UNICAST_CLASS_BIT, BATADV_UNICAST_CLASS_BIT);
            assert!(v & BATADV_PACKET_TYPE_MASK == v); // bit 7 unused
        }
    }

    #[test]
    fn test_icmp_subtype_codes_match_inet_icmp() {
        // BATADV ICMP reuses ICMPv4 sub-type numbering.
        assert_eq!(BATADV_ECHO_REPLY, 0);
        assert_eq!(BATADV_DESTINATION_UNREACHABLE, 3);
        assert_eq!(BATADV_ECHO_REQUEST, 8);
        assert_eq!(BATADV_TTL_EXCEEDED, 11);
        assert_eq!(BATADV_PARAMETER_PROBLEM, 12);
        // BATADV adds throughput-probe sub-type 15.
        assert_eq!(BATADV_TP, 15);
    }

    #[test]
    fn test_tvlv_ids_dense_1_to_7() {
        let t = [
            BATADV_TVLV_GW,
            BATADV_TVLV_DAT,
            BATADV_TVLV_NC,
            BATADV_TVLV_TT,
            BATADV_TVLV_ROAM,
            BATADV_TVLV_MCAST,
            BATADV_TVLV_MCAST_TRACKER,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, 1 + i);
        }
    }
}
