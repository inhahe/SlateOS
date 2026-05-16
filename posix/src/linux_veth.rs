//! Linux veth (virtual Ethernet pair) constants.
//!
//! veth devices are always created in pairs: packets sent to one
//! end appear on the other. Used to connect network namespaces
//! (container networking), bridge ports, and testing scenarios.

// ---------------------------------------------------------------------------
// Netlink attributes for veth creation
// ---------------------------------------------------------------------------

/// Veth info (container for peer attributes).
pub const VETH_INFO_UNSPEC: u16 = 0;
/// Peer device info.
pub const VETH_INFO_PEER: u16 = 1;

// ---------------------------------------------------------------------------
// Link type
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND value for veth.
pub const VETH_KIND: &str = "veth";

// ---------------------------------------------------------------------------
// Default parameters
// ---------------------------------------------------------------------------

/// Default MTU for veth pairs (same as Ethernet).
pub const VETH_DEFAULT_MTU: u32 = 1500;

/// Default TX queue length.
pub const VETH_DEFAULT_TXQUEUELEN: u32 = 1000;

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

/// Veth supports GSO (Generic Segmentation Offload).
pub const VETH_FEATURE_GSO: u32 = 1 << 0;
/// Veth supports GRO (Generic Receive Offload).
pub const VETH_FEATURE_GRO: u32 = 1 << 1;
/// Veth supports XDP.
pub const VETH_FEATURE_XDP: u32 = 1 << 2;
/// Veth supports peer XDP redirect.
pub const VETH_FEATURE_XDP_REDIRECT: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// XDP modes for veth
// ---------------------------------------------------------------------------

/// Native XDP on veth (since Linux 5.4).
pub const VETH_XDP_NATIVE: u32 = 1;
/// XDP redirect between veth pairs.
pub const VETH_XDP_REDIRECT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_attrs_distinct() {
        assert_ne!(VETH_INFO_UNSPEC, VETH_INFO_PEER);
    }

    #[test]
    fn test_kind() {
        assert_eq!(VETH_KIND, "veth");
    }

    #[test]
    fn test_default_mtu() {
        assert_eq!(VETH_DEFAULT_MTU, 1500);
    }

    #[test]
    fn test_features_powers_of_two() {
        let features = [
            VETH_FEATURE_GSO, VETH_FEATURE_GRO,
            VETH_FEATURE_XDP, VETH_FEATURE_XDP_REDIRECT,
        ];
        for f in &features {
            assert!(f.is_power_of_two(), "0x{:x}", f);
        }
    }

    #[test]
    fn test_features_no_overlap() {
        let features = [
            VETH_FEATURE_GSO, VETH_FEATURE_GRO,
            VETH_FEATURE_XDP, VETH_FEATURE_XDP_REDIRECT,
        ];
        for i in 0..features.len() {
            for j in (i + 1)..features.len() {
                assert_eq!(features[i] & features[j], 0);
            }
        }
    }

    #[test]
    fn test_xdp_modes_distinct() {
        assert_ne!(VETH_XDP_NATIVE, VETH_XDP_REDIRECT);
    }
}
