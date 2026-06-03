//! `<linux/mpls.h>` — Multiprotocol Label Switching ABI.
//!
//! MPLS adds 4-byte labels between the L2 header and the L3 payload —
//! used by ISP backbones and data-centre fabrics for fast forwarding
//! without per-packet routing-table lookups. Linux added native MPLS
//! support in 4.1; `iproute2`'s `ip -family mpls route` and FRR/BIRD
//! configure label-switched paths via the netlink ABI below.

// ---------------------------------------------------------------------------
// Address family / EtherType
// ---------------------------------------------------------------------------

/// `AF_MPLS` — Linux 4.1+.
pub const AF_MPLS: u32 = 28;
/// Unicast MPLS EtherType.
pub const ETH_P_MPLS_UC: u16 = 0x8847;
/// Multicast MPLS EtherType.
pub const ETH_P_MPLS_MC: u16 = 0x8848;

// ---------------------------------------------------------------------------
// Reserved label values (RFC 3032)
// ---------------------------------------------------------------------------

/// IPv4 Explicit NULL label.
pub const MPLS_LABEL_IPV4NULL: u32 = 0;
/// Router Alert label.
pub const MPLS_LABEL_RTALERT: u32 = 1;
/// IPv6 Explicit NULL label.
pub const MPLS_LABEL_IPV6NULL: u32 = 2;
/// Implicit NULL label — penultimate hop popping.
pub const MPLS_LABEL_IMPLNULL: u32 = 3;
/// Entropy Label Indicator (RFC 6790).
pub const MPLS_LABEL_ENTROPY: u32 = 7;
/// Generic Associated Channel Label.
pub const MPLS_LABEL_GAL: u32 = 13;
/// OAM Alert label (RFC 7026).
pub const MPLS_LABEL_OAMALERT: u32 = 14;
/// Extension label (RFC 7274).
pub const MPLS_LABEL_EXTENSION: u32 = 15;

/// First label available for general use.
pub const MPLS_LABEL_FIRST_UNRESERVED: u32 = 16;
/// Maximum label value (20-bit field).
pub const MPLS_LABEL_MAX: u32 = (1 << 20) - 1;

// ---------------------------------------------------------------------------
// On-the-wire MPLS shim header layout
// ---------------------------------------------------------------------------

/// MPLS label stack entry is 4 bytes (label:20, TC:3, S:1, TTL:8).
pub const MPLS_HLEN: usize = 4;

/// Bit position of the Bottom-of-Stack indicator within a 32-bit big-endian shim.
pub const MPLS_LS_S_SHIFT: u32 = 8;
/// Bit position of the Traffic Class field.
pub const MPLS_LS_TC_SHIFT: u32 = 9;
/// Bit position of the Label field.
pub const MPLS_LS_LABEL_SHIFT: u32 = 12;

// ---------------------------------------------------------------------------
// netlink (`RTM_NEWROUTE` etc.) attributes
// ---------------------------------------------------------------------------

pub const MPLS_IPTUNNEL_UNSPEC: u32 = 0;
pub const MPLS_IPTUNNEL_DST: u32 = 1;
pub const MPLS_IPTUNNEL_TTL: u32 = 2;

// ---------------------------------------------------------------------------
// `sysctl` interface
// ---------------------------------------------------------------------------

pub const SYSCTL_MPLS_PLATFORM_LABELS: &str = "/proc/sys/net/mpls/platform_labels";
pub const SYSCTL_MPLS_DEFAULT_TTL: &str = "/proc/sys/net/mpls/default_ttl";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_and_ethertype() {
        assert_eq!(AF_MPLS, 28);
        assert_eq!(ETH_P_MPLS_UC, 0x8847);
        assert_eq!(ETH_P_MPLS_MC, 0x8848);
        // Multicast EtherType is one above unicast.
        assert_eq!(ETH_P_MPLS_MC - ETH_P_MPLS_UC, 1);
    }

    #[test]
    fn test_reserved_labels_distinct_and_below_16() {
        let r = [
            MPLS_LABEL_IPV4NULL,
            MPLS_LABEL_RTALERT,
            MPLS_LABEL_IPV6NULL,
            MPLS_LABEL_IMPLNULL,
            MPLS_LABEL_ENTROPY,
            MPLS_LABEL_GAL,
            MPLS_LABEL_OAMALERT,
            MPLS_LABEL_EXTENSION,
        ];
        for i in 0..r.len() {
            // Reserved labels are the IANA "Special-Purpose Label" block 0..15.
            assert!(r[i] < MPLS_LABEL_FIRST_UNRESERVED);
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
    }

    #[test]
    fn test_label_field_is_20_bit() {
        assert_eq!(MPLS_LABEL_MAX, 0xF_FFFF);
        assert_eq!(MPLS_LABEL_FIRST_UNRESERVED, 16);
    }

    #[test]
    fn test_shim_field_offsets() {
        // S (bottom-of-stack) is bit 8 from the LSB end of the 32-bit shim.
        assert_eq!(MPLS_LS_S_SHIFT, 8);
        // TC follows S in the next 3 bits.
        assert_eq!(MPLS_LS_TC_SHIFT, 9);
        // Label occupies the top 20 bits.
        assert_eq!(MPLS_LS_LABEL_SHIFT, 12);
        // The 20-bit label field fits within 32 bits when shifted.
        assert!(MPLS_LS_LABEL_SHIFT + 20 == 32);
        // 4-byte on-wire size.
        assert_eq!(MPLS_HLEN, 4);
    }

    #[test]
    fn test_iptunnel_attributes_dense() {
        let a = [MPLS_IPTUNNEL_UNSPEC, MPLS_IPTUNNEL_DST, MPLS_IPTUNNEL_TTL];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
