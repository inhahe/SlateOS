//! `<linux/pci.h>` / `<linux/pci_regs.h>` — PCI configuration register constants.
//!
//! Standard PCI configuration space register offsets and class codes,
//! used by PCI device drivers, sysfs, and diagnostic tools (lspci).

// ---------------------------------------------------------------------------
// PCI configuration space registers (Type 0)
// ---------------------------------------------------------------------------

/// Vendor ID register.
pub const PCI_VENDOR_ID: u8 = 0x00;
/// Device ID register.
pub const PCI_DEVICE_ID: u8 = 0x02;
/// Command register.
pub const PCI_COMMAND: u8 = 0x04;
/// Status register.
pub const PCI_STATUS: u8 = 0x06;
/// Revision ID.
pub const PCI_REVISION_ID: u8 = 0x08;
/// Programming interface.
pub const PCI_CLASS_PROG: u8 = 0x09;
/// Subclass code.
pub const PCI_CLASS_DEVICE: u8 = 0x0A;
/// Cache line size.
pub const PCI_CACHE_LINE_SIZE: u8 = 0x0C;
/// Latency timer.
pub const PCI_LATENCY_TIMER: u8 = 0x0D;
/// Header type.
pub const PCI_HEADER_TYPE: u8 = 0x0E;
/// BIST.
pub const PCI_BIST: u8 = 0x0F;

/// BAR0.
pub const PCI_BASE_ADDRESS_0: u8 = 0x10;
/// BAR1.
pub const PCI_BASE_ADDRESS_1: u8 = 0x14;
/// BAR2.
pub const PCI_BASE_ADDRESS_2: u8 = 0x18;
/// BAR3.
pub const PCI_BASE_ADDRESS_3: u8 = 0x1C;
/// BAR4.
pub const PCI_BASE_ADDRESS_4: u8 = 0x20;
/// BAR5.
pub const PCI_BASE_ADDRESS_5: u8 = 0x24;

/// Subsystem vendor ID.
pub const PCI_SUBSYSTEM_VENDOR_ID: u8 = 0x2C;
/// Subsystem ID.
pub const PCI_SUBSYSTEM_ID: u8 = 0x2E;
/// Expansion ROM base.
pub const PCI_ROM_ADDRESS: u8 = 0x30;
/// Capability pointer.
pub const PCI_CAPABILITY_LIST: u8 = 0x34;
/// Interrupt line.
pub const PCI_INTERRUPT_LINE: u8 = 0x3C;
/// Interrupt pin.
pub const PCI_INTERRUPT_PIN: u8 = 0x3D;

// ---------------------------------------------------------------------------
// PCI command register bits
// ---------------------------------------------------------------------------

/// Enable I/O space.
pub const PCI_COMMAND_IO: u16 = 0x0001;
/// Enable memory space.
pub const PCI_COMMAND_MEMORY: u16 = 0x0002;
/// Enable bus master.
pub const PCI_COMMAND_MASTER: u16 = 0x0004;
/// Enable special cycles.
pub const PCI_COMMAND_SPECIAL: u16 = 0x0008;
/// Memory write and invalidate.
pub const PCI_COMMAND_INVALIDATE: u16 = 0x0010;
/// Enable VGA palette snoop.
pub const PCI_COMMAND_VGA_PALETTE: u16 = 0x0020;
/// Enable parity error response.
pub const PCI_COMMAND_PARITY: u16 = 0x0040;
/// Enable SERR#.
pub const PCI_COMMAND_SERR: u16 = 0x0100;
/// Enable fast back-to-back writes.
pub const PCI_COMMAND_FAST_BACK: u16 = 0x0200;
/// Disable interrupts.
pub const PCI_COMMAND_INTX_DISABLE: u16 = 0x0400;

// ---------------------------------------------------------------------------
// PCI class codes (high byte of class)
// ---------------------------------------------------------------------------

/// Unclassified device.
pub const PCI_CLASS_NOT_DEFINED: u16 = 0x0000;
/// Mass storage controller.
pub const PCI_CLASS_STORAGE: u16 = 0x0100;
/// Network controller.
pub const PCI_CLASS_NETWORK: u16 = 0x0200;
/// Display controller.
pub const PCI_CLASS_DISPLAY: u16 = 0x0300;
/// Multimedia controller.
pub const PCI_CLASS_MULTIMEDIA: u16 = 0x0400;
/// Memory controller.
pub const PCI_CLASS_MEMORY: u16 = 0x0500;
/// Bridge device.
pub const PCI_CLASS_BRIDGE: u16 = 0x0600;
/// Communication controller.
pub const PCI_CLASS_COMMUNICATION: u16 = 0x0700;
/// System peripheral.
pub const PCI_CLASS_SYSTEM: u16 = 0x0800;
/// Input device.
pub const PCI_CLASS_INPUT: u16 = 0x0900;
/// Serial bus controller.
pub const PCI_CLASS_SERIAL: u16 = 0x0C00;
/// Wireless controller.
pub const PCI_CLASS_WIRELESS: u16 = 0x0D00;
/// Processing accelerator.
pub const PCI_CLASS_ACCELERATOR: u16 = 0x1200;

// ---------------------------------------------------------------------------
// BAR flags
// ---------------------------------------------------------------------------

/// BAR is I/O space (bit 0).
pub const PCI_BASE_ADDRESS_SPACE_IO: u32 = 0x01;
/// BAR address mask for memory.
pub const PCI_BASE_ADDRESS_MEM_MASK: u32 = !0x0F;
/// BAR is 64-bit (memory type bits).
pub const PCI_BASE_ADDRESS_MEM_TYPE_64: u32 = 0x04;
/// BAR is prefetchable.
pub const PCI_BASE_ADDRESS_MEM_PREFETCH: u32 = 0x08;

// ---------------------------------------------------------------------------
// Capability IDs
// ---------------------------------------------------------------------------

/// Power Management.
pub const PCI_CAP_ID_PM: u8 = 0x01;
/// AGP.
pub const PCI_CAP_ID_AGP: u8 = 0x02;
/// MSI.
pub const PCI_CAP_ID_MSI: u8 = 0x05;
/// PCI-X.
pub const PCI_CAP_ID_PCIX: u8 = 0x07;
/// Vendor-specific.
pub const PCI_CAP_ID_VNDR: u8 = 0x09;
/// MSI-X.
pub const PCI_CAP_ID_MSIX: u8 = 0x11;
/// PCIe.
pub const PCI_CAP_ID_EXP: u8 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_registers_ordered() {
        assert_eq!(PCI_VENDOR_ID, 0x00);
        assert_eq!(PCI_DEVICE_ID, 0x02);
        assert_eq!(PCI_COMMAND, 0x04);
        assert_eq!(PCI_STATUS, 0x06);
    }

    #[test]
    fn test_bars_sequential() {
        assert_eq!(PCI_BASE_ADDRESS_0, 0x10);
        assert_eq!(PCI_BASE_ADDRESS_1, 0x14);
        assert_eq!(PCI_BASE_ADDRESS_2, 0x18);
        assert_eq!(PCI_BASE_ADDRESS_3, 0x1C);
        assert_eq!(PCI_BASE_ADDRESS_4, 0x20);
        assert_eq!(PCI_BASE_ADDRESS_5, 0x24);
    }

    #[test]
    fn test_command_bits_powers_of_two() {
        let bits = [
            PCI_COMMAND_IO, PCI_COMMAND_MEMORY, PCI_COMMAND_MASTER,
            PCI_COMMAND_SPECIAL, PCI_COMMAND_INVALIDATE,
            PCI_COMMAND_VGA_PALETTE, PCI_COMMAND_PARITY,
            PCI_COMMAND_SERR, PCI_COMMAND_FAST_BACK,
            PCI_COMMAND_INTX_DISABLE,
        ];
        for b in &bits {
            assert!(b.is_power_of_two(), "cmd bit {b:#06x} not power of 2");
        }
    }

    #[test]
    fn test_class_codes_distinct() {
        let classes = [
            PCI_CLASS_NOT_DEFINED, PCI_CLASS_STORAGE, PCI_CLASS_NETWORK,
            PCI_CLASS_DISPLAY, PCI_CLASS_MULTIMEDIA, PCI_CLASS_MEMORY,
            PCI_CLASS_BRIDGE, PCI_CLASS_SERIAL, PCI_CLASS_WIRELESS,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_cap_ids_distinct() {
        let caps = [
            PCI_CAP_ID_PM, PCI_CAP_ID_AGP, PCI_CAP_ID_MSI,
            PCI_CAP_ID_PCIX, PCI_CAP_ID_VNDR, PCI_CAP_ID_MSIX,
            PCI_CAP_ID_EXP,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }
}
