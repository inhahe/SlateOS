//! `<linux/pci_regs.h>` — PCIe (PCI Express) specific constants.
//!
//! PCIe extends PCI with serial links, extended configuration space
//! (4096 bytes), and advanced features like MSI-X, AER, and ACS.
//! These constants cover PCIe capability structure offsets, link
//! speeds, and width encoding.

// ---------------------------------------------------------------------------
// PCIe link speeds (encoding in Link Status/Control registers)
// ---------------------------------------------------------------------------

/// PCIe Gen 1 (2.5 GT/s).
pub const PCIE_SPEED_2_5GT: u8 = 1;
/// PCIe Gen 2 (5.0 GT/s).
pub const PCIE_SPEED_5_0GT: u8 = 2;
/// PCIe Gen 3 (8.0 GT/s).
pub const PCIE_SPEED_8_0GT: u8 = 3;
/// PCIe Gen 4 (16.0 GT/s).
pub const PCIE_SPEED_16_0GT: u8 = 4;
/// PCIe Gen 5 (32.0 GT/s).
pub const PCIE_SPEED_32_0GT: u8 = 5;
/// PCIe Gen 6 (64.0 GT/s).
pub const PCIE_SPEED_64_0GT: u8 = 6;

// ---------------------------------------------------------------------------
// PCIe link widths
// ---------------------------------------------------------------------------

/// x1 link width.
pub const PCIE_LNK_WIDTH_X1: u8 = 0x01;
/// x2 link width.
pub const PCIE_LNK_WIDTH_X2: u8 = 0x02;
/// x4 link width.
pub const PCIE_LNK_WIDTH_X4: u8 = 0x04;
/// x8 link width.
pub const PCIE_LNK_WIDTH_X8: u8 = 0x08;
/// x12 link width.
pub const PCIE_LNK_WIDTH_X12: u8 = 0x0C;
/// x16 link width.
pub const PCIE_LNK_WIDTH_X16: u8 = 0x10;
/// x32 link width.
pub const PCIE_LNK_WIDTH_X32: u8 = 0x20;

// ---------------------------------------------------------------------------
// PCIe device/port types (in PCI Express Capabilities Register)
// ---------------------------------------------------------------------------

/// PCI Express Endpoint.
pub const PCI_EXP_TYPE_ENDPOINT: u8 = 0x0;
/// Legacy PCI Express Endpoint.
pub const PCI_EXP_TYPE_LEG_END: u8 = 0x1;
/// Root Complex Root Port.
pub const PCI_EXP_TYPE_ROOT_PORT: u8 = 0x4;
/// Upstream Port of PCI Express Switch.
pub const PCI_EXP_TYPE_UPSTREAM: u8 = 0x5;
/// Downstream Port of PCI Express Switch.
pub const PCI_EXP_TYPE_DOWNSTREAM: u8 = 0x6;
/// PCI Express to PCI/PCI-X Bridge.
pub const PCI_EXP_TYPE_PCI_BRIDGE: u8 = 0x7;
/// PCI/PCI-X to PCI Express Bridge.
pub const PCI_EXP_TYPE_PCIE_BRIDGE: u8 = 0x8;
/// Root Complex Integrated Endpoint.
pub const PCI_EXP_TYPE_RC_END: u8 = 0x9;
/// Root Complex Event Collector.
pub const PCI_EXP_TYPE_RC_EC: u8 = 0xA;

// ---------------------------------------------------------------------------
// PCIe extended capability IDs (in extended config space, offset 0x100+)
// ---------------------------------------------------------------------------

/// Advanced Error Reporting.
pub const PCI_EXT_CAP_ID_AER: u16 = 0x0001;
/// Virtual Channel.
pub const PCI_EXT_CAP_ID_VC: u16 = 0x0002;
/// Device Serial Number.
pub const PCI_EXT_CAP_ID_DSN: u16 = 0x0003;
/// Power Budgeting.
pub const PCI_EXT_CAP_ID_PWR: u16 = 0x0004;
/// Access Control Services.
pub const PCI_EXT_CAP_ID_ACS: u16 = 0x000D;
/// Address Translation Services.
pub const PCI_EXT_CAP_ID_ATS: u16 = 0x000F;
/// Single Root I/O Virtualization.
pub const PCI_EXT_CAP_ID_SRIOV: u16 = 0x0010;
/// Latency Tolerance Reporting.
pub const PCI_EXT_CAP_ID_LTR: u16 = 0x0018;
/// L1 PM Substates.
pub const PCI_EXT_CAP_ID_L1SS: u16 = 0x001E;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speeds_sequential() {
        assert_eq!(PCIE_SPEED_2_5GT, 1);
        assert_eq!(PCIE_SPEED_64_0GT, 6);
    }

    #[test]
    fn test_widths_distinct() {
        let widths = [
            PCIE_LNK_WIDTH_X1, PCIE_LNK_WIDTH_X2, PCIE_LNK_WIDTH_X4,
            PCIE_LNK_WIDTH_X8, PCIE_LNK_WIDTH_X12, PCIE_LNK_WIDTH_X16,
            PCIE_LNK_WIDTH_X32,
        ];
        for i in 0..widths.len() {
            for j in (i + 1)..widths.len() {
                assert_ne!(widths[i], widths[j]);
            }
        }
    }

    #[test]
    fn test_device_types_distinct() {
        let types = [
            PCI_EXP_TYPE_ENDPOINT, PCI_EXP_TYPE_LEG_END,
            PCI_EXP_TYPE_ROOT_PORT, PCI_EXP_TYPE_UPSTREAM,
            PCI_EXP_TYPE_DOWNSTREAM, PCI_EXP_TYPE_PCI_BRIDGE,
            PCI_EXP_TYPE_PCIE_BRIDGE, PCI_EXP_TYPE_RC_END,
            PCI_EXP_TYPE_RC_EC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ext_cap_ids_distinct() {
        let caps = [
            PCI_EXT_CAP_ID_AER, PCI_EXT_CAP_ID_VC, PCI_EXT_CAP_ID_DSN,
            PCI_EXT_CAP_ID_PWR, PCI_EXT_CAP_ID_ACS, PCI_EXT_CAP_ID_ATS,
            PCI_EXT_CAP_ID_SRIOV, PCI_EXT_CAP_ID_LTR, PCI_EXT_CAP_ID_L1SS,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }
}
