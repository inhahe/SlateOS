//! `<linux/thunderbolt.h>` — Additional Thunderbolt constants.
//!
//! Supplementary Thunderbolt constants covering security levels,
//! device types, and ring configuration flags.

// ---------------------------------------------------------------------------
// Thunderbolt security levels
// ---------------------------------------------------------------------------

/// No security.
pub const TB_SECURITY_NONE: u32 = 0;
/// User approval required.
pub const TB_SECURITY_USER: u32 = 1;
/// Secure connect (key exchange).
pub const TB_SECURITY_SECURE: u32 = 2;
/// DP only (no PCIe tunneling).
pub const TB_SECURITY_DPONLY: u32 = 3;
/// USB-only mode.
pub const TB_SECURITY_USBONLY: u32 = 4;
/// No PCIe tunneling.
pub const TB_SECURITY_NOPCIE: u32 = 5;

// ---------------------------------------------------------------------------
// Thunderbolt device types
// ---------------------------------------------------------------------------

/// Host router.
pub const TB_TYPE_HOST: u32 = 0;
/// Device router.
pub const TB_TYPE_DEVICE: u32 = 1;
/// Non-router (retimer, etc.).
pub const TB_TYPE_NHI: u32 = 2;

// ---------------------------------------------------------------------------
// Thunderbolt tunnel types
// ---------------------------------------------------------------------------

/// PCIe tunnel.
pub const TB_TUNNEL_PCIE: u32 = 1;
/// DisplayPort tunnel.
pub const TB_TUNNEL_DP: u32 = 2;
/// DMA tunnel.
pub const TB_TUNNEL_DMA: u32 = 3;
/// USB3 tunnel.
pub const TB_TUNNEL_USB3: u32 = 4;

// ---------------------------------------------------------------------------
// Thunderbolt link speed values (Gb/s)
// ---------------------------------------------------------------------------

/// 10 Gb/s link.
pub const TB_LINK_SPEED_10: u32 = 10;
/// 20 Gb/s link.
pub const TB_LINK_SPEED_20: u32 = 20;
/// 40 Gb/s link (TBT3).
pub const TB_LINK_SPEED_40: u32 = 40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_device_types_distinct() {
        let types = [TB_TYPE_HOST, TB_TYPE_DEVICE, TB_TYPE_NHI];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_tunnel_types_distinct() {
        let types = [TB_TUNNEL_PCIE, TB_TUNNEL_DP, TB_TUNNEL_DMA, TB_TUNNEL_USB3];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_link_speeds_ordered() {
        assert!(TB_LINK_SPEED_10 < TB_LINK_SPEED_20);
        assert!(TB_LINK_SPEED_20 < TB_LINK_SPEED_40);
    }
}
