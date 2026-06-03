//! `<linux/thunderbolt.h>` — Thunderbolt/USB4 constants.
//!
//! Thunderbolt constants covering security levels,
//! device types, tunnel modes, and NHI ring parameters.

// ---------------------------------------------------------------------------
// Security levels
// ---------------------------------------------------------------------------

/// No security (legacy mode).
pub const TB_SECURITY_NONE: u32 = 0;
/// User approval.
pub const TB_SECURITY_USER: u32 = 1;
/// Secure connect (challenge).
pub const TB_SECURITY_SECURE: u32 = 2;
/// DP tunnel only.
pub const TB_SECURITY_DPONLY: u32 = 3;
/// USB only.
pub const TB_SECURITY_USBONLY: u32 = 4;
/// No PCIe tunneling.
pub const TB_SECURITY_NOPCIE: u32 = 5;

// ---------------------------------------------------------------------------
// Device state
// ---------------------------------------------------------------------------

/// Device not connected.
pub const TB_STATE_DISABLED: u32 = 0;
/// Device connected.
pub const TB_STATE_ENABLED: u32 = 1;
/// Device authorized.
pub const TB_STATE_AUTHORIZED: u32 = 2;

// ---------------------------------------------------------------------------
// Tunnel types
// ---------------------------------------------------------------------------

/// PCIe tunnel.
pub const TB_TUNNEL_PCIE: u32 = 1;
/// DisplayPort tunnel.
pub const TB_TUNNEL_DP: u32 = 2;
/// DMA tunnel.
pub const TB_TUNNEL_DMA: u32 = 4;
/// USB3 tunnel.
pub const TB_TUNNEL_USB3: u32 = 8;

// ---------------------------------------------------------------------------
// NHI (Native Host Interface) ring types
// ---------------------------------------------------------------------------

/// TX ring.
pub const TB_RING_TYPE_TX: u32 = 0;
/// RX ring.
pub const TB_RING_TYPE_RX: u32 = 1;
/// Max frame size.
pub const TB_RING_MAX_FRAME_SIZE: u32 = 4096;
/// Default ring size.
pub const TB_RING_DEFAULT_SIZE: u32 = 256;

// ---------------------------------------------------------------------------
// Port types
// ---------------------------------------------------------------------------

/// Inactive port.
pub const TB_PORT_INACTIVE: u32 = 0;
/// Downstream port.
pub const TB_PORT_DOWNSTREAM: u32 = 1;
/// Upstream port.
pub const TB_PORT_UPSTREAM: u32 = 2;
/// NHI port.
pub const TB_PORT_NHI: u32 = 3;

// ---------------------------------------------------------------------------
// Link speed (Gbps)
// ---------------------------------------------------------------------------

/// Thunderbolt 1 (10 Gbps).
pub const TB_LINK_SPEED_GEN1: u32 = 10;
/// Thunderbolt 2 (20 Gbps).
pub const TB_LINK_SPEED_GEN2: u32 = 20;
/// Thunderbolt 3/4 (40 Gbps).
pub const TB_LINK_SPEED_GEN3: u32 = 40;
/// USB4 Gen 4 (80 Gbps).
pub const TB_LINK_SPEED_GEN4: u32 = 80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_levels_sequential() {
        assert_eq!(TB_SECURITY_NONE, 0);
        assert_eq!(TB_SECURITY_USER, 1);
        assert_eq!(TB_SECURITY_NOPCIE, 5);
    }

    #[test]
    fn test_security_levels_distinct() {
        let levels = [
            TB_SECURITY_NONE,
            TB_SECURITY_USER,
            TB_SECURITY_SECURE,
            TB_SECURITY_DPONLY,
            TB_SECURITY_USBONLY,
            TB_SECURITY_NOPCIE,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_tunnel_types_power_of_two() {
        let tunnels = [TB_TUNNEL_PCIE, TB_TUNNEL_DP, TB_TUNNEL_DMA, TB_TUNNEL_USB3];
        for t in &tunnels {
            assert!(t.is_power_of_two(), "{} not power of two", t);
        }
    }

    #[test]
    fn test_port_types_distinct() {
        let ports = [
            TB_PORT_INACTIVE,
            TB_PORT_DOWNSTREAM,
            TB_PORT_UPSTREAM,
            TB_PORT_NHI,
        ];
        for i in 0..ports.len() {
            for j in (i + 1)..ports.len() {
                assert_ne!(ports[i], ports[j]);
            }
        }
    }

    #[test]
    fn test_link_speeds_increasing() {
        assert!(TB_LINK_SPEED_GEN1 < TB_LINK_SPEED_GEN2);
        assert!(TB_LINK_SPEED_GEN2 < TB_LINK_SPEED_GEN3);
        assert!(TB_LINK_SPEED_GEN3 < TB_LINK_SPEED_GEN4);
    }

    #[test]
    fn test_ring_defaults() {
        assert_eq!(TB_RING_MAX_FRAME_SIZE, 4096);
        assert_eq!(TB_RING_DEFAULT_SIZE, 256);
    }
}
