//! `<linux/pci_regs.h>` (config subset) — PCI configuration space constants.
//!
//! Every PCI device has a 256-byte (PCI) or 4096-byte (PCIe) configuration
//! space containing device identity (vendor/device ID), BAR (Base Address
//! Register) assignments, interrupt configuration, and capability
//! pointers. The configuration space is accessed via ECAM (Enhanced
//! Configuration Access Mechanism) on modern systems or via I/O ports
//! 0xCF8/0xCFC on legacy x86.

// ---------------------------------------------------------------------------
// Configuration space header offsets (Type 0 — endpoint)
// ---------------------------------------------------------------------------

/// Vendor ID (16-bit).
pub const PCI_VENDOR_ID: u32 = 0x00;
/// Device ID (16-bit).
pub const PCI_DEVICE_ID: u32 = 0x02;
/// Command register (16-bit).
pub const PCI_COMMAND: u32 = 0x04;
/// Status register (16-bit).
pub const PCI_STATUS: u32 = 0x06;
/// Revision ID (8-bit).
pub const PCI_REVISION_ID: u32 = 0x08;
/// Class code (24-bit: class, subclass, prog-if).
pub const PCI_CLASS_PROG: u32 = 0x09;
/// Subclass code (8-bit).
pub const PCI_CLASS_DEVICE: u32 = 0x0A;
/// Cache line size (8-bit).
pub const PCI_CACHE_LINE_SIZE: u32 = 0x0C;
/// Latency timer (8-bit).
pub const PCI_LATENCY_TIMER: u32 = 0x0D;
/// Header type (8-bit).
pub const PCI_HEADER_TYPE: u32 = 0x0E;
/// BIST register (8-bit).
pub const PCI_BIST: u32 = 0x0F;
/// BAR 0 (32-bit).
pub const PCI_BASE_ADDRESS_0: u32 = 0x10;
/// BAR 1.
pub const PCI_BASE_ADDRESS_1: u32 = 0x14;
/// BAR 2.
pub const PCI_BASE_ADDRESS_2: u32 = 0x18;
/// BAR 3.
pub const PCI_BASE_ADDRESS_3: u32 = 0x1C;
/// BAR 4.
pub const PCI_BASE_ADDRESS_4: u32 = 0x20;
/// BAR 5.
pub const PCI_BASE_ADDRESS_5: u32 = 0x24;
/// Subsystem vendor ID.
pub const PCI_SUBSYSTEM_VENDOR_ID: u32 = 0x2C;
/// Subsystem ID.
pub const PCI_SUBSYSTEM_ID: u32 = 0x2E;
/// Expansion ROM base address.
pub const PCI_ROM_ADDRESS: u32 = 0x30;
/// Capability pointer.
pub const PCI_CAPABILITY_LIST: u32 = 0x34;
/// Interrupt line (8-bit).
pub const PCI_INTERRUPT_LINE: u32 = 0x3C;
/// Interrupt pin (8-bit).
pub const PCI_INTERRUPT_PIN: u32 = 0x3D;

// ---------------------------------------------------------------------------
// PCI command register bits
// ---------------------------------------------------------------------------

/// I/O space access enable.
pub const PCI_COMMAND_IO: u32 = 0x01;
/// Memory space access enable.
pub const PCI_COMMAND_MEMORY: u32 = 0x02;
/// Bus master enable.
pub const PCI_COMMAND_MASTER: u32 = 0x04;
/// Interrupt disable.
pub const PCI_COMMAND_INTX_DISABLE: u32 = 0x400;

// ---------------------------------------------------------------------------
// PCI header types
// ---------------------------------------------------------------------------

/// Type 0: standard endpoint device.
pub const PCI_HEADER_TYPE_NORMAL: u32 = 0x00;
/// Type 1: PCI-to-PCI bridge.
pub const PCI_HEADER_TYPE_BRIDGE: u32 = 0x01;
/// Type 2: CardBus bridge.
pub const PCI_HEADER_TYPE_CARDBUS: u32 = 0x02;
/// Multi-function device flag.
pub const PCI_HEADER_TYPE_MFD: u32 = 0x80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_offsets_sequential() {
        assert_eq!(PCI_BASE_ADDRESS_1, PCI_BASE_ADDRESS_0 + 4);
        assert_eq!(PCI_BASE_ADDRESS_2, PCI_BASE_ADDRESS_1 + 4);
        assert_eq!(PCI_BASE_ADDRESS_3, PCI_BASE_ADDRESS_2 + 4);
    }

    #[test]
    fn test_command_bits_no_overlap() {
        let bits = [PCI_COMMAND_IO, PCI_COMMAND_MEMORY, PCI_COMMAND_MASTER];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_header_types_distinct() {
        assert_ne!(PCI_HEADER_TYPE_NORMAL, PCI_HEADER_TYPE_BRIDGE);
        assert_ne!(PCI_HEADER_TYPE_BRIDGE, PCI_HEADER_TYPE_CARDBUS);
    }
}
