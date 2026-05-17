//! `<linux/msi.h>` — Message Signaled Interrupts constants.
//!
//! MSI and MSI-X replace traditional level/edge-triggered PCI
//! interrupts with in-band messages written to a special address.
//! This eliminates sharing, reduces latency, and enables per-queue
//! interrupts for modern devices (NVMe, network adapters, etc.).

// ---------------------------------------------------------------------------
// MSI capability register offsets (relative to cap start)
// ---------------------------------------------------------------------------

/// MSI message control register (16-bit).
pub const PCI_MSI_FLAGS: u8 = 0x02;
/// MSI message address (low 32 bits).
pub const PCI_MSI_ADDRESS_LO: u8 = 0x04;
/// MSI message address (high 32 bits, if 64-bit capable).
pub const PCI_MSI_ADDRESS_HI: u8 = 0x08;
/// MSI message data (16-bit, 32-bit mode).
pub const PCI_MSI_DATA_32: u8 = 0x08;
/// MSI message data (16-bit, 64-bit mode).
pub const PCI_MSI_DATA_64: u8 = 0x0C;

// ---------------------------------------------------------------------------
// MSI message control flags
// ---------------------------------------------------------------------------

/// MSI enable.
pub const PCI_MSI_FLAGS_ENABLE: u16 = 1 << 0;
/// Multiple Message Capable (bits 3:1).
pub const PCI_MSI_FLAGS_QMASK: u16 = 0x000E;
/// Multiple Message Enable (bits 6:4).
pub const PCI_MSI_FLAGS_QSIZE: u16 = 0x0070;
/// 64-bit address capable.
pub const PCI_MSI_FLAGS_64BIT: u16 = 1 << 7;
/// Per-vector masking capable.
pub const PCI_MSI_FLAGS_MASKBIT: u16 = 1 << 8;

// ---------------------------------------------------------------------------
// MSI-X capability register offsets
// ---------------------------------------------------------------------------

/// MSI-X message control (16-bit).
pub const PCI_MSIX_FLAGS: u8 = 0x02;
/// MSI-X table offset + BIR.
pub const PCI_MSIX_TABLE: u8 = 0x04;
/// MSI-X PBA (Pending Bit Array) offset + BIR.
pub const PCI_MSIX_PBA: u8 = 0x08;

// ---------------------------------------------------------------------------
// MSI-X message control flags
// ---------------------------------------------------------------------------

/// MSI-X enable.
pub const PCI_MSIX_FLAGS_ENABLE: u16 = 1 << 15;
/// Function mask (mask all vectors).
pub const PCI_MSIX_FLAGS_MASKALL: u16 = 1 << 14;
/// Table size mask (bits 10:0, actual size = value + 1).
pub const PCI_MSIX_FLAGS_QSIZE: u16 = 0x07FF;

// ---------------------------------------------------------------------------
// MSI-X table entry layout (each entry = 16 bytes)
// ---------------------------------------------------------------------------

/// Entry size in bytes.
pub const PCI_MSIX_ENTRY_SIZE: u8 = 16;
/// Lower message address offset within entry.
pub const PCI_MSIX_ENTRY_ADDR_LO: u8 = 0;
/// Upper message address offset within entry.
pub const PCI_MSIX_ENTRY_ADDR_HI: u8 = 4;
/// Message data offset within entry.
pub const PCI_MSIX_ENTRY_DATA: u8 = 8;
/// Vector control offset within entry.
pub const PCI_MSIX_ENTRY_VECTOR_CTRL: u8 = 12;
/// Mask bit in vector control.
pub const PCI_MSIX_ENTRY_CTRL_MASKBIT: u32 = 1;

// ---------------------------------------------------------------------------
// MSI domain flags
// ---------------------------------------------------------------------------

/// Allocate MSI vectors.
pub const MSI_FLAG_USE_DEF_DOMAIN_OPS: u32 = 1 << 0;
/// Use device-specific chip ops.
pub const MSI_FLAG_USE_DEF_CHIP_OPS: u32 = 1 << 1;
/// Multi-MSI supported.
pub const MSI_FLAG_MULTI_PCI_MSI: u32 = 1 << 2;
/// PCI MSI-X supported.
pub const MSI_FLAG_PCI_MSIX: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msi_offsets_valid() {
        assert_eq!(PCI_MSI_FLAGS, 0x02);
        assert_eq!(PCI_MSI_ADDRESS_LO, 0x04);
    }

    #[test]
    fn test_msi_flags_enable() {
        assert_eq!(PCI_MSI_FLAGS_ENABLE, 1);
    }

    #[test]
    fn test_msix_entry_layout() {
        assert_eq!(PCI_MSIX_ENTRY_SIZE, 16);
        assert!(PCI_MSIX_ENTRY_ADDR_LO < PCI_MSIX_ENTRY_ADDR_HI);
        assert!(PCI_MSIX_ENTRY_ADDR_HI < PCI_MSIX_ENTRY_DATA);
        assert!(PCI_MSIX_ENTRY_DATA < PCI_MSIX_ENTRY_VECTOR_CTRL);
    }

    #[test]
    fn test_domain_flags_no_overlap() {
        let flags = [
            MSI_FLAG_USE_DEF_DOMAIN_OPS, MSI_FLAG_USE_DEF_CHIP_OPS,
            MSI_FLAG_MULTI_PCI_MSI, MSI_FLAG_PCI_MSIX,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
