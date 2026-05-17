//! `<linux/pci_regs.h>` — PCI configuration space register offsets.
//!
//! PCI devices expose a standardized configuration space accessible
//! via configuration read/write cycles. These offsets define the
//! standard header fields, capability structures, and PCIe extended
//! configuration space layout.

// ---------------------------------------------------------------------------
// Standard configuration header (Type 0)
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
/// Programming interface (8-bit).
pub const PCI_CLASS_PROG: u8 = 0x09;
/// Sub-class code (8-bit).
pub const PCI_CLASS_DEVICE: u8 = 0x0A;
/// Cache line size (8-bit).
pub const PCI_CACHE_LINE_SIZE: u8 = 0x0C;
/// Latency timer (8-bit).
pub const PCI_LATENCY_TIMER: u8 = 0x0D;
/// Header type (8-bit).
pub const PCI_HEADER_TYPE: u8 = 0x0E;
/// Subsystem vendor ID (16-bit).
pub const PCI_SUBSYSTEM_VENDOR_ID: u8 = 0x2C;
/// Subsystem ID (16-bit).
pub const PCI_SUBSYSTEM_ID: u8 = 0x2E;
/// Interrupt line (8-bit).
pub const PCI_INTERRUPT_LINE: u8 = 0x3C;
/// Interrupt pin (8-bit).
pub const PCI_INTERRUPT_PIN: u8 = 0x3D;
/// Capabilities pointer (8-bit).
pub const PCI_CAPABILITY_LIST: u8 = 0x34;

// ---------------------------------------------------------------------------
// BAR (Base Address Register) offsets
// ---------------------------------------------------------------------------

/// BAR 0.
pub const PCI_BASE_ADDRESS_0: u8 = 0x10;
/// BAR 1.
pub const PCI_BASE_ADDRESS_1: u8 = 0x14;
/// BAR 2.
pub const PCI_BASE_ADDRESS_2: u8 = 0x18;
/// BAR 3.
pub const PCI_BASE_ADDRESS_3: u8 = 0x1C;
/// BAR 4.
pub const PCI_BASE_ADDRESS_4: u8 = 0x20;
/// BAR 5.
pub const PCI_BASE_ADDRESS_5: u8 = 0x24;

// ---------------------------------------------------------------------------
// Command register bits
// ---------------------------------------------------------------------------

/// I/O space access enable.
pub const PCI_COMMAND_IO: u16 = 1 << 0;
/// Memory space access enable.
pub const PCI_COMMAND_MEMORY: u16 = 1 << 1;
/// Bus master enable.
pub const PCI_COMMAND_MASTER: u16 = 1 << 2;
/// INTx disable.
pub const PCI_COMMAND_INTX_DISABLE: u16 = 1 << 10;

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
/// Vendor specific.
pub const PCI_CAP_ID_VNDR: u8 = 0x09;
/// PCIe.
pub const PCI_CAP_ID_EXP: u8 = 0x10;
/// MSI-X.
pub const PCI_CAP_ID_MSIX: u8 = 0x11;

// ---------------------------------------------------------------------------
// PCIe extended capability IDs
// ---------------------------------------------------------------------------

/// Advanced Error Reporting.
pub const PCI_EXT_CAP_ID_AER: u16 = 0x0001;
/// Virtual Channel.
pub const PCI_EXT_CAP_ID_VC: u16 = 0x0002;
/// Serial Number.
pub const PCI_EXT_CAP_ID_DSN: u16 = 0x0003;
/// SR-IOV.
pub const PCI_EXT_CAP_ID_SRIOV: u16 = 0x0010;
/// Resizable BAR.
pub const PCI_EXT_CAP_ID_REBAR: u16 = 0x0015;
/// L1 PM Substates.
pub const PCI_EXT_CAP_ID_L1SS: u16 = 0x001E;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_offsets_distinct() {
        let offsets = [
            PCI_VENDOR_ID, PCI_DEVICE_ID, PCI_COMMAND, PCI_STATUS,
            PCI_REVISION_ID, PCI_CLASS_PROG, PCI_CLASS_DEVICE,
            PCI_CACHE_LINE_SIZE, PCI_LATENCY_TIMER, PCI_HEADER_TYPE,
            PCI_SUBSYSTEM_VENDOR_ID, PCI_SUBSYSTEM_ID,
            PCI_INTERRUPT_LINE, PCI_INTERRUPT_PIN, PCI_CAPABILITY_LIST,
        ];
        for i in 0..offsets.len() {
            for j in (i + 1)..offsets.len() {
                assert_ne!(offsets[i], offsets[j]);
            }
        }
    }

    #[test]
    fn test_bars_sequential() {
        assert_eq!(PCI_BASE_ADDRESS_1 - PCI_BASE_ADDRESS_0, 4);
        assert_eq!(PCI_BASE_ADDRESS_2 - PCI_BASE_ADDRESS_1, 4);
        assert_eq!(PCI_BASE_ADDRESS_3 - PCI_BASE_ADDRESS_2, 4);
        assert_eq!(PCI_BASE_ADDRESS_4 - PCI_BASE_ADDRESS_3, 4);
        assert_eq!(PCI_BASE_ADDRESS_5 - PCI_BASE_ADDRESS_4, 4);
    }

    #[test]
    fn test_command_bits_no_overlap() {
        let bits = [
            PCI_COMMAND_IO, PCI_COMMAND_MEMORY,
            PCI_COMMAND_MASTER, PCI_COMMAND_INTX_DISABLE,
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
            PCI_CAP_ID_PM, PCI_CAP_ID_AGP, PCI_CAP_ID_MSI,
            PCI_CAP_ID_PCIX, PCI_CAP_ID_VNDR, PCI_CAP_ID_EXP,
            PCI_CAP_ID_MSIX,
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
            PCI_EXT_CAP_ID_AER, PCI_EXT_CAP_ID_VC, PCI_EXT_CAP_ID_DSN,
            PCI_EXT_CAP_ID_SRIOV, PCI_EXT_CAP_ID_REBAR, PCI_EXT_CAP_ID_L1SS,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }
}
