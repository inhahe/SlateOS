//! `<linux/bareudp.h>` — "bare UDP" tunnel netlink attributes.
//!
//! BareUDP carries arbitrary L3 protocols inside a UDP envelope without
//! any further header (cf. GENEVE, VXLAN). It is configured purely via
//! rtnetlink `IFLA_INFO_DATA` attributes — there are no ioctls.

// ---------------------------------------------------------------------------
// netlink attribute identifiers (`enum ifla_bareudp_attrs`)
// ---------------------------------------------------------------------------

pub const IFLA_BAREUDP_UNSPEC: u32 = 0;
pub const IFLA_BAREUDP_PORT: u32 = 1;
pub const IFLA_BAREUDP_ETHERTYPE: u32 = 2;
pub const IFLA_BAREUDP_SRCPORT_MIN: u32 = 3;
pub const IFLA_BAREUDP_MULTIPROTO_MODE: u32 = 4;
pub const __IFLA_BAREUDP_MAX: u32 = 5;

/// Highest defined attribute (inclusive).
pub const IFLA_BAREUDP_MAX: u32 = __IFLA_BAREUDP_MAX - 1;

// ---------------------------------------------------------------------------
// IANA-allocated default ports
// ---------------------------------------------------------------------------

/// Default UDP port for MPLS-in-UDP (RFC 7510).
pub const BAREUDP_MPLS_PORT: u16 = 6635;

/// Default UDP port for ETH-over-UDP (Linux convention).
pub const BAREUDP_ETH_PORT: u16 = 6636;

// ---------------------------------------------------------------------------
// Driver-name / sysfs identifier
// ---------------------------------------------------------------------------

pub const BAREUDP_KIND: &str = "bareudp";

// ---------------------------------------------------------------------------
// EtherType values that a BareUDP tunnel typically carries
// ---------------------------------------------------------------------------

pub const ETH_P_MPLS_UC: u16 = 0x8847;
pub const ETH_P_MPLS_MC: u16 = 0x8848;
pub const ETH_P_IP: u16 = 0x0800;
pub const ETH_P_IPV6: u16 = 0x86DD;

// ---------------------------------------------------------------------------
// Source-port randomisation window
// ---------------------------------------------------------------------------

/// Default lowest UDP source port (kernel default when SRCPORT_MIN unset).
pub const BAREUDP_DEFAULT_SRCPORT_MIN: u16 = 12_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_dense_0_to_4_with_max() {
        let a = [
            IFLA_BAREUDP_UNSPEC,
            IFLA_BAREUDP_PORT,
            IFLA_BAREUDP_ETHERTYPE,
            IFLA_BAREUDP_SRCPORT_MIN,
            IFLA_BAREUDP_MULTIPROTO_MODE,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // The sentinel _MAX is one past the last valid attribute.
        assert_eq!(__IFLA_BAREUDP_MAX, a.len() as u32);
        assert_eq!(IFLA_BAREUDP_MAX, __IFLA_BAREUDP_MAX - 1);
        assert_eq!(IFLA_BAREUDP_MAX, IFLA_BAREUDP_MULTIPROTO_MODE);
    }

    #[test]
    fn test_default_ports_adjacent() {
        // IANA allocated 6635 for MPLS-in-UDP; Linux picks 6636 for ETH.
        assert_eq!(BAREUDP_MPLS_PORT, 6635);
        assert_eq!(BAREUDP_ETH_PORT, 6636);
        assert_eq!(BAREUDP_ETH_PORT - BAREUDP_MPLS_PORT, 1);
        // Both ports are well above the well-known/system range.
        assert!(BAREUDP_MPLS_PORT > 1023);
        assert!(BAREUDP_ETH_PORT > 1023);
    }

    #[test]
    fn test_driver_name() {
        assert_eq!(BAREUDP_KIND, "bareudp");
        assert!(BAREUDP_KIND.bytes().all(|b| b.is_ascii_lowercase()));
    }

    #[test]
    fn test_ethertype_values_match_iana() {
        // IEEE assignments.
        assert_eq!(ETH_P_IP, 0x0800);
        assert_eq!(ETH_P_IPV6, 0x86DD);
        assert_eq!(ETH_P_MPLS_UC, 0x8847);
        assert_eq!(ETH_P_MPLS_MC, 0x8848);
        // MPLS uc/mc form a consecutive pair.
        assert_eq!(ETH_P_MPLS_MC - ETH_P_MPLS_UC, 1);
        // All EtherTypes ≥ 0x0600 (per IEEE 802 length-vs-type rule).
        for &v in &[ETH_P_IP, ETH_P_IPV6, ETH_P_MPLS_UC, ETH_P_MPLS_MC] {
            assert!(v >= 0x0600);
        }
    }

    #[test]
    fn test_default_srcport_min_in_user_range() {
        // Default lowest UDP source port for entropy hashing — sits
        // safely above the well-known/system range (<1024) and well
        // below the dynamic/ephemeral start (49152).
        assert_eq!(BAREUDP_DEFAULT_SRCPORT_MIN, 12_000);
        assert!(BAREUDP_DEFAULT_SRCPORT_MIN > 1024);
        assert!(BAREUDP_DEFAULT_SRCPORT_MIN < 49_152);
    }
}
