//! `<linux/vxlan.h>` — VXLAN tunnel constants (extended).
//!
//! Extended VXLAN constants covering netlink attributes,
//! flags, VNI limits, and forwarding database parameters.

// ---------------------------------------------------------------------------
// VXLAN netlink attribute types (IFLA_VXLAN_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_VXLAN_UNSPEC: u32 = 0;
/// VNI (VXLAN Network Identifier).
pub const IFLA_VXLAN_ID: u32 = 1;
/// Multicast group (IPv4).
pub const IFLA_VXLAN_GROUP: u32 = 2;
/// Link (parent interface).
pub const IFLA_VXLAN_LINK: u32 = 3;
/// Local IP address.
pub const IFLA_VXLAN_LOCAL: u32 = 4;
/// TTL.
pub const IFLA_VXLAN_TTL: u32 = 5;
/// TOS.
pub const IFLA_VXLAN_TOS: u32 = 6;
/// Learning mode.
pub const IFLA_VXLAN_LEARNING: u32 = 7;
/// Aging timer (seconds).
pub const IFLA_VXLAN_AGEING: u32 = 8;
/// Max FDB entries.
pub const IFLA_VXLAN_LIMIT: u32 = 9;
/// Port range.
pub const IFLA_VXLAN_PORT_RANGE: u32 = 10;
/// Proxy ARP.
pub const IFLA_VXLAN_PROXY: u32 = 11;
/// RSC (Route Short Circuit).
pub const IFLA_VXLAN_RSC: u32 = 12;
/// L2miss notification.
pub const IFLA_VXLAN_L2MISS: u32 = 13;
/// L3miss notification.
pub const IFLA_VXLAN_L3MISS: u32 = 14;
/// Destination port.
pub const IFLA_VXLAN_PORT: u32 = 15;
/// Multicast group (IPv6).
pub const IFLA_VXLAN_GROUP6: u32 = 16;
/// Local IP (IPv6).
pub const IFLA_VXLAN_LOCAL6: u32 = 17;
/// UDP checksum.
pub const IFLA_VXLAN_UDP_CSUM: u32 = 18;
/// UDP zero checksum (TX IPv6).
pub const IFLA_VXLAN_UDP_ZERO_CSUM6_TX: u32 = 19;
/// UDP zero checksum (RX IPv6).
pub const IFLA_VXLAN_UDP_ZERO_CSUM6_RX: u32 = 20;
/// Remote checksum TX.
pub const IFLA_VXLAN_REMCSUM_TX: u32 = 21;
/// Remote checksum RX.
pub const IFLA_VXLAN_REMCSUM_RX: u32 = 22;
/// GBP (Group-Based Policy).
pub const IFLA_VXLAN_GBP: u32 = 23;
/// Remote checksum no-partial.
pub const IFLA_VXLAN_REMCSUM_NOPARTIAL: u32 = 24;
/// Collect metadata.
pub const IFLA_VXLAN_COLLECT_METADATA: u32 = 25;
/// Label (flow label).
pub const IFLA_VXLAN_LABEL: u32 = 26;
/// GPE (Generic Protocol Extension).
pub const IFLA_VXLAN_GPE: u32 = 27;
/// TTL inherit.
pub const IFLA_VXLAN_TTL_INHERIT: u32 = 28;
/// DF (don't fragment).
pub const IFLA_VXLAN_DF: u32 = 29;

// ---------------------------------------------------------------------------
// VXLAN default port
// ---------------------------------------------------------------------------

/// Default VXLAN UDP port.
pub const VXLAN_UDP_PORT: u16 = 4789;
/// Legacy VXLAN port (used by some implementations).
pub const VXLAN_UDP_PORT_LEGACY: u16 = 8472;

// ---------------------------------------------------------------------------
// VXLAN VNI
// ---------------------------------------------------------------------------

/// Max VNI (24-bit field).
pub const VXLAN_VNI_MAX: u32 = 0x00FFFFFF;

// ---------------------------------------------------------------------------
// VXLAN DF modes
// ---------------------------------------------------------------------------

/// Unset.
pub const VXLAN_DF_UNSET: u32 = 0;
/// Set DF.
pub const VXLAN_DF_SET: u32 = 1;
/// Inherit from inner.
pub const VXLAN_DF_INHERIT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            IFLA_VXLAN_UNSPEC,
            IFLA_VXLAN_ID,
            IFLA_VXLAN_GROUP,
            IFLA_VXLAN_LINK,
            IFLA_VXLAN_LOCAL,
            IFLA_VXLAN_TTL,
            IFLA_VXLAN_TOS,
            IFLA_VXLAN_LEARNING,
            IFLA_VXLAN_AGEING,
            IFLA_VXLAN_LIMIT,
            IFLA_VXLAN_PORT_RANGE,
            IFLA_VXLAN_PROXY,
            IFLA_VXLAN_RSC,
            IFLA_VXLAN_L2MISS,
            IFLA_VXLAN_L3MISS,
            IFLA_VXLAN_PORT,
            IFLA_VXLAN_GROUP6,
            IFLA_VXLAN_LOCAL6,
            IFLA_VXLAN_UDP_CSUM,
            IFLA_VXLAN_UDP_ZERO_CSUM6_TX,
            IFLA_VXLAN_UDP_ZERO_CSUM6_RX,
            IFLA_VXLAN_REMCSUM_TX,
            IFLA_VXLAN_REMCSUM_RX,
            IFLA_VXLAN_GBP,
            IFLA_VXLAN_REMCSUM_NOPARTIAL,
            IFLA_VXLAN_COLLECT_METADATA,
            IFLA_VXLAN_LABEL,
            IFLA_VXLAN_GPE,
            IFLA_VXLAN_TTL_INHERIT,
            IFLA_VXLAN_DF,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_default_port() {
        assert_eq!(VXLAN_UDP_PORT, 4789);
    }

    #[test]
    fn test_legacy_port() {
        assert_eq!(VXLAN_UDP_PORT_LEGACY, 8472);
    }

    #[test]
    fn test_vni_max() {
        assert_eq!(VXLAN_VNI_MAX, 0x00FFFFFF);
    }

    #[test]
    fn test_df_modes_distinct() {
        let modes = [VXLAN_DF_UNSET, VXLAN_DF_SET, VXLAN_DF_INHERIT];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(IFLA_VXLAN_UNSPEC, 0);
    }
}
