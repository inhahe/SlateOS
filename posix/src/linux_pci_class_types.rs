//! `<linux/pci_ids.h>` (class subset) — PCI device class codes.
//!
//! PCI class codes categorize devices by function (like USB device
//! classes). The full class code is 24 bits: base class (8 bits),
//! sub-class (8 bits), and programming interface (8 bits). The kernel
//! uses class codes for generic driver matching when no vendor/device
//! specific driver exists.

// ---------------------------------------------------------------------------
// PCI base class codes
// ---------------------------------------------------------------------------

/// Pre-2.0 device (class code undefined).
pub const PCI_BASE_CLASS_OLD: u32 = 0x00;
/// Mass storage controller.
pub const PCI_BASE_CLASS_STORAGE: u32 = 0x01;
/// Network controller.
pub const PCI_BASE_CLASS_NETWORK: u32 = 0x02;
/// Display controller.
pub const PCI_BASE_CLASS_DISPLAY: u32 = 0x03;
/// Multimedia device.
pub const PCI_BASE_CLASS_MULTIMEDIA: u32 = 0x04;
/// Memory controller.
pub const PCI_BASE_CLASS_MEMORY: u32 = 0x05;
/// Bridge device.
pub const PCI_BASE_CLASS_BRIDGE: u32 = 0x06;
/// Communication controller.
pub const PCI_BASE_CLASS_COMMUNICATION: u32 = 0x07;
/// System peripheral.
pub const PCI_BASE_CLASS_SYSTEM: u32 = 0x08;
/// Input device controller.
pub const PCI_BASE_CLASS_INPUT: u32 = 0x09;
/// Docking station.
pub const PCI_BASE_CLASS_DOCKING: u32 = 0x0A;
/// Processor.
pub const PCI_BASE_CLASS_PROCESSOR: u32 = 0x0B;
/// Serial bus controller (USB, FireWire, etc.).
pub const PCI_BASE_CLASS_SERIAL: u32 = 0x0C;
/// Wireless controller.
pub const PCI_BASE_CLASS_WIRELESS: u32 = 0x0D;
/// Intelligent I/O controller.
pub const PCI_BASE_CLASS_INTELLIGENT: u32 = 0x0E;
/// Satellite communication controller.
pub const PCI_BASE_CLASS_SATELLITE: u32 = 0x0F;
/// Encryption/decryption controller.
pub const PCI_BASE_CLASS_CRYPT: u32 = 0x10;
/// Data acquisition / signal processing.
pub const PCI_BASE_CLASS_SIGNAL_PROCESSING: u32 = 0x11;

// ---------------------------------------------------------------------------
// Storage subclass codes
// ---------------------------------------------------------------------------

/// SCSI bus controller.
pub const PCI_CLASS_STORAGE_SCSI: u32 = 0x0100;
/// IDE controller.
pub const PCI_CLASS_STORAGE_IDE: u32 = 0x0101;
/// Floppy disk controller.
pub const PCI_CLASS_STORAGE_FLOPPY: u32 = 0x0102;
/// RAID controller.
pub const PCI_CLASS_STORAGE_RAID: u32 = 0x0104;
/// SATA controller.
pub const PCI_CLASS_STORAGE_SATA: u32 = 0x0106;
/// NVMe controller.
pub const PCI_CLASS_STORAGE_NVME: u32 = 0x0108;

// ---------------------------------------------------------------------------
// Display subclass codes
// ---------------------------------------------------------------------------

/// VGA compatible controller.
pub const PCI_CLASS_DISPLAY_VGA: u32 = 0x0300;
/// XGA controller.
pub const PCI_CLASS_DISPLAY_XGA: u32 = 0x0301;
/// 3D controller (non-VGA).
pub const PCI_CLASS_DISPLAY_3D: u32 = 0x0302;

// ---------------------------------------------------------------------------
// Serial bus subclass codes
// ---------------------------------------------------------------------------

/// USB controller.
pub const PCI_CLASS_SERIAL_USB: u32 = 0x0C03;
/// FireWire (IEEE 1394) controller.
pub const PCI_CLASS_SERIAL_FIREWIRE: u32 = 0x0C00;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_classes_distinct() {
        let classes = [
            PCI_BASE_CLASS_OLD, PCI_BASE_CLASS_STORAGE,
            PCI_BASE_CLASS_NETWORK, PCI_BASE_CLASS_DISPLAY,
            PCI_BASE_CLASS_MULTIMEDIA, PCI_BASE_CLASS_MEMORY,
            PCI_BASE_CLASS_BRIDGE, PCI_BASE_CLASS_COMMUNICATION,
            PCI_BASE_CLASS_SYSTEM, PCI_BASE_CLASS_INPUT,
            PCI_BASE_CLASS_SERIAL, PCI_BASE_CLASS_WIRELESS,
            PCI_BASE_CLASS_CRYPT, PCI_BASE_CLASS_SIGNAL_PROCESSING,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_storage_subclasses_distinct() {
        let subs = [
            PCI_CLASS_STORAGE_SCSI, PCI_CLASS_STORAGE_IDE,
            PCI_CLASS_STORAGE_FLOPPY, PCI_CLASS_STORAGE_RAID,
            PCI_CLASS_STORAGE_SATA, PCI_CLASS_STORAGE_NVME,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }
}
