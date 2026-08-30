//! `<linux/veth.h>` — virtual Ethernet pair (`veth`).
//!
//! A `veth` pair is two virtual NICs connected back-to-back: packets
//! sent on one come out on the other. Every container runtime
//! (Docker, Podman, containerd) uses veth pairs to splice container
//! network namespaces into a host bridge.

// ---------------------------------------------------------------------------
// `rtnl_link_ops.kind` string — what `ip link add type veth` uses
// ---------------------------------------------------------------------------

pub const VETH_KIND: &str = "veth";

// ---------------------------------------------------------------------------
// `IFLA_VETH_*` netlink attributes (info-specific attributes nested
//  inside `IFLA_INFO_DATA`)
// ---------------------------------------------------------------------------

pub const IFLA_VETH_UNSPEC: u16 = 0;
pub const IFLA_VETH_PEER: u16 = 1;

pub const IFLA_VETH_MAX: u16 = IFLA_VETH_PEER;

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

/// All Ethernet links default to a 1500-byte MTU.
pub const VETH_DEFAULT_MTU: u32 = 1500;

/// veth supports up to the 9000-byte jumbo frame size.
pub const VETH_MAX_MTU: u32 = 65_535;

/// Minimum MTU — the IPv6 MTU floor.
pub const VETH_MIN_MTU: u32 = 1280;

// ---------------------------------------------------------------------------
// veth-specific ethtool stat indices
// ---------------------------------------------------------------------------

pub const VETH_STAT_RX_DROPS: u32 = 0;
pub const VETH_STAT_RX_QUEUE_0_XDP_PACKETS: u32 = 1;
pub const VETH_STAT_RX_QUEUE_0_XDP_BYTES: u32 = 2;

// ---------------------------------------------------------------------------
// veth ethtool driver name
// ---------------------------------------------------------------------------

pub const VETH_DRIVER_NAME: &str = "veth";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_veth_kind_and_driver_match() {
        assert_eq!(VETH_KIND, "veth");
        assert_eq!(VETH_DRIVER_NAME, "veth");
        // The kind and driver name are deliberately the same — that's
        // how userspace finds them.
        assert_eq!(VETH_KIND, VETH_DRIVER_NAME);
    }

    #[test]
    fn test_ifla_attrs_dense() {
        assert_eq!(IFLA_VETH_UNSPEC, 0);
        assert_eq!(IFLA_VETH_PEER, 1);
        // Only one real attribute beyond UNSPEC.
        assert_eq!(IFLA_VETH_MAX, IFLA_VETH_PEER);
    }

    #[test]
    fn test_mtu_window() {
        // veth allows the full Ethernet MTU window: IPv6 floor to
        // the absolute 16-bit upper bound.
        assert_eq!(VETH_DEFAULT_MTU, 1500);
        assert_eq!(VETH_MIN_MTU, 1280);
        assert_eq!(VETH_MAX_MTU, u16::MAX as u32);
        assert!(VETH_MIN_MTU < VETH_DEFAULT_MTU);
        assert!(VETH_DEFAULT_MTU < VETH_MAX_MTU);
    }

    #[test]
    fn test_stat_indices_dense_from_zero() {
        assert_eq!(VETH_STAT_RX_DROPS, 0);
        assert_eq!(VETH_STAT_RX_QUEUE_0_XDP_PACKETS, 1);
        assert_eq!(VETH_STAT_RX_QUEUE_0_XDP_BYTES, 2);
    }
}
