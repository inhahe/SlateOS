//! `<linux/pci_regs.h>` — Additional PCI constants (part 2).
//!
//! Supplementary PCI constants covering capability IDs,
//! command register bits, and status register bits.

// ---------------------------------------------------------------------------
// PCI command register bits
// ---------------------------------------------------------------------------

/// I/O space enable.
pub const PCI_COMMAND_IO: u16 = 0x0001;
/// Memory space enable.
pub const PCI_COMMAND_MEMORY: u16 = 0x0002;
/// Bus master enable.
pub const PCI_COMMAND_MASTER: u16 = 0x0004;
/// Special cycles.
pub const PCI_COMMAND_SPECIAL: u16 = 0x0008;
/// Memory write/invalidate.
pub const PCI_COMMAND_INVALIDATE: u16 = 0x0010;
/// VGA palette snoop.
pub const PCI_COMMAND_VGA_PALETTE: u16 = 0x0020;
/// Parity error response.
pub const PCI_COMMAND_PARITY: u16 = 0x0040;
/// Stepping control.
pub const PCI_COMMAND_WAIT: u16 = 0x0080;
/// SERR# enable.
pub const PCI_COMMAND_SERR: u16 = 0x0100;
/// Fast back-to-back.
pub const PCI_COMMAND_FAST_BACK: u16 = 0x0200;
/// INTx disable.
pub const PCI_COMMAND_INTX_DISABLE: u16 = 0x0400;

// ---------------------------------------------------------------------------
// PCI capability IDs
// ---------------------------------------------------------------------------

/// Power Management.
pub const PCI_CAP_ID_PM: u8 = 0x01;
/// AGP.
pub const PCI_CAP_ID_AGP: u8 = 0x02;
/// Vital Product Data.
pub const PCI_CAP_ID_VPD: u8 = 0x03;
/// Slot Identification.
pub const PCI_CAP_ID_SLOTID: u8 = 0x04;
/// MSI.
pub const PCI_CAP_ID_MSI: u8 = 0x05;
/// CompactPCI HotSwap.
pub const PCI_CAP_ID_CHSWP: u8 = 0x06;
/// PCI-X.
pub const PCI_CAP_ID_PCIX: u8 = 0x07;
/// Vendor-specific.
pub const PCI_CAP_ID_VNDR: u8 = 0x09;
/// PCI Express.
pub const PCI_CAP_ID_EXP: u8 = 0x10;
/// MSI-X.
pub const PCI_CAP_ID_MSIX: u8 = 0x11;
/// SATA Data/Index.
pub const PCI_CAP_ID_SATA: u8 = 0x12;
/// Advanced Features.
pub const PCI_CAP_ID_AF: u8 = 0x13;

// ---------------------------------------------------------------------------
// PCI Express extended capability IDs
// ---------------------------------------------------------------------------

/// Advanced Error Reporting.
pub const PCI_EXT_CAP_ID_ERR: u16 = 0x0001;
/// Virtual Channel.
pub const PCI_EXT_CAP_ID_VC: u16 = 0x0002;
/// Device Serial Number.
pub const PCI_EXT_CAP_ID_DSN: u16 = 0x0003;
/// Power Budgeting.
pub const PCI_EXT_CAP_ID_PWR: u16 = 0x0004;
/// ACS.
pub const PCI_EXT_CAP_ID_ACS: u16 = 0x000D;
/// ARI.
pub const PCI_EXT_CAP_ID_ARI: u16 = 0x000E;
/// SR-IOV.
pub const PCI_EXT_CAP_ID_SRIOV: u16 = 0x0010;
/// LTR.
pub const PCI_EXT_CAP_ID_LTR: u16 = 0x0018;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_bits_no_overlap() {
        let bits = [
            PCI_COMMAND_IO,
            PCI_COMMAND_MEMORY,
            PCI_COMMAND_MASTER,
            PCI_COMMAND_SPECIAL,
            PCI_COMMAND_INVALIDATE,
            PCI_COMMAND_VGA_PALETTE,
            PCI_COMMAND_PARITY,
            PCI_COMMAND_WAIT,
            PCI_COMMAND_SERR,
            PCI_COMMAND_FAST_BACK,
            PCI_COMMAND_INTX_DISABLE,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_cap_ids_distinct() {
        let caps = [
            PCI_CAP_ID_PM,
            PCI_CAP_ID_AGP,
            PCI_CAP_ID_VPD,
            PCI_CAP_ID_SLOTID,
            PCI_CAP_ID_MSI,
            PCI_CAP_ID_CHSWP,
            PCI_CAP_ID_PCIX,
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
        let caps = [
            PCI_EXT_CAP_ID_ERR,
            PCI_EXT_CAP_ID_VC,
            PCI_EXT_CAP_ID_DSN,
            PCI_EXT_CAP_ID_PWR,
            PCI_EXT_CAP_ID_ACS,
            PCI_EXT_CAP_ID_ARI,
            PCI_EXT_CAP_ID_SRIOV,
            PCI_EXT_CAP_ID_LTR,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }
}
