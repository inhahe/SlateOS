//! `<linux/pci_regs.h>` (PCIe subset) — PCI Express link and device constants.
//!
//! PCIe replaces the parallel PCI bus with serial point-to-point links.
//! Each link has one or more lanes (x1, x2, x4, x8, x16) running at
//! different speeds (Gen1=2.5GT/s through Gen6=64GT/s). The PCIe
//! capability structure in config space controls link speed negotiation,
//! power management (ASPM), error reporting, and device capabilities.

// ---------------------------------------------------------------------------
// PCIe device/port types
// ---------------------------------------------------------------------------

/// PCIe Endpoint (regular device).
pub const PCI_EXP_TYPE_ENDPOINT: u32 = 0x0;
/// Legacy PCI Express Endpoint.
pub const PCI_EXP_TYPE_LEG_END: u32 = 0x1;
/// Root Complex Root Port.
pub const PCI_EXP_TYPE_ROOT_PORT: u32 = 0x4;
/// Upstream port of a PCIe Switch.
pub const PCI_EXP_TYPE_UPSTREAM: u32 = 0x5;
/// Downstream port of a PCIe Switch.
pub const PCI_EXP_TYPE_DOWNSTREAM: u32 = 0x6;
/// PCI Express to PCI/PCI-X Bridge.
pub const PCI_EXP_TYPE_PCI_BRIDGE: u32 = 0x7;
/// PCI/PCI-X to PCI Express Bridge.
pub const PCI_EXP_TYPE_PCIE_BRIDGE: u32 = 0x8;
/// Root Complex Integrated Endpoint.
pub const PCI_EXP_TYPE_RC_END: u32 = 0x9;
/// Root Complex Event Collector.
pub const PCI_EXP_TYPE_RC_EC: u32 = 0xA;

// ---------------------------------------------------------------------------
// PCIe link speeds
// ---------------------------------------------------------------------------

/// Gen1: 2.5 GT/s.
pub const PCI_EXP_LNKSTA_SPEED_2_5GT: u32 = 0x1;
/// Gen2: 5.0 GT/s.
pub const PCI_EXP_LNKSTA_SPEED_5GT: u32 = 0x2;
/// Gen3: 8.0 GT/s.
pub const PCI_EXP_LNKSTA_SPEED_8GT: u32 = 0x3;
/// Gen4: 16.0 GT/s.
pub const PCI_EXP_LNKSTA_SPEED_16GT: u32 = 0x4;
/// Gen5: 32.0 GT/s.
pub const PCI_EXP_LNKSTA_SPEED_32GT: u32 = 0x5;
/// Gen6: 64.0 GT/s.
pub const PCI_EXP_LNKSTA_SPEED_64GT: u32 = 0x6;

// ---------------------------------------------------------------------------
// PCIe link widths
// ---------------------------------------------------------------------------

/// x1 link width.
pub const PCI_EXP_LNKSTA_WIDTH_X1: u32 = 0x01;
/// x2 link width.
pub const PCI_EXP_LNKSTA_WIDTH_X2: u32 = 0x02;
/// x4 link width.
pub const PCI_EXP_LNKSTA_WIDTH_X4: u32 = 0x04;
/// x8 link width.
pub const PCI_EXP_LNKSTA_WIDTH_X8: u32 = 0x08;
/// x16 link width.
pub const PCI_EXP_LNKSTA_WIDTH_X16: u32 = 0x10;
/// x32 link width.
pub const PCI_EXP_LNKSTA_WIDTH_X32: u32 = 0x20;

// ---------------------------------------------------------------------------
// ASPM (Active State Power Management)
// ---------------------------------------------------------------------------

/// ASPM disabled.
pub const PCI_EXP_LNKCTL_ASPM_DISABLED: u32 = 0x0;
/// ASPM L0s (fast entry/exit, small power saving).
pub const PCI_EXP_LNKCTL_ASPM_L0S: u32 = 0x1;
/// ASPM L1 (deeper idle, more power saving).
pub const PCI_EXP_LNKCTL_ASPM_L1: u32 = 0x2;
/// ASPM L0s + L1 (both enabled).
pub const PCI_EXP_LNKCTL_ASPM_L0S_L1: u32 = 0x3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types = [
            PCI_EXP_TYPE_ENDPOINT,
            PCI_EXP_TYPE_LEG_END,
            PCI_EXP_TYPE_ROOT_PORT,
            PCI_EXP_TYPE_UPSTREAM,
            PCI_EXP_TYPE_DOWNSTREAM,
            PCI_EXP_TYPE_PCI_BRIDGE,
            PCI_EXP_TYPE_PCIE_BRIDGE,
            PCI_EXP_TYPE_RC_END,
            PCI_EXP_TYPE_RC_EC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_speeds_ordered() {
        assert!(PCI_EXP_LNKSTA_SPEED_2_5GT < PCI_EXP_LNKSTA_SPEED_5GT);
        assert!(PCI_EXP_LNKSTA_SPEED_5GT < PCI_EXP_LNKSTA_SPEED_8GT);
        assert!(PCI_EXP_LNKSTA_SPEED_8GT < PCI_EXP_LNKSTA_SPEED_16GT);
        assert!(PCI_EXP_LNKSTA_SPEED_16GT < PCI_EXP_LNKSTA_SPEED_32GT);
        assert!(PCI_EXP_LNKSTA_SPEED_32GT < PCI_EXP_LNKSTA_SPEED_64GT);
    }

    #[test]
    fn test_aspm_levels() {
        assert_eq!(
            PCI_EXP_LNKCTL_ASPM_L0S_L1,
            PCI_EXP_LNKCTL_ASPM_L0S | PCI_EXP_LNKCTL_ASPM_L1
        );
    }
}
