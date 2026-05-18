//! `<linux/pci_regs.h>` (PM capability) — PCI power management constants.
//!
//! PCI Power Management allows individual devices to enter low-power
//! states (D1, D2, D3hot, D3cold) independent of the rest of the
//! system. The PM capability in config space controls power state
//! transitions and reports device capabilities (which states are
//! supported, PME wake-up support). Runtime PM uses these to power
//! down idle devices automatically.

// ---------------------------------------------------------------------------
// PCI power states
// ---------------------------------------------------------------------------

/// D0: fully operational.
pub const PCI_PM_D0: u32 = 0;
/// D1: light sleep (optional, device-specific).
pub const PCI_PM_D1: u32 = 1;
/// D2: deeper sleep (optional).
pub const PCI_PM_D2: u32 = 2;
/// D3hot: device is off but power is still present.
pub const PCI_PM_D3HOT: u32 = 3;
/// D3cold: power fully removed from device.
pub const PCI_PM_D3COLD: u32 = 4;

// ---------------------------------------------------------------------------
// PM capability register bits
// ---------------------------------------------------------------------------

/// Device supports D1 state.
pub const PCI_PM_CAP_D1: u32 = 0x0200;
/// Device supports D2 state.
pub const PCI_PM_CAP_D2: u32 = 0x0400;
/// Device can generate PME from D0.
pub const PCI_PM_CAP_PME_D0: u32 = 0x0800;
/// Device can generate PME from D1.
pub const PCI_PM_CAP_PME_D1: u32 = 0x1000;
/// Device can generate PME from D2.
pub const PCI_PM_CAP_PME_D2: u32 = 0x2000;
/// Device can generate PME from D3hot.
pub const PCI_PM_CAP_PME_D3HOT: u32 = 0x4000;
/// Device can generate PME from D3cold.
pub const PCI_PM_CAP_PME_D3COLD: u32 = 0x8000;

// ---------------------------------------------------------------------------
// PM control/status register bits
// ---------------------------------------------------------------------------

/// Power state mask (bits 0-1).
pub const PCI_PM_CTRL_STATE_MASK: u32 = 0x0003;
/// PME enable.
pub const PCI_PM_CTRL_PME_ENABLE: u32 = 0x0100;
/// PME status (write-1-to-clear).
pub const PCI_PM_CTRL_PME_STATUS: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_states_ordered() {
        assert!(PCI_PM_D0 < PCI_PM_D1);
        assert!(PCI_PM_D1 < PCI_PM_D2);
        assert!(PCI_PM_D2 < PCI_PM_D3HOT);
        assert!(PCI_PM_D3HOT < PCI_PM_D3COLD);
    }

    #[test]
    fn test_pme_caps_no_overlap() {
        let caps = [
            PCI_PM_CAP_PME_D0, PCI_PM_CAP_PME_D1,
            PCI_PM_CAP_PME_D2, PCI_PM_CAP_PME_D3HOT,
            PCI_PM_CAP_PME_D3COLD,
        ];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_ctrl_bits() {
        assert_eq!(PCI_PM_CTRL_STATE_MASK, 3);
        // PME_ENABLE and PME_STATUS are in different bit positions
        assert_eq!(PCI_PM_CTRL_PME_ENABLE & PCI_PM_CTRL_PME_STATUS, 0);
        assert_eq!(PCI_PM_CTRL_PME_ENABLE & PCI_PM_CTRL_STATE_MASK, 0);
    }
}
