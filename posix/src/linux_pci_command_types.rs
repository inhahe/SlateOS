//! `<linux/pci_regs.h>` — PCI command and status register bit constants.
//!
//! The PCI Command register controls a device's response to I/O and
//! memory transactions, interrupt generation, and bus mastering.
//! The Status register reports error conditions and capabilities.
//! These are the most frequently accessed PCI configuration registers.

// ---------------------------------------------------------------------------
// PCI Command register bits (offset 0x04, 16-bit)
// ---------------------------------------------------------------------------

/// Enable response to I/O space accesses.
pub const PCI_COMMAND_IO: u16 = 0x0001;
/// Enable response to memory space accesses.
pub const PCI_COMMAND_MEMORY: u16 = 0x0002;
/// Enable bus mastering (device can initiate DMA).
pub const PCI_COMMAND_MASTER: u16 = 0x0004;
/// Enable response to special cycles.
pub const PCI_COMMAND_SPECIAL: u16 = 0x0008;
/// Enable Memory Write and Invalidate.
pub const PCI_COMMAND_INVALIDATE: u16 = 0x0010;
/// Enable palette snooping (VGA compatibility).
pub const PCI_COMMAND_VGA_PALETTE: u16 = 0x0020;
/// Enable parity error response.
pub const PCI_COMMAND_PARITY: u16 = 0x0040;
/// Enable address/data stepping (legacy).
pub const PCI_COMMAND_WAIT: u16 = 0x0080;
/// Enable SERR# driver.
pub const PCI_COMMAND_SERR: u16 = 0x0100;
/// Enable back-to-back writes (legacy).
pub const PCI_COMMAND_FAST_BACK: u16 = 0x0200;
/// Disable INTx assertion.
pub const PCI_COMMAND_INTX_DISABLE: u16 = 0x0400;

// ---------------------------------------------------------------------------
// PCI Status register bits (offset 0x06, 16-bit)
// ---------------------------------------------------------------------------

/// Device has pending interrupt.
pub const PCI_STATUS_INTERRUPT: u16 = 0x0008;
/// Device has capabilities list.
pub const PCI_STATUS_CAP_LIST: u16 = 0x0010;
/// Device supports 66 MHz.
pub const PCI_STATUS_66MHZ: u16 = 0x0020;
/// Device supports back-to-back.
pub const PCI_STATUS_FAST_BACK: u16 = 0x0080;
/// Master data parity error.
pub const PCI_STATUS_PARITY: u16 = 0x0100;
/// Signaled target abort.
pub const PCI_STATUS_SIG_TARGET_ABORT: u16 = 0x0800;
/// Received target abort.
pub const PCI_STATUS_REC_TARGET_ABORT: u16 = 0x1000;
/// Received master abort.
pub const PCI_STATUS_REC_MASTER_ABORT: u16 = 0x2000;
/// Signaled system error.
pub const PCI_STATUS_SIG_SYSTEM_ERROR: u16 = 0x4000;
/// Detected parity error.
pub const PCI_STATUS_DETECTED_PARITY: u16 = 0x8000;

// ---------------------------------------------------------------------------
// Common PCI command combinations
// ---------------------------------------------------------------------------

/// Enable device for DMA (memory + bus master).
pub const PCI_COMMAND_DMA_ENABLE: u16 = PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER;
/// Enable device for MMIO + IO + DMA.
pub const PCI_COMMAND_ALL_IO: u16 = PCI_COMMAND_IO | PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER;

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
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_status_bits_distinct() {
        let bits = [
            PCI_STATUS_INTERRUPT,
            PCI_STATUS_CAP_LIST,
            PCI_STATUS_66MHZ,
            PCI_STATUS_FAST_BACK,
            PCI_STATUS_PARITY,
            PCI_STATUS_SIG_TARGET_ABORT,
            PCI_STATUS_REC_TARGET_ABORT,
            PCI_STATUS_REC_MASTER_ABORT,
            PCI_STATUS_SIG_SYSTEM_ERROR,
            PCI_STATUS_DETECTED_PARITY,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_dma_enable_combination() {
        assert_eq!(
            PCI_COMMAND_DMA_ENABLE,
            PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER
        );
        assert_ne!(PCI_COMMAND_DMA_ENABLE & PCI_COMMAND_MEMORY, 0);
        assert_ne!(PCI_COMMAND_DMA_ENABLE & PCI_COMMAND_MASTER, 0);
    }
}
