//! `<linux/geneve.h>` — Additional Geneve tunnel constants.
//!
//! Supplementary Geneve constants covering option types,
//! header fields, and tunnel flags.

// ---------------------------------------------------------------------------
// Geneve option classes
// ---------------------------------------------------------------------------

/// Linux option class.
pub const GENEVE_OPT_CLASS_LINUX: u16 = 0x0100;
/// Open vSwitch option class.
pub const GENEVE_OPT_CLASS_OVS: u16 = 0x0102;

// ---------------------------------------------------------------------------
// Geneve header constants
// ---------------------------------------------------------------------------

/// Geneve base header length (8 bytes).
pub const GENEVE_BASE_HLEN: u32 = 8;
/// Maximum options length (255 * 4 = 1020 bytes).
pub const GENEVE_MAX_OPT_LEN: u32 = 1020;
/// Maximum total header length.
pub const GENEVE_MAX_HLEN: u32 = 8 + 1020;
/// Option length granularity (4 bytes).
pub const GENEVE_OPT_LEN_UNIT: u32 = 4;

// ---------------------------------------------------------------------------
// Geneve option type flags
// ---------------------------------------------------------------------------

/// Critical option bit.
pub const GENEVE_OPT_TYPE_CRITICAL: u8 = 0x80;
/// Option type mask (lower 7 bits).
pub const GENEVE_OPT_TYPE_MASK: u8 = 0x7F;

// ---------------------------------------------------------------------------
// Geneve UDP port
// ---------------------------------------------------------------------------

/// Default Geneve UDP port.
pub const GENEVE_UDP_PORT: u16 = 6081;

// ---------------------------------------------------------------------------
// Geneve tunnel flags
// ---------------------------------------------------------------------------

/// Use IPv6 outer.
pub const GENEVE_F_IPV6: u32 = 1 << 0;
/// Collect metadata.
pub const GENEVE_F_COLLECT_METADATA: u32 = 1 << 1;
/// UDP checksum.
pub const GENEVE_F_UDP_CSUM: u32 = 1 << 2;
/// UDP zero checksum for RX.
pub const GENEVE_F_UDP_ZERO_CSUM6_RX: u32 = 1 << 3;
/// UDP zero checksum for TX.
pub const GENEVE_F_UDP_ZERO_CSUM6_TX: u32 = 1 << 4;
/// Inner protocol type ethernet.
pub const GENEVE_F_INNER_PROTO_INHERIT: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opt_classes_distinct() {
        assert_ne!(GENEVE_OPT_CLASS_LINUX, GENEVE_OPT_CLASS_OVS);
    }

    #[test]
    fn test_header_constants() {
        assert_eq!(GENEVE_BASE_HLEN, 8);
        assert_eq!(GENEVE_MAX_OPT_LEN, 1020);
        assert_eq!(GENEVE_MAX_HLEN, GENEVE_BASE_HLEN + GENEVE_MAX_OPT_LEN);
    }

    #[test]
    fn test_opt_type_masks() {
        assert_eq!(GENEVE_OPT_TYPE_CRITICAL | GENEVE_OPT_TYPE_MASK, 0xFF);
        assert_eq!(GENEVE_OPT_TYPE_CRITICAL & GENEVE_OPT_TYPE_MASK, 0);
    }

    #[test]
    fn test_udp_port() {
        assert_eq!(GENEVE_UDP_PORT, 6081);
    }

    #[test]
    fn test_tunnel_flags_power_of_two() {
        let flags = [
            GENEVE_F_IPV6, GENEVE_F_COLLECT_METADATA,
            GENEVE_F_UDP_CSUM, GENEVE_F_UDP_ZERO_CSUM6_RX,
            GENEVE_F_UDP_ZERO_CSUM6_TX, GENEVE_F_INNER_PROTO_INHERIT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_tunnel_flags_no_overlap() {
        let flags = [
            GENEVE_F_IPV6, GENEVE_F_COLLECT_METADATA,
            GENEVE_F_UDP_CSUM, GENEVE_F_UDP_ZERO_CSUM6_RX,
            GENEVE_F_UDP_ZERO_CSUM6_TX, GENEVE_F_INNER_PROTO_INHERIT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
