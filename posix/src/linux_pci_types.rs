//! `<linux/pci_regs.h>` — PCI/PCIe register and capability constants.
//!
//! PCI (Peripheral Component Interconnect) and PCIe are the primary
//! hardware interconnects for GPUs, NICs, NVMe SSDs, and most other
//! high-performance devices. Configuration space registers control
//! device addressing, interrupts, power management, and capabilities.

// ---------------------------------------------------------------------------
// PCI configuration space header registers
// ---------------------------------------------------------------------------

/// Vendor ID register offset.
pub const PCI_VENDOR_ID: u32 = 0x00;
/// Device ID register offset.
pub const PCI_DEVICE_ID: u32 = 0x02;
/// Command register offset.
pub const PCI_COMMAND: u32 = 0x04;
/// Status register offset.
pub const PCI_STATUS: u32 = 0x06;
/// Revision ID register offset.
pub const PCI_REVISION_ID: u32 = 0x08;
/// Class code register offset.
pub const PCI_CLASS_PROG: u32 = 0x09;
/// Subclass register offset.
pub const PCI_CLASS_DEVICE: u32 = 0x0A;
/// Header type register offset.
pub const PCI_HEADER_TYPE: u32 = 0x0E;
/// BAR0 register offset.
pub const PCI_BASE_ADDRESS_0: u32 = 0x10;
/// Interrupt line register offset.
pub const PCI_INTERRUPT_LINE: u32 = 0x3C;
/// Interrupt pin register offset.
pub const PCI_INTERRUPT_PIN: u32 = 0x3D;

// ---------------------------------------------------------------------------
// PCI command register bits
// ---------------------------------------------------------------------------

/// Enable I/O space.
pub const PCI_COMMAND_IO: u16 = 0x0001;
/// Enable memory space.
pub const PCI_COMMAND_MEMORY: u16 = 0x0002;
/// Enable bus mastering.
pub const PCI_COMMAND_MASTER: u16 = 0x0004;
/// Enable interrupt disable.
pub const PCI_COMMAND_INTX_DISABLE: u16 = 0x0400;

// ---------------------------------------------------------------------------
// PCI capability IDs
// ---------------------------------------------------------------------------

/// Power management.
pub const PCI_CAP_ID_PM: u8 = 0x01;
/// MSI (Message Signaled Interrupts).
pub const PCI_CAP_ID_MSI: u8 = 0x05;
/// MSI-X.
pub const PCI_CAP_ID_MSIX: u8 = 0x11;
/// PCI Express.
pub const PCI_CAP_ID_EXP: u8 = 0x10;

// ---------------------------------------------------------------------------
// PCIe device types
// ---------------------------------------------------------------------------

/// PCIe endpoint.
pub const PCI_EXP_TYPE_ENDPOINT: u32 = 0x0;
/// PCIe root port.
pub const PCI_EXP_TYPE_ROOT_PORT: u32 = 0x4;
/// PCIe upstream switch port.
pub const PCI_EXP_TYPE_UPSTREAM: u32 = 0x5;
/// PCIe downstream switch port.
pub const PCI_EXP_TYPE_DOWNSTREAM: u32 = 0x6;

// ---------------------------------------------------------------------------
// PCI power states
// ---------------------------------------------------------------------------

/// Fully operational.
pub const PCI_D0: u32 = 0;
/// Light sleep.
pub const PCI_D1: u32 = 1;
/// Deeper sleep.
pub const PCI_D2: u32 = 2;
/// Deepest sleep (device off but responds to PME).
pub const PCI_D3HOT: u32 = 3;
/// Power removed.
pub const PCI_D3COLD: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_offsets_distinct() {
        let regs = [
            PCI_VENDOR_ID,
            PCI_DEVICE_ID,
            PCI_COMMAND,
            PCI_STATUS,
            PCI_REVISION_ID,
            PCI_CLASS_PROG,
            PCI_CLASS_DEVICE,
            PCI_HEADER_TYPE,
            PCI_BASE_ADDRESS_0,
            PCI_INTERRUPT_LINE,
            PCI_INTERRUPT_PIN,
        ];
        for i in 0..regs.len() {
            for j in (i + 1)..regs.len() {
                assert_ne!(regs[i], regs[j]);
            }
        }
    }

    #[test]
    fn test_command_bits_no_overlap() {
        let bits = [
            PCI_COMMAND_IO,
            PCI_COMMAND_MEMORY,
            PCI_COMMAND_MASTER,
            PCI_COMMAND_INTX_DISABLE,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_cap_ids_distinct() {
        let caps = [
            PCI_CAP_ID_PM,
            PCI_CAP_ID_MSI,
            PCI_CAP_ID_MSIX,
            PCI_CAP_ID_EXP,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_power_states_ascending() {
        assert!(PCI_D0 < PCI_D1);
        assert!(PCI_D1 < PCI_D2);
        assert!(PCI_D2 < PCI_D3HOT);
        assert!(PCI_D3HOT < PCI_D3COLD);
    }
}
