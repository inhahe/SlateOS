//! `<linux/pci.h>` — PCI power management state constants.
//!
//! PCI power management defines device power states (D0-D3),
//! PME (Power Management Event) wake capabilities, and the ASPM
//! (Active State Power Management) link states that control PCIe
//! link power during idle periods.

// ---------------------------------------------------------------------------
// PCI-PM device power states
// ---------------------------------------------------------------------------

/// D0: Fully operational.
pub const PCI_D0: u32 = 0;
/// D1: Light sleep (optional, device-specific).
pub const PCI_D1: u32 = 1;
/// D2: Deeper sleep (optional, device-specific).
pub const PCI_D2: u32 = 2;
/// D3hot: Off but power applied (can signal PME).
pub const PCI_D3HOT: u32 = 3;
/// D3cold: Off with power removed (Vaux or fully off).
pub const PCI_D3COLD: u32 = 4;
/// Unknown/uninitialized power state.
pub const PCI_UNKNOWN: u32 = 5;
/// Power state error.
pub const PCI_POWER_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// PME (Power Management Event) support flags
// ---------------------------------------------------------------------------

/// Device can generate PME from D0.
pub const PCI_PM_CAP_PME_D0: u32 = 1 << 11;
/// Device can generate PME from D1.
pub const PCI_PM_CAP_PME_D1: u32 = 1 << 12;
/// Device can generate PME from D2.
pub const PCI_PM_CAP_PME_D2: u32 = 1 << 13;
/// Device can generate PME from D3hot.
pub const PCI_PM_CAP_PME_D3HOT: u32 = 1 << 14;
/// Device can generate PME from D3cold.
pub const PCI_PM_CAP_PME_D3COLD: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// ASPM (Active State Power Management) policies
// ---------------------------------------------------------------------------

/// ASPM disabled.
pub const PCIE_ASPM_DISABLED: u32 = 0;
/// ASPM L0s only (fast entry/exit, small savings).
pub const PCIE_ASPM_L0S: u32 = 1;
/// ASPM L1 only (deeper savings, slower transition).
pub const PCIE_ASPM_L1: u32 = 2;
/// ASPM L0s + L1 both enabled.
pub const PCIE_ASPM_L0S_L1: u32 = 3;

// ---------------------------------------------------------------------------
// PM control/status register bits
// ---------------------------------------------------------------------------

/// Power state field mask (bits 1:0 of PMCSR).
pub const PCI_PM_CTRL_STATE_MASK: u16 = 0x0003;
/// PME enable (bit 8 of PMCSR).
pub const PCI_PM_CTRL_PME_ENABLE: u16 = 0x0100;
/// PME status (bit 15 of PMCSR, write-1-to-clear).
pub const PCI_PM_CTRL_PME_STATUS: u16 = 0x8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_states_sequential() {
        assert_eq!(PCI_D0, 0);
        assert_eq!(PCI_D1, 1);
        assert_eq!(PCI_D2, 2);
        assert_eq!(PCI_D3HOT, 3);
        assert_eq!(PCI_D3COLD, 4);
    }

    #[test]
    fn test_pme_caps_no_overlap() {
        let caps = [
            PCI_PM_CAP_PME_D0,
            PCI_PM_CAP_PME_D1,
            PCI_PM_CAP_PME_D2,
            PCI_PM_CAP_PME_D3HOT,
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
    fn test_aspm_distinct() {
        let modes = [
            PCIE_ASPM_DISABLED,
            PCIE_ASPM_L0S,
            PCIE_ASPM_L1,
            PCIE_ASPM_L0S_L1,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_aspm_l0s_l1_combines() {
        assert_eq!(PCIE_ASPM_L0S_L1, PCIE_ASPM_L0S | PCIE_ASPM_L1);
    }

    #[test]
    fn test_pmcsr_bits() {
        assert_eq!(PCI_PM_CTRL_PME_ENABLE & PCI_PM_CTRL_PME_STATUS, 0);
    }
}
