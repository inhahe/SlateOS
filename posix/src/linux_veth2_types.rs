//! `<linux/veth.h>` — Additional veth constants.
//!
//! Supplementary veth (virtual ethernet pair) constants covering
//! netlink attributes and XDP flags.

// ---------------------------------------------------------------------------
// Veth netlink attributes (VETH_INFO_*)
// ---------------------------------------------------------------------------

/// Unspec attribute.
pub const VETH_INFO_UNSPEC: u32 = 0;
/// Peer information.
pub const VETH_INFO_PEER: u32 = 1;

// ---------------------------------------------------------------------------
// Veth XDP flags
// ---------------------------------------------------------------------------

/// XDP: native mode.
pub const VETH_XDP_NATIVE: u32 = 1 << 0;
/// XDP: generic (SKB) mode.
pub const VETH_XDP_GENERIC: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Veth features
// ---------------------------------------------------------------------------

/// GSO (Generic Segmentation Offload) maximum size.
pub const VETH_GSO_MAX_SIZE: u32 = 65536;
/// GRO (Generic Receive Offload) max list length.
pub const VETH_GRO_MAX_SIZE: u32 = 65536;
/// Default TX queue length.
pub const VETH_TX_QUEUE_LEN: u32 = 1000;
/// Default number of TX queues.
pub const VETH_NUM_TX_QUEUES: u32 = 1;
/// Default number of RX queues.
pub const VETH_NUM_RX_QUEUES: u32 = 1;
/// Maximum MTU.
pub const VETH_MAX_MTU: u32 = 65535;
/// Minimum MTU.
pub const VETH_MIN_MTU: u32 = 68;

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
    fn test_xdp_flags_power_of_two() {
        assert!(VETH_XDP_NATIVE.is_power_of_two());
        assert!(VETH_XDP_GENERIC.is_power_of_two());
    }

    #[test]
    fn test_xdp_flags_no_overlap() {
        assert_eq!(VETH_XDP_NATIVE & VETH_XDP_GENERIC, 0);
    }

    #[test]
    fn test_mtu_range() {
        assert!(VETH_MIN_MTU < VETH_MAX_MTU);
    }

    #[test]
    fn test_queue_defaults() {
        assert_eq!(VETH_NUM_TX_QUEUES, 1);
        assert_eq!(VETH_NUM_RX_QUEUES, 1);
    }

    #[test]
    fn test_tx_queue_len() {
        assert_eq!(VETH_TX_QUEUE_LEN, 1000);
    }
}
