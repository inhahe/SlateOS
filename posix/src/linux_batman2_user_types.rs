//! Legacy `batman` (non-Adv) on-wire packet format.
//!
//! The original BATMAN routing protocol (`batmand`, layer 3) predates
//! `batman-adv` and lived as a userspace daemon over UDP. This module
//! captures the wire constants needed by userspace tooling that still
//! interoperates with legacy nodes.

// ---------------------------------------------------------------------------
// UDP service ports (RFC-style allocation by openmesh.org)
// ---------------------------------------------------------------------------

/// Default port used by the legacy `batmand` daemon.
pub const BATMAND_DEFAULT_PORT: u16 = 4305;
/// Visualization daemon (`vis`) companion port.
pub const BATMAND_DEFAULT_VIS_PORT: u16 = 4307;
/// Gateway-tunnel UDP port.
pub const BATMAND_DEFAULT_GW_PORT: u16 = 4306;

// ---------------------------------------------------------------------------
// Packet header version field
// ---------------------------------------------------------------------------

/// Protocol version 0.3.x carried in the legacy header.
pub const BATMAND_VERSION: u8 = 5;

// ---------------------------------------------------------------------------
// Packet sub-types (`batman_packet.type`)
// ---------------------------------------------------------------------------

pub const BAT_PACKET: u8 = 0x00;
pub const BAT_ICMP: u8 = 0x01;
pub const BAT_UNICAST: u8 = 0x02;
pub const BAT_BCAST: u8 = 0x03;
pub const BAT_VIS: u8 = 0x04;
pub const BAT_UNICAST_FRAG: u8 = 0x05;

// ---------------------------------------------------------------------------
// Flag bits inside `batman_packet.flags`
// ---------------------------------------------------------------------------

pub const BATMAN_UNIDIRECTIONAL: u8 = 0x40;
pub const BATMAN_DIRECTLINK: u8 = 0x80;
pub const BATMAN_VIS_INFO_BCAST_SUB: u8 = 0x01;

// ---------------------------------------------------------------------------
// Default TTL and timing
// ---------------------------------------------------------------------------

pub const BATMAN_TTL: u8 = 50;
pub const BATMAN_OGM_INTERVAL_MS: u32 = 1_000;
pub const BATMAN_PURGE_TIMEOUT_MS: u32 = 200_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_ports_distinct_and_clustered() {
        // openmesh.org allocated 4305..4307 as a contiguous block.
        assert_eq!(BATMAND_DEFAULT_PORT, 4305);
        assert_eq!(BATMAND_DEFAULT_GW_PORT, 4306);
        assert_eq!(BATMAND_DEFAULT_VIS_PORT, 4307);
        assert_eq!(BATMAND_DEFAULT_GW_PORT - BATMAND_DEFAULT_PORT, 1);
        assert_eq!(BATMAND_DEFAULT_VIS_PORT - BATMAND_DEFAULT_GW_PORT, 1);
        // All in the registered-port range.
        for &v in &[
            BATMAND_DEFAULT_PORT,
            BATMAND_DEFAULT_GW_PORT,
            BATMAND_DEFAULT_VIS_PORT,
        ] {
            assert!((1024..49152).contains(&v));
        }
    }

    #[test]
    fn test_protocol_version() {
        // Final legacy version was 5 (batmand 0.3.x).
        assert_eq!(BATMAND_VERSION, 5);
    }

    #[test]
    fn test_packet_subtypes_dense_0_to_5() {
        let p = [
            BAT_PACKET,
            BAT_ICMP,
            BAT_UNICAST,
            BAT_BCAST,
            BAT_VIS,
            BAT_UNICAST_FRAG,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_flag_bits_in_high_nibble() {
        // DIRECTLINK is bit 7, UNIDIRECTIONAL bit 6 — they form the
        // two high flag bits of the byte.
        assert_eq!(BATMAN_DIRECTLINK, 0x80);
        assert_eq!(BATMAN_UNIDIRECTIONAL, 0x40);
        assert!(BATMAN_DIRECTLINK.is_power_of_two());
        assert!(BATMAN_UNIDIRECTIONAL.is_power_of_two());
        assert_eq!(BATMAN_DIRECTLINK & BATMAN_UNIDIRECTIONAL, 0);
        // VIS_INFO_BCAST_SUB lives in the low nibble and is disjoint
        // from the directional flags.
        assert_eq!(BATMAN_VIS_INFO_BCAST_SUB, 0x01);
        assert_eq!(BATMAN_VIS_INFO_BCAST_SUB & BATMAN_DIRECTLINK, 0);
        assert_eq!(BATMAN_VIS_INFO_BCAST_SUB & BATMAN_UNIDIRECTIONAL, 0);
    }

    #[test]
    fn test_ttl_and_timers() {
        // Default initial TTL on OGMs — sufficient for mesh diameters
        // up to ~50 hops.
        assert_eq!(BATMAN_TTL, 50);
        // OGM emitted every second; nodes purged after ~200s of silence.
        assert_eq!(BATMAN_OGM_INTERVAL_MS, 1_000);
        assert_eq!(BATMAN_PURGE_TIMEOUT_MS, 200_000);
        assert_eq!(
            BATMAN_PURGE_TIMEOUT_MS / BATMAN_OGM_INTERVAL_MS,
            200
        );
    }
}
