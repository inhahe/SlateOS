//! `<linux/geneve.h>` — GENEVE tunnel constants.
//!
//! GENEVE (Generic Network Virtualization Encapsulation) is a
//! flexible network overlay protocol. It encapsulates inner
//! Ethernet frames in UDP with a variable-length option header,
//! supporting multiple virtual networks (VNIs) over a shared
//! physical infrastructure.

// ---------------------------------------------------------------------------
// GENEVE header
// ---------------------------------------------------------------------------

/// GENEVE UDP destination port (standard).
pub const GENEVE_UDP_PORT: u16 = 6081;
/// GENEVE header base size (without options, bytes).
pub const GENEVE_BASE_HLEN: usize = 8;
/// Maximum options length (252 bytes, 63 * 4-byte words).
pub const GENEVE_MAX_OPT_LEN: usize = 252;

// ---------------------------------------------------------------------------
// GENEVE VNI (Virtual Network Identifier)
// ---------------------------------------------------------------------------

/// VNI field is 24 bits.
pub const GENEVE_VNI_BITS: u32 = 24;
/// Maximum VNI value.
pub const GENEVE_VNI_MAX: u32 = (1 << GENEVE_VNI_BITS) - 1;

// ---------------------------------------------------------------------------
// GENEVE header flags
// ---------------------------------------------------------------------------

/// Critical options present (receiver must understand them).
pub const GENEVE_F_CRITICAL: u8 = 1 << 2;
/// OAM (Operations/Admin/Maintenance) packet.
pub const GENEVE_F_OAM: u8 = 1 << 7;

// ---------------------------------------------------------------------------
// Option classes
// ---------------------------------------------------------------------------

/// Linux-specific options.
pub const GENEVE_OPT_CLASS_LINUX: u16 = 0x0100;
/// Open vSwitch options.
pub const GENEVE_OPT_CLASS_OVS: u16 = 0x0102;

// ---------------------------------------------------------------------------
// Netlink attributes (IFLA_GENEVE_*)
// ---------------------------------------------------------------------------

/// Tunnel ID (VNI).
pub const IFLA_GENEVE_ID: u16 = 1;
/// Remote IPv4 address.
pub const IFLA_GENEVE_REMOTE: u16 = 2;
/// TTL.
pub const IFLA_GENEVE_TTL: u16 = 3;
/// TOS.
pub const IFLA_GENEVE_TOS: u16 = 4;
/// Destination port.
pub const IFLA_GENEVE_PORT: u16 = 5;
/// Collect metadata mode.
pub const IFLA_GENEVE_COLLECT_METADATA: u16 = 6;
/// Remote IPv6 address.
pub const IFLA_GENEVE_REMOTE6: u16 = 7;
/// UDP checksum.
pub const IFLA_GENEVE_UDP_CSUM: u16 = 8;

// ---------------------------------------------------------------------------
// Link type
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND for geneve.
pub const GENEVE_KIND: &str = "geneve";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_port() {
        assert_eq!(GENEVE_UDP_PORT, 6081);
    }

    #[test]
    fn test_vni_max() {
        assert_eq!(GENEVE_VNI_MAX, 0x00FFFFFF);
    }

    #[test]
    fn test_header_flags_distinct() {
        assert_ne!(GENEVE_F_CRITICAL, GENEVE_F_OAM);
        assert_eq!(GENEVE_F_CRITICAL & GENEVE_F_OAM, 0);
    }

    #[test]
    fn test_opt_classes_distinct() {
        assert_ne!(GENEVE_OPT_CLASS_LINUX, GENEVE_OPT_CLASS_OVS);
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            IFLA_GENEVE_ID,
            IFLA_GENEVE_REMOTE,
            IFLA_GENEVE_TTL,
            IFLA_GENEVE_TOS,
            IFLA_GENEVE_PORT,
            IFLA_GENEVE_COLLECT_METADATA,
            IFLA_GENEVE_REMOTE6,
            IFLA_GENEVE_UDP_CSUM,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_base_hlen() {
        assert_eq!(GENEVE_BASE_HLEN, 8);
    }
}
