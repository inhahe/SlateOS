//! `<linux/thunderbolt.h>` — Thunderbolt/USB4 constants.
//!
//! Thunderbolt is a high-speed I/O interconnect combining PCIe and
//! DisplayPort. USB4 is the standardized evolution. The kernel
//! manages security levels, device authorization, and tunneling.

// ---------------------------------------------------------------------------
// Security levels
// ---------------------------------------------------------------------------

/// No security (devices connect freely).
pub const TB_SECURITY_NONE: u32 = 0;
/// User authorization required.
pub const TB_SECURITY_USER: u32 = 1;
/// Secure connect (challenge-response).
pub const TB_SECURITY_SECURE: u32 = 2;
/// DMA protection by IOMMU (no user interaction).
pub const TB_SECURITY_DPONLY: u32 = 3;
/// USB4 security.
pub const TB_SECURITY_USB4: u32 = 4;
/// BIOS-managed security.
pub const TB_SECURITY_NOPCIE: u32 = 5;

// ---------------------------------------------------------------------------
// Device authorization states
// ---------------------------------------------------------------------------

/// Not authorized.
pub const TB_AUTH_NONE: u32 = 0;
/// First key (approved for this boot).
pub const TB_AUTH_FIRST_KEY: u32 = 1;
/// Secure key (approved permanently).
pub const TB_AUTH_SECURE_KEY: u32 = 2;

// ---------------------------------------------------------------------------
// Tunnel types
// ---------------------------------------------------------------------------

/// PCIe tunnel.
pub const TB_TUNNEL_PCI: u32 = 1 << 0;
/// DisplayPort tunnel.
pub const TB_TUNNEL_DP: u32 = 1 << 1;
/// DMA tunnel.
pub const TB_TUNNEL_DMA: u32 = 1 << 2;
/// USB3 tunnel.
pub const TB_TUNNEL_USB3: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Link speeds (Gbps)
// ---------------------------------------------------------------------------

/// Thunderbolt 1 speed (10 Gbps per lane).
pub const TB_LINK_SPEED_10: u32 = 10;
/// Thunderbolt 2 speed (20 Gbps per lane).
pub const TB_LINK_SPEED_20: u32 = 20;
/// Thunderbolt 3/4 speed (40 Gbps per lane, bidirectional).
pub const TB_LINK_SPEED_40: u32 = 40;

// ---------------------------------------------------------------------------
// Link widths
// ---------------------------------------------------------------------------

/// Single lane.
pub const TB_LINK_WIDTH_SINGLE: u32 = 1;
/// Dual lane (two lanes bonded).
pub const TB_LINK_WIDTH_DUAL: u32 = 2;

// ---------------------------------------------------------------------------
// NHI (Native Host Interface) ring types
// ---------------------------------------------------------------------------

/// TX ring.
pub const TB_RING_TYPE_TX: u32 = 0;
/// RX ring.
pub const TB_RING_TYPE_RX: u32 = 1;

// ---------------------------------------------------------------------------
// Connection manager types
// ---------------------------------------------------------------------------

/// Firmware connection manager.
pub const TB_CM_FW: u32 = 0;
/// Software connection manager.
pub const TB_CM_SW: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_levels_distinct() {
        let levels = [
            TB_SECURITY_NONE, TB_SECURITY_USER, TB_SECURITY_SECURE,
            TB_SECURITY_DPONLY, TB_SECURITY_USB4, TB_SECURITY_NOPCIE,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_auth_states_distinct() {
        let states = [TB_AUTH_NONE, TB_AUTH_FIRST_KEY, TB_AUTH_SECURE_KEY];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_tunnel_types_are_powers_of_two() {
        let tunnels = [
            TB_TUNNEL_PCI, TB_TUNNEL_DP, TB_TUNNEL_DMA, TB_TUNNEL_USB3,
        ];
        for tunnel in &tunnels {
            assert!(tunnel.is_power_of_two());
        }
    }

    #[test]
    fn test_tunnel_types_no_overlap() {
        let tunnels = [
            TB_TUNNEL_PCI, TB_TUNNEL_DP, TB_TUNNEL_DMA, TB_TUNNEL_USB3,
        ];
        for i in 0..tunnels.len() {
            for j in (i + 1)..tunnels.len() {
                assert_eq!(tunnels[i] & tunnels[j], 0);
            }
        }
    }

    #[test]
    fn test_link_speeds() {
        assert!(TB_LINK_SPEED_10 < TB_LINK_SPEED_20);
        assert!(TB_LINK_SPEED_20 < TB_LINK_SPEED_40);
    }

    #[test]
    fn test_link_widths() {
        assert_eq!(TB_LINK_WIDTH_SINGLE, 1);
        assert_eq!(TB_LINK_WIDTH_DUAL, 2);
    }

    #[test]
    fn test_ring_types() {
        assert_ne!(TB_RING_TYPE_TX, TB_RING_TYPE_RX);
    }

    #[test]
    fn test_cm_types() {
        assert_ne!(TB_CM_FW, TB_CM_SW);
    }
}
