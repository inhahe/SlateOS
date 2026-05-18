//! `<linux/pci_regs.h>` — PCI configuration space register offsets.
//!
//! Every PCI device has a 256-byte configuration space (4096 for
//! PCIe extended). Standard registers at fixed offsets identify the
//! device, control its behavior, and describe its capabilities.
//! These offsets are the same for all PCI devices.

// ---------------------------------------------------------------------------
// PCI Type 0 (endpoint) configuration header offsets
// ---------------------------------------------------------------------------

/// Vendor ID register (16-bit).
pub const PCI_VENDOR_ID: u8 = 0x00;
/// Device ID register (16-bit).
pub const PCI_DEVICE_ID: u8 = 0x02;
/// Command register (16-bit).
pub const PCI_COMMAND: u8 = 0x04;
/// Status register (16-bit).
pub const PCI_STATUS: u8 = 0x06;
/// Revision ID (8-bit).
pub const PCI_REVISION_ID: u8 = 0x08;
/// Programming interface (8-bit, class code byte).
pub const PCI_CLASS_PROG: u8 = 0x09;
/// Subclass code (8-bit).
pub const PCI_CLASS_DEVICE: u8 = 0x0A;
/// Cache line size (8-bit).
pub const PCI_CACHE_LINE_SIZE: u8 = 0x0C;
/// Latency timer (8-bit).
pub const PCI_LATENCY_TIMER: u8 = 0x0D;
/// Header type (8-bit, bit 7 = multi-function).
pub const PCI_HEADER_TYPE: u8 = 0x0E;
/// BIST register (8-bit).
pub const PCI_BIST: u8 = 0x0F;
/// Base Address Register 0.
pub const PCI_BASE_ADDRESS_0: u8 = 0x10;
/// Base Address Register 1.
pub const PCI_BASE_ADDRESS_1: u8 = 0x14;
/// Base Address Register 2.
pub const PCI_BASE_ADDRESS_2: u8 = 0x18;
/// Base Address Register 3.
pub const PCI_BASE_ADDRESS_3: u8 = 0x1C;
/// Base Address Register 4.
pub const PCI_BASE_ADDRESS_4: u8 = 0x20;
/// Base Address Register 5.
pub const PCI_BASE_ADDRESS_5: u8 = 0x24;
/// Subsystem Vendor ID.
pub const PCI_SUBSYSTEM_VENDOR_ID: u8 = 0x2C;
/// Subsystem ID.
pub const PCI_SUBSYSTEM_ID: u8 = 0x2E;
/// Expansion ROM base address.
pub const PCI_ROM_ADDRESS: u8 = 0x30;
/// Capabilities pointer (8-bit, offset to first cap).
pub const PCI_CAPABILITY_LIST: u8 = 0x34;
/// Interrupt line (8-bit, IRQ number).
pub const PCI_INTERRUPT_LINE: u8 = 0x3C;
/// Interrupt pin (8-bit, 1=INTA, 2=INTB, etc.).
pub const PCI_INTERRUPT_PIN: u8 = 0x3D;

// ---------------------------------------------------------------------------
// BAR type bits
// ---------------------------------------------------------------------------

/// BAR is I/O space (bit 0 set).
pub const PCI_BASE_ADDRESS_SPACE_IO: u32 = 0x01;
/// BAR is memory space (bit 0 clear).
pub const PCI_BASE_ADDRESS_SPACE_MEMORY: u32 = 0x00;
/// Memory BAR is 64-bit (bits 2:1 = 10).
pub const PCI_BASE_ADDRESS_MEM_TYPE_64: u32 = 0x04;
/// Memory BAR is prefetchable.
pub const PCI_BASE_ADDRESS_MEM_PREFETCH: u32 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_offsets_ordered() {
        assert!(PCI_VENDOR_ID < PCI_DEVICE_ID);
        assert!(PCI_DEVICE_ID < PCI_COMMAND);
        assert!(PCI_COMMAND < PCI_STATUS);
        assert!(PCI_BASE_ADDRESS_0 < PCI_BASE_ADDRESS_5);
    }

    #[test]
    fn test_offsets_distinct() {
        let offsets = [
            PCI_VENDOR_ID, PCI_DEVICE_ID, PCI_COMMAND, PCI_STATUS,
            PCI_REVISION_ID, PCI_CLASS_PROG, PCI_CLASS_DEVICE,
            PCI_CACHE_LINE_SIZE, PCI_LATENCY_TIMER, PCI_HEADER_TYPE,
            PCI_BIST, PCI_BASE_ADDRESS_0, PCI_BASE_ADDRESS_1,
            PCI_CAPABILITY_LIST, PCI_INTERRUPT_LINE, PCI_INTERRUPT_PIN,
        ];
        for i in 0..offsets.len() {
            for j in (i + 1)..offsets.len() {
                assert_ne!(offsets[i], offsets[j]);
            }
        }
    }

    #[test]
    fn test_bar_bits() {
        assert_eq!(PCI_BASE_ADDRESS_SPACE_IO, 1);
        assert_eq!(PCI_BASE_ADDRESS_SPACE_MEMORY, 0);
    }
}
