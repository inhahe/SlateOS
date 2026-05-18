//! `<linux/geneve.h>` — GENEVE tunnel constants.
//!
//! GENEVE (Generic Network Virtualization Encapsulation) is
//! a network overlay protocol.  These constants define GENEVE
//! netlink attributes, option types, and header fields.

// ---------------------------------------------------------------------------
// GENEVE netlink attribute types (IFLA_GENEVE_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_GENEVE_UNSPEC: u32 = 0;
/// VNI (Virtual Network Identifier).
pub const IFLA_GENEVE_ID: u32 = 1;
/// Remote IPv4 address.
pub const IFLA_GENEVE_REMOTE: u32 = 2;
/// TTL (Time to Live).
pub const IFLA_GENEVE_TTL: u32 = 3;
/// TOS (Type of Service).
pub const IFLA_GENEVE_TOS: u32 = 4;
/// Destination port (UDP).
pub const IFLA_GENEVE_PORT: u32 = 5;
/// Collect metadata mode.
pub const IFLA_GENEVE_COLLECT_METADATA: u32 = 6;
/// Remote IPv6 address.
pub const IFLA_GENEVE_REMOTE6: u32 = 7;
/// UDP checksum.
pub const IFLA_GENEVE_UDP_CSUM: u32 = 8;
/// UDP zero checksum (TX for IPv6).
pub const IFLA_GENEVE_UDP_ZERO_CSUM6_TX: u32 = 9;
/// UDP zero checksum (RX for IPv6).
pub const IFLA_GENEVE_UDP_ZERO_CSUM6_RX: u32 = 10;
/// Label (flow label for IPv6).
pub const IFLA_GENEVE_LABEL: u32 = 11;
/// TTL inherit.
pub const IFLA_GENEVE_TTL_INHERIT: u32 = 12;
/// DF (don't fragment).
pub const IFLA_GENEVE_DF: u32 = 13;
/// Inner protocol info.
pub const IFLA_GENEVE_INNER_PROTO_INHERIT: u32 = 14;

// ---------------------------------------------------------------------------
// GENEVE default port
// ---------------------------------------------------------------------------

/// Default GENEVE UDP port.
pub const GENEVE_UDP_PORT: u16 = 6081;

// ---------------------------------------------------------------------------
// GENEVE header flags
// ---------------------------------------------------------------------------

/// Critical option present.
pub const GENEVE_CRIT_OPT_TYPE: u8 = 1 << 7;
/// OAM frame.
pub const GENEVE_F_OAM: u8 = 1 << 7;
/// Version mask (2 bits).
pub const GENEVE_VER_MASK: u8 = 0xC0;

// ---------------------------------------------------------------------------
// GENEVE option classes (well-known)
// ---------------------------------------------------------------------------

/// Linux-specific options.
pub const GENEVE_CLASS_LINUX: u16 = 0x0100;
/// Open vSwitch options.
pub const GENEVE_CLASS_OVS: u16 = 0x0102;
/// VMware options.
pub const GENEVE_CLASS_VMWARE: u16 = 0x0000;
/// Microsoft options.
pub const GENEVE_CLASS_MICROSOFT: u16 = 0x0104;

// ---------------------------------------------------------------------------
// GENEVE VNI range
// ---------------------------------------------------------------------------

/// Maximum VNI value (24-bit).
pub const GENEVE_VNI_MAX: u32 = 0x00FFFFFF;
/// VNI mask.
pub const GENEVE_VNI_MASK: u32 = 0x00FFFFFF;

// ---------------------------------------------------------------------------
// GENEVE DF modes
// ---------------------------------------------------------------------------

/// Unset (default).
pub const GENEVE_DF_UNSET: u32 = 0;
/// Set DF bit.
pub const GENEVE_DF_SET: u32 = 1;
/// Inherit DF from inner packet.
pub const GENEVE_DF_INHERIT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            IFLA_GENEVE_UNSPEC, IFLA_GENEVE_ID,
            IFLA_GENEVE_REMOTE, IFLA_GENEVE_TTL,
            IFLA_GENEVE_TOS, IFLA_GENEVE_PORT,
            IFLA_GENEVE_COLLECT_METADATA, IFLA_GENEVE_REMOTE6,
            IFLA_GENEVE_UDP_CSUM, IFLA_GENEVE_UDP_ZERO_CSUM6_TX,
            IFLA_GENEVE_UDP_ZERO_CSUM6_RX, IFLA_GENEVE_LABEL,
            IFLA_GENEVE_TTL_INHERIT, IFLA_GENEVE_DF,
            IFLA_GENEVE_INNER_PROTO_INHERIT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_default_port() {
        assert_eq!(GENEVE_UDP_PORT, 6081);
    }

    #[test]
    fn test_vni_max() {
        assert_eq!(GENEVE_VNI_MAX, 0x00FFFFFF);
    }

    #[test]
    fn test_vni_mask_matches_max() {
        assert_eq!(GENEVE_VNI_MASK, GENEVE_VNI_MAX);
    }

    #[test]
    fn test_option_classes_distinct() {
        let classes = [
            GENEVE_CLASS_LINUX, GENEVE_CLASS_OVS,
            GENEVE_CLASS_VMWARE, GENEVE_CLASS_MICROSOFT,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_df_modes_distinct() {
        let modes = [GENEVE_DF_UNSET, GENEVE_DF_SET, GENEVE_DF_INHERIT];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(IFLA_GENEVE_UNSPEC, 0);
    }

    #[test]
    fn test_df_unset_is_zero() {
        assert_eq!(GENEVE_DF_UNSET, 0);
    }
}
