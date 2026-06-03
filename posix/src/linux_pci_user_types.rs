//! `<linux/pci.h>` — PCI userspace ABI (sysfs paths + config-space bits).
//!
//! Userspace tools (`lspci`, `setpci`, libpciaccess, DPDK's UIO probe,
//! VFIO bind scripts) discover devices and twiddle config-space
//! registers through these constants.

// ---------------------------------------------------------------------------
// sysfs roots
// ---------------------------------------------------------------------------

pub const SYSFS_PCI_DEVICES: &str = "/sys/bus/pci/devices";
pub const SYSFS_PCI_DRIVERS: &str = "/sys/bus/pci/drivers";
pub const SYSFS_PCI_RESCAN: &str = "/sys/bus/pci/rescan";
pub const SYSFS_PCI_SLOTS: &str = "/sys/bus/pci/slots";
pub const PROC_PCI_DEVICES: &str = "/proc/bus/pci/devices";

// ---------------------------------------------------------------------------
// PCI config-space layout sizes
// ---------------------------------------------------------------------------

/// Pre-PCIe Type-0 header — 256 bytes.
pub const PCI_CFG_SPACE_SIZE: u32 = 256;
/// PCIe extended config space.
pub const PCI_CFG_SPACE_EXP_SIZE: u32 = 4096;
/// PCI base-address-register count for a Type-0 header.
pub const PCI_STD_NUM_BARS: u32 = 6;

// ---------------------------------------------------------------------------
// Config-space register offsets (header bytes 0x00..0x40)
// ---------------------------------------------------------------------------

pub const PCI_VENDOR_ID: u32 = 0x00;
pub const PCI_DEVICE_ID: u32 = 0x02;
pub const PCI_COMMAND: u32 = 0x04;
pub const PCI_STATUS: u32 = 0x06;
pub const PCI_REVISION_ID: u32 = 0x08;
pub const PCI_CLASS_PROG: u32 = 0x09;
pub const PCI_CLASS_DEVICE: u32 = 0x0A;
pub const PCI_HEADER_TYPE: u32 = 0x0E;
pub const PCI_BASE_ADDRESS_0: u32 = 0x10;
pub const PCI_SUBSYSTEM_VENDOR_ID: u32 = 0x2C;
pub const PCI_SUBSYSTEM_ID: u32 = 0x2E;
pub const PCI_CAPABILITY_LIST: u32 = 0x34;
pub const PCI_INTERRUPT_LINE: u32 = 0x3C;
pub const PCI_INTERRUPT_PIN: u32 = 0x3D;

// ---------------------------------------------------------------------------
// `PCI_COMMAND` register bits
// ---------------------------------------------------------------------------

pub const PCI_COMMAND_IO: u16 = 1 << 0;
pub const PCI_COMMAND_MEMORY: u16 = 1 << 1;
pub const PCI_COMMAND_MASTER: u16 = 1 << 2;
pub const PCI_COMMAND_SPECIAL: u16 = 1 << 3;
pub const PCI_COMMAND_INVALIDATE: u16 = 1 << 4;
pub const PCI_COMMAND_VGA_PALETTE: u16 = 1 << 5;
pub const PCI_COMMAND_PARITY: u16 = 1 << 6;
pub const PCI_COMMAND_WAIT: u16 = 1 << 7;
pub const PCI_COMMAND_SERR: u16 = 1 << 8;
pub const PCI_COMMAND_FAST_BACK: u16 = 1 << 9;
pub const PCI_COMMAND_INTX_DISABLE: u16 = 1 << 10;

// ---------------------------------------------------------------------------
// `PCI_HEADER_TYPE` bits
// ---------------------------------------------------------------------------

pub const PCI_HEADER_TYPE_NORMAL: u8 = 0;
pub const PCI_HEADER_TYPE_BRIDGE: u8 = 1;
pub const PCI_HEADER_TYPE_CARDBUS: u8 = 2;
/// Bit 7: device is multi-function.
pub const PCI_HEADER_TYPE_MULTI_FUNCTION: u8 = 0x80;

// ---------------------------------------------------------------------------
// Capability ID values commonly walked by lspci
// ---------------------------------------------------------------------------

pub const PCI_CAP_ID_PM: u8 = 0x01;
pub const PCI_CAP_ID_AGP: u8 = 0x02;
pub const PCI_CAP_ID_VPD: u8 = 0x03;
pub const PCI_CAP_ID_SLOTID: u8 = 0x04;
pub const PCI_CAP_ID_MSI: u8 = 0x05;
pub const PCI_CAP_ID_CHSWP: u8 = 0x06;
pub const PCI_CAP_ID_PCIX: u8 = 0x07;
pub const PCI_CAP_ID_VNDR: u8 = 0x09;
pub const PCI_CAP_ID_EXP: u8 = 0x10;
pub const PCI_CAP_ID_MSIX: u8 = 0x11;
pub const PCI_CAP_ID_SATA: u8 = 0x12;
pub const PCI_CAP_ID_AF: u8 = 0x13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_pci_paths() {
        assert!(SYSFS_PCI_DEVICES.starts_with("/sys/bus/pci/"));
        assert!(SYSFS_PCI_DRIVERS.starts_with("/sys/bus/pci/"));
        assert!(SYSFS_PCI_RESCAN.ends_with("/rescan"));
        assert!(SYSFS_PCI_SLOTS.ends_with("/slots"));
        assert!(PROC_PCI_DEVICES.starts_with("/proc/bus/pci/"));
    }

    #[test]
    fn test_config_space_sizes() {
        // Legacy = 256 B, extended = 4 KiB.
        assert_eq!(PCI_CFG_SPACE_SIZE, 256);
        assert_eq!(PCI_CFG_SPACE_EXP_SIZE, 4096);
        assert!(PCI_CFG_SPACE_EXP_SIZE > PCI_CFG_SPACE_SIZE);
        // Six standard BARs.
        assert_eq!(PCI_STD_NUM_BARS, 6);
    }

    #[test]
    fn test_header_register_offsets_in_range() {
        // Every offset listed must fall inside the legacy 256-byte header.
        let o = [
            PCI_VENDOR_ID,
            PCI_DEVICE_ID,
            PCI_COMMAND,
            PCI_STATUS,
            PCI_REVISION_ID,
            PCI_CLASS_PROG,
            PCI_CLASS_DEVICE,
            PCI_HEADER_TYPE,
            PCI_BASE_ADDRESS_0,
            PCI_SUBSYSTEM_VENDOR_ID,
            PCI_SUBSYSTEM_ID,
            PCI_CAPABILITY_LIST,
            PCI_INTERRUPT_LINE,
            PCI_INTERRUPT_PIN,
        ];
        for v in o {
            assert!(v < PCI_CFG_SPACE_SIZE);
        }
        // Vendor/device ID pair are the first two halfwords.
        assert_eq!(PCI_VENDOR_ID, 0);
        assert_eq!(PCI_DEVICE_ID, 2);
    }

    #[test]
    fn test_command_bits_single_bit_low_11() {
        let c = [
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
        let mut or = 0u16;
        for (i, &v) in c.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1 << i);
            or |= v;
        }
        // Eleven bits = 0x7FF.
        assert_eq!(or, 0x07FF);
    }

    #[test]
    fn test_header_type_layout() {
        assert_eq!(PCI_HEADER_TYPE_NORMAL, 0);
        assert_eq!(PCI_HEADER_TYPE_BRIDGE, 1);
        assert_eq!(PCI_HEADER_TYPE_CARDBUS, 2);
        // Bit 7 marks multi-function.
        assert_eq!(PCI_HEADER_TYPE_MULTI_FUNCTION, 0x80);
        assert!(PCI_HEADER_TYPE_MULTI_FUNCTION.is_power_of_two());
    }

    #[test]
    fn test_capability_ids_distinct_and_known() {
        let c = [
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
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // Anchors: PCIe = 0x10, MSI-X = 0x11.
        assert_eq!(PCI_CAP_ID_EXP, 0x10);
        assert_eq!(PCI_CAP_ID_MSIX, 0x11);
    }
}
