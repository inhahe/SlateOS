//! `<linux/pci_regs.h>` (capability subset) — PCI capability IDs.
//!
//! PCI capabilities are optional feature blocks linked as a list in
//! configuration space. Each capability has an 8-bit ID and a pointer
//! to the next capability. Capabilities extend the basic PCI spec
//! with features like MSI interrupts, power management, PCIe link
//! control, and SR-IOV virtualization.

// ---------------------------------------------------------------------------
// PCI capability IDs
// ---------------------------------------------------------------------------

/// Power Management.
pub const PCI_CAP_ID_PM: u32 = 0x01;
/// AGP (Accelerated Graphics Port, legacy).
pub const PCI_CAP_ID_AGP: u32 = 0x02;
/// VPD (Vital Product Data).
pub const PCI_CAP_ID_VPD: u32 = 0x03;
/// Slot Identification.
pub const PCI_CAP_ID_SLOTID: u32 = 0x04;
/// MSI (Message Signaled Interrupts).
pub const PCI_CAP_ID_MSI: u32 = 0x05;
/// CompactPCI Hot Swap.
pub const PCI_CAP_ID_CHSWP: u32 = 0x06;
/// PCI-X.
pub const PCI_CAP_ID_PCIX: u32 = 0x07;
/// HyperTransport.
pub const PCI_CAP_ID_HT: u32 = 0x08;
/// Vendor-specific.
pub const PCI_CAP_ID_VNDR: u32 = 0x09;
/// Debug port.
pub const PCI_CAP_ID_DBG: u32 = 0x0A;
/// CompactPCI Central Resource Control.
pub const PCI_CAP_ID_CCRC: u32 = 0x0B;
/// PCI Hot-Plug.
pub const PCI_CAP_ID_SHPC: u32 = 0x0C;
/// PCIe bridge Subsystem Vendor ID.
pub const PCI_CAP_ID_SSVID: u32 = 0x0D;
/// AGP 8x.
pub const PCI_CAP_ID_AGP3: u32 = 0x0E;
/// Secure Device.
pub const PCI_CAP_ID_SECURE: u32 = 0x0F;
/// PCI Express.
pub const PCI_CAP_ID_EXP: u32 = 0x10;
/// MSI-X (extended MSI).
pub const PCI_CAP_ID_MSIX: u32 = 0x11;
/// SATA Data/Index Configuration.
pub const PCI_CAP_ID_SATA: u32 = 0x12;
/// Advanced Features (AF).
pub const PCI_CAP_ID_AF: u32 = 0x13;

// ---------------------------------------------------------------------------
// PCIe Extended capability IDs (in extended config space, offset 0x100+)
// ---------------------------------------------------------------------------

/// Advanced Error Reporting (AER).
pub const PCI_EXT_CAP_ID_AER: u32 = 0x01;
/// Virtual Channel (VC).
pub const PCI_EXT_CAP_ID_VC: u32 = 0x02;
/// Device Serial Number.
pub const PCI_EXT_CAP_ID_DSN: u32 = 0x03;
/// Power Budgeting.
pub const PCI_EXT_CAP_ID_PWR: u32 = 0x04;
/// Root Complex Link Declaration.
pub const PCI_EXT_CAP_ID_RCLD: u32 = 0x05;
/// ACS (Access Control Services).
pub const PCI_EXT_CAP_ID_ACS: u32 = 0x0D;
/// ARI (Alternative Routing-ID Interpretation).
pub const PCI_EXT_CAP_ID_ARI: u32 = 0x0E;
/// ATS (Address Translation Services).
pub const PCI_EXT_CAP_ID_ATS: u32 = 0x0F;
/// SR-IOV (Single Root I/O Virtualization).
pub const PCI_EXT_CAP_ID_SRIOV: u32 = 0x10;
/// LTR (Latency Tolerance Reporting).
pub const PCI_EXT_CAP_ID_LTR: u32 = 0x18;
/// DPC (Downstream Port Containment).
pub const PCI_EXT_CAP_ID_DPC: u32 = 0x1D;
/// L1 PM Substates.
pub const PCI_EXT_CAP_ID_L1SS: u32 = 0x1E;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_ids_distinct() {
        let caps = [
            PCI_CAP_ID_PM,
            PCI_CAP_ID_AGP,
            PCI_CAP_ID_VPD,
            PCI_CAP_ID_MSI,
            PCI_CAP_ID_VNDR,
            PCI_CAP_ID_EXP,
            PCI_CAP_ID_MSIX,
            PCI_CAP_ID_SATA,
            PCI_CAP_ID_AF,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_ext_cap_ids_distinct() {
        let ext = [
            PCI_EXT_CAP_ID_AER,
            PCI_EXT_CAP_ID_VC,
            PCI_EXT_CAP_ID_DSN,
            PCI_EXT_CAP_ID_ACS,
            PCI_EXT_CAP_ID_ARI,
            PCI_EXT_CAP_ID_ATS,
            PCI_EXT_CAP_ID_SRIOV,
            PCI_EXT_CAP_ID_LTR,
            PCI_EXT_CAP_ID_DPC,
            PCI_EXT_CAP_ID_L1SS,
        ];
        for i in 0..ext.len() {
            for j in (i + 1)..ext.len() {
                assert_ne!(ext[i], ext[j]);
            }
        }
    }
}
