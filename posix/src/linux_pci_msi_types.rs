//! `<linux/pci_regs.h>` (MSI/MSI-X subset) — PCI MSI interrupt constants.
//!
//! MSI (Message Signaled Interrupts) replaces traditional pin-based
//! interrupts with in-band memory writes. The device writes a specific
//! value to a specific address to trigger an interrupt — no shared
//! interrupt lines, no routing tables, no level/edge confusion. MSI-X
//! extends this with per-vector masking and up to 2048 vectors,
//! enabling efficient multi-queue device drivers.

// ---------------------------------------------------------------------------
// MSI capability control bits
// ---------------------------------------------------------------------------

/// MSI enable (bit 0 of Message Control).
pub const PCI_MSI_FLAGS_ENABLE: u32 = 0x0001;
/// Multiple Message Capable mask (bits 1-3).
pub const PCI_MSI_FLAGS_QMASK: u32 = 0x000E;
/// Multiple Message Enable mask (bits 4-6).
pub const PCI_MSI_FLAGS_QSIZE: u32 = 0x0070;
/// 64-bit address capable.
pub const PCI_MSI_FLAGS_64BIT: u32 = 0x0080;
/// Per-vector masking capable.
pub const PCI_MSI_FLAGS_MASKBIT: u32 = 0x0100;

// ---------------------------------------------------------------------------
// MSI-X capability control bits
// ---------------------------------------------------------------------------

/// MSI-X enable.
pub const PCI_MSIX_FLAGS_ENABLE: u32 = 0x8000;
/// Function mask (mask all vectors).
pub const PCI_MSIX_FLAGS_MASKALL: u32 = 0x4000;
/// Table size mask (bits 0-10, actual size = value + 1).
pub const PCI_MSIX_FLAGS_QSIZE: u32 = 0x07FF;

// ---------------------------------------------------------------------------
// MSI-X table entry fields
// ---------------------------------------------------------------------------

/// Offset of message address (lower 32 bits) in table entry.
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
// MSI-X vector control bits
// ---------------------------------------------------------------------------

/// Vector is masked (interrupt suppressed).
pub const PCI_MSIX_ENTRY_CTRL_MASKBIT: u32 = 0x0001;

// ---------------------------------------------------------------------------
// MSI limits
// ---------------------------------------------------------------------------

/// Maximum MSI vectors (2^5 = 32).
pub const PCI_MSI_MAX_VECTORS: u32 = 32;
/// Maximum MSI-X vectors.
pub const PCI_MSIX_MAX_VECTORS: u32 = 2048;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msi_flags_bits() {
        assert!(PCI_MSI_FLAGS_ENABLE.is_power_of_two());
        assert!(PCI_MSI_FLAGS_64BIT.is_power_of_two());
        assert!(PCI_MSI_FLAGS_MASKBIT.is_power_of_two());
    }

    #[test]
    fn test_msix_entry_offsets() {
        assert!(PCI_MSIX_ENTRY_LOWER_ADDR < PCI_MSIX_ENTRY_UPPER_ADDR);
        assert!(PCI_MSIX_ENTRY_UPPER_ADDR < PCI_MSIX_ENTRY_DATA);
        assert!(PCI_MSIX_ENTRY_DATA < PCI_MSIX_ENTRY_VECTOR_CTRL);
        assert_eq!(PCI_MSIX_ENTRY_SIZE, PCI_MSIX_ENTRY_VECTOR_CTRL + 4);
    }

    #[test]
    fn test_vector_limits() {
        assert!(PCI_MSI_MAX_VECTORS > 0);
        assert!(PCI_MSIX_MAX_VECTORS > PCI_MSI_MAX_VECTORS);
    }
}
