//! `<linux/if_link.h>` (`IFLA_VXLAN_*`) — VXLAN tunnel ABI.
//!
//! VXLAN encapsulates Ethernet frames in UDP, with a 24-bit VNI to
//! identify the overlay network. It is how cloud providers and
//! Kubernetes CNI plugins (Flannel, Calico VXLAN mode) build L2
//! overlays across L3 fabrics.

// ---------------------------------------------------------------------------
// Wire-level constants
// ---------------------------------------------------------------------------

/// IANA-assigned UDP port for VXLAN (RFC 7348).
pub const VXLAN_UDP_PORT_IANA: u16 = 4789;

/// Linux's historical default port (pre-IANA assignment, still seen
/// in legacy deployments).
pub const VXLAN_UDP_PORT_LINUX_LEGACY: u16 = 8472;

/// VXLAN header is 8 bytes: 4-byte flags + 4-byte VNI(24)/reserved(8).
pub const VXLAN_HDR_LEN: usize = 8;

/// 24-bit VNI space → 16,777,216 distinct overlay networks.
pub const VXLAN_N_VID: u32 = 1 << 24;

/// Flag bit in the VXLAN header byte 0 — "VNI present".
pub const VXLAN_FLAG_I: u8 = 0x08;

/// Mask for the 24-bit VNI when packed in the 32-bit VNI/reserved word.
pub const VXLAN_VNI_MASK: u32 = 0x00FF_FFFF;

// ---------------------------------------------------------------------------
// rtnetlink link kind
// ---------------------------------------------------------------------------

pub const VXLAN_KIND: &str = "vxlan";

// ---------------------------------------------------------------------------
// `IFLA_VXLAN_*` netlink attributes (subset)
// ---------------------------------------------------------------------------

pub const IFLA_VXLAN_UNSPEC: u16 = 0;
pub const IFLA_VXLAN_ID: u16 = 1;
pub const IFLA_VXLAN_GROUP: u16 = 2;
pub const IFLA_VXLAN_LINK: u16 = 3;
pub const IFLA_VXLAN_LOCAL: u16 = 4;
pub const IFLA_VXLAN_TTL: u16 = 5;
pub const IFLA_VXLAN_TOS: u16 = 6;
pub const IFLA_VXLAN_LEARNING: u16 = 7;
pub const IFLA_VXLAN_AGEING: u16 = 8;
pub const IFLA_VXLAN_LIMIT: u16 = 9;
pub const IFLA_VXLAN_PORT_RANGE: u16 = 10;
pub const IFLA_VXLAN_PROXY: u16 = 11;
pub const IFLA_VXLAN_RSC: u16 = 12;
pub const IFLA_VXLAN_L2MISS: u16 = 13;
pub const IFLA_VXLAN_L3MISS: u16 = 14;
pub const IFLA_VXLAN_PORT: u16 = 15;

// ---------------------------------------------------------------------------
// TTL defaults
// ---------------------------------------------------------------------------

/// 0 means "inherit from inner packet".
pub const VXLAN_TTL_INHERIT: u8 = 0;
/// The standard recommended TTL is 64.
pub const VXLAN_TTL_DEFAULT: u8 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iana_port_and_legacy_port() {
        // RFC 7348 assigned 4789; Linux shipped earlier with 8472 and
        // still supports it for compatibility.
        assert_eq!(VXLAN_UDP_PORT_IANA, 4789);
        assert_eq!(VXLAN_UDP_PORT_LINUX_LEGACY, 8472);
        assert_ne!(VXLAN_UDP_PORT_IANA, VXLAN_UDP_PORT_LINUX_LEGACY);
    }

    #[test]
    fn test_header_size_and_flag_bit() {
        // 4 bytes flags + 4 bytes VNI/reserved.
        assert_eq!(VXLAN_HDR_LEN, 8);
        // I-flag bit is byte 0 bit 3.
        assert_eq!(VXLAN_FLAG_I, 1 << 3);
    }

    #[test]
    fn test_vni_space_24_bit() {
        // VNI is 24 bits → 2^24 values, mask covers the low 24 bits.
        assert_eq!(VXLAN_N_VID, 16_777_216);
        assert_eq!(VXLAN_VNI_MASK, 0x00FF_FFFF);
        assert_eq!(VXLAN_VNI_MASK + 1, VXLAN_N_VID);
    }

    #[test]
    fn test_ifla_attrs_dense_0_to_15() {
        let a = [
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
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_ttl_defaults() {
        assert_eq!(VXLAN_TTL_INHERIT, 0);
        assert_eq!(VXLAN_TTL_DEFAULT, 64);
    }

    #[test]
    fn test_kind_string() {
        assert_eq!(VXLAN_KIND, "vxlan");
    }
}
