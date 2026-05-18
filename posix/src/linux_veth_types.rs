//! `<linux/veth.h>` — Virtual Ethernet (veth) pair constants.
//!
//! Veth devices are virtual Ethernet pairs commonly used in
//! network namespaces and containers.  These constants define
//! veth netlink attribute types and peer configuration.

// ---------------------------------------------------------------------------
// Veth netlink attribute types (VETH_INFO_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const VETH_INFO_UNSPEC: u32 = 0;
/// Peer info (nested attributes for peer device).
pub const VETH_INFO_PEER: u32 = 1;

// ---------------------------------------------------------------------------
// Veth-related interface flags (subset of IFLA_*)
// ---------------------------------------------------------------------------

/// Unspecified link attribute.
pub const IFLA_VETH_UNSPEC: u32 = 0;
/// Interface name.
pub const IFLA_VETH_IFNAME: u32 = 1;
/// MTU.
pub const IFLA_VETH_MTU: u32 = 2;
/// TX queue length.
pub const IFLA_VETH_TXQUEUELEN: u32 = 3;
/// Namespace ID.
pub const IFLA_VETH_NET_NS_PID: u32 = 4;
/// Namespace FD.
pub const IFLA_VETH_NET_NS_FD: u32 = 5;
/// Namespace by name.
pub const IFLA_VETH_TARGET_NETNSID: u32 = 6;

// ---------------------------------------------------------------------------
// Default veth parameters
// ---------------------------------------------------------------------------

/// Default MTU for veth.
pub const VETH_DEFAULT_MTU: u32 = 1500;
/// Maximum MTU for veth.
pub const VETH_MAX_MTU: u32 = 65535;
/// Default TX queue length.
pub const VETH_DEFAULT_TXQLEN: u32 = 1000;

// ---------------------------------------------------------------------------
// Veth GSO (Generic Segmentation Offload) features
// ---------------------------------------------------------------------------

/// GSO software segmentation.
pub const VETH_GSO_SW: u32 = 1 << 0;
/// GSO hardware.
pub const VETH_GSO_HW: u32 = 1 << 1;
/// GRO (Generic Receive Offload).
pub const VETH_GRO: u32 = 1 << 2;
/// XDP support.
pub const VETH_XDP: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_distinct() {
        assert_ne!(VETH_INFO_UNSPEC, VETH_INFO_PEER);
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(VETH_INFO_UNSPEC, 0);
    }

    #[test]
    fn test_ifla_attrs_distinct() {
        let attrs = [
            IFLA_VETH_UNSPEC, IFLA_VETH_IFNAME,
            IFLA_VETH_MTU, IFLA_VETH_TXQUEUELEN,
            IFLA_VETH_NET_NS_PID, IFLA_VETH_NET_NS_FD,
            IFLA_VETH_TARGET_NETNSID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_default_mtu() {
        assert_eq!(VETH_DEFAULT_MTU, 1500);
    }

    #[test]
    fn test_max_mtu_larger() {
        assert!(VETH_MAX_MTU > VETH_DEFAULT_MTU);
    }

    #[test]
    fn test_gso_features_powers_of_two() {
        let feats = [VETH_GSO_SW, VETH_GSO_HW, VETH_GRO, VETH_XDP];
        for f in &feats {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_gso_features_no_overlap() {
        let feats = [VETH_GSO_SW, VETH_GSO_HW, VETH_GRO, VETH_XDP];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_default_txqlen() {
        assert_eq!(VETH_DEFAULT_TXQLEN, 1000);
    }

    #[test]
    fn test_peer_is_one() {
        assert_eq!(VETH_INFO_PEER, 1);
    }
}
