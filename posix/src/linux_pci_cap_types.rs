//! `<linux/pci_regs.h>` — PCI capability IDs and MSI/MSI-X constants.
//!
//! PCI capabilities are optional feature sets advertised via a linked
//! list in configuration space. Each capability has an ID and a
//! pointer to the next. MSI and MSI-X are the primary interrupt
//! mechanisms for modern PCI/PCIe devices.

// ---------------------------------------------------------------------------
// PCI capability IDs (standard config space, offset < 0x100)
// ---------------------------------------------------------------------------

/// Power Management capability.
pub const PCI_CAP_ID_PM: u8 = 0x01;
/// AGP (Accelerated Graphics Port).
pub const PCI_CAP_ID_AGP: u8 = 0x02;
/// Vital Product Data.
pub const PCI_CAP_ID_VPD: u8 = 0x03;
/// Slot Identification.
pub const PCI_CAP_ID_SLOTID: u8 = 0x04;
/// Message Signaled Interrupts (MSI).
pub const PCI_CAP_ID_MSI: u8 = 0x05;
/// CompactPCI HotSwap.
pub const PCI_CAP_ID_CHSWP: u8 = 0x06;
/// PCI-X.
pub const PCI_CAP_ID_PCIX: u8 = 0x07;
/// HyperTransport.
pub const PCI_CAP_ID_HT: u8 = 0x08;
/// Vendor Specific.
pub const PCI_CAP_ID_VNDR: u8 = 0x09;
/// PCI Express.
pub const PCI_CAP_ID_EXP: u8 = 0x10;
/// MSI-X (extended MSI).
pub const PCI_CAP_ID_MSIX: u8 = 0x11;
/// SATA Data/Index Configuration.
pub const PCI_CAP_ID_SATA: u8 = 0x12;
/// Advanced Features (AF).
pub const PCI_CAP_ID_AF: u8 = 0x13;

// ---------------------------------------------------------------------------
// MSI control register bits
// ---------------------------------------------------------------------------

/// MSI Enable.
pub const PCI_MSI_FLAGS_ENABLE: u16 = 0x0001;
/// Multiple Message Capable (bits 3:1, encoded as power of 2).
pub const PCI_MSI_FLAGS_QMASK: u16 = 0x000E;
/// Multiple Message Enable (bits 6:4).
pub const PCI_MSI_FLAGS_QSIZE: u16 = 0x0070;
/// 64-bit address capable.
pub const PCI_MSI_FLAGS_64BIT: u16 = 0x0080;
/// Per-vector masking capable.
pub const PCI_MSI_FLAGS_MASKBIT: u16 = 0x0100;

// ---------------------------------------------------------------------------
// MSI-X control register bits
// ---------------------------------------------------------------------------

/// MSI-X Enable.
pub const PCI_MSIX_FLAGS_ENABLE: u16 = 0x8000;
/// Function Mask (mask all vectors).
pub const PCI_MSIX_FLAGS_MASKALL: u16 = 0x4000;
/// Table size mask (bits 10:0, actual size = value + 1).
pub const PCI_MSIX_FLAGS_QSIZE: u16 = 0x07FF;

// ---------------------------------------------------------------------------
// MSI-X table entry fields
// ---------------------------------------------------------------------------

/// Offset of message address (low 32 bits) in table entry.
pub const PCI_MSIX_ENTRY_LOWER_ADDR: u32 = 0x00;
/// Offset of message address (upper 32 bits) in table entry.
pub const PCI_MSIX_ENTRY_UPPER_ADDR: u32 = 0x04;
/// Offset of message data in table entry.
pub const PCI_MSIX_ENTRY_DATA: u32 = 0x08;
/// Offset of vector control in table entry.
pub const PCI_MSIX_ENTRY_VECTOR_CTRL: u32 = 0x0C;
/// Size of one MSI-X table entry.
pub const PCI_MSIX_ENTRY_SIZE: u32 = 16;

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
            PCI_CAP_ID_SLOTID,
            PCI_CAP_ID_MSI,
            PCI_CAP_ID_CHSWP,
            PCI_CAP_ID_PCIX,
            PCI_CAP_ID_HT,
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
    fn test_msi_enable_bit() {
        assert_eq!(PCI_MSI_FLAGS_ENABLE, 1);
    }

    #[test]
    fn test_msix_enable_is_high_bit() {
        assert_eq!(PCI_MSIX_FLAGS_ENABLE, 0x8000);
    }

    #[test]
    fn test_msix_entry_layout() {
        assert_eq!(PCI_MSIX_ENTRY_LOWER_ADDR, 0);
        assert_eq!(PCI_MSIX_ENTRY_SIZE, 16);
        assert!(PCI_MSIX_ENTRY_VECTOR_CTRL < PCI_MSIX_ENTRY_SIZE);
    }
}
