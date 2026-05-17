//! `<linux/pci_ids.h>` — Common PCI vendor and device ID constants.
//!
//! PCI vendor IDs are assigned by PCI-SIG. This module contains
//! a representative set of well-known vendor IDs and common device
//! class codes used for driver matching and device identification.

// ---------------------------------------------------------------------------
// Vendor IDs
// ---------------------------------------------------------------------------

/// Intel Corporation.
pub const PCI_VENDOR_ID_INTEL: u16 = 0x8086;
/// AMD (Advanced Micro Devices).
pub const PCI_VENDOR_ID_AMD: u16 = 0x1022;
/// NVIDIA Corporation.
pub const PCI_VENDOR_ID_NVIDIA: u16 = 0x10DE;
/// Realtek Semiconductor.
pub const PCI_VENDOR_ID_REALTEK: u16 = 0x10EC;
/// Broadcom Inc.
pub const PCI_VENDOR_ID_BROADCOM: u16 = 0x14E4;
/// Qualcomm Atheros.
pub const PCI_VENDOR_ID_ATHEROS: u16 = 0x168C;
/// Red Hat (virtio).
pub const PCI_VENDOR_ID_REDHAT: u16 = 0x1AF4;
/// Samsung Electronics.
pub const PCI_VENDOR_ID_SAMSUNG: u16 = 0x144D;
/// Texas Instruments.
pub const PCI_VENDOR_ID_TI: u16 = 0x104C;
/// VIA Technologies.
pub const PCI_VENDOR_ID_VIA: u16 = 0x1106;
/// Marvell Technology.
pub const PCI_VENDOR_ID_MARVELL: u16 = 0x11AB;
/// MediaTek.
pub const PCI_VENDOR_ID_MEDIATEK: u16 = 0x14C3;

// ---------------------------------------------------------------------------
// Device class codes (upper byte = base class)
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
/// Simple communication controller.
pub const PCI_CLASS_COMMUNICATION: u16 = 0x0700;
/// Generic system peripheral.
pub const PCI_CLASS_SYSTEM: u16 = 0x0800;
/// Input device controller.
pub const PCI_CLASS_INPUT: u16 = 0x0900;
/// Serial bus controller.
pub const PCI_CLASS_SERIAL: u16 = 0x0C00;
/// Wireless controller.
pub const PCI_CLASS_WIRELESS: u16 = 0x0D00;
/// Encryption controller.
pub const PCI_CLASS_CRYPT: u16 = 0x1000;
/// Signal processing controller.
pub const PCI_CLASS_SIGNAL_PROC: u16 = 0x1100;

// ---------------------------------------------------------------------------
// Subclass codes (storage)
// ---------------------------------------------------------------------------

/// SCSI storage controller.
pub const PCI_SUBCLASS_STORAGE_SCSI: u8 = 0x00;
/// IDE controller.
pub const PCI_SUBCLASS_STORAGE_IDE: u8 = 0x01;
/// SATA controller.
pub const PCI_SUBCLASS_STORAGE_SATA: u8 = 0x06;
/// NVM controller (NVMe).
pub const PCI_SUBCLASS_STORAGE_NVM: u8 = 0x08;

// ---------------------------------------------------------------------------
// Special device IDs
// ---------------------------------------------------------------------------

/// Any vendor (wildcard for driver matching).
pub const PCI_ANY_ID: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vendor_ids_distinct() {
        let vendors = [
            PCI_VENDOR_ID_INTEL, PCI_VENDOR_ID_AMD, PCI_VENDOR_ID_NVIDIA,
            PCI_VENDOR_ID_REALTEK, PCI_VENDOR_ID_BROADCOM,
            PCI_VENDOR_ID_ATHEROS, PCI_VENDOR_ID_REDHAT,
            PCI_VENDOR_ID_SAMSUNG, PCI_VENDOR_ID_TI,
            PCI_VENDOR_ID_VIA, PCI_VENDOR_ID_MARVELL,
            PCI_VENDOR_ID_MEDIATEK,
        ];
        for i in 0..vendors.len() {
            for j in (i + 1)..vendors.len() {
                assert_ne!(vendors[i], vendors[j]);
            }
        }
    }

    #[test]
    fn test_class_codes_distinct() {
        let classes = [
            PCI_CLASS_NOT_DEFINED, PCI_CLASS_STORAGE, PCI_CLASS_NETWORK,
            PCI_CLASS_DISPLAY, PCI_CLASS_MULTIMEDIA, PCI_CLASS_MEMORY,
            PCI_CLASS_BRIDGE, PCI_CLASS_COMMUNICATION, PCI_CLASS_SYSTEM,
            PCI_CLASS_INPUT, PCI_CLASS_SERIAL, PCI_CLASS_WIRELESS,
            PCI_CLASS_CRYPT, PCI_CLASS_SIGNAL_PROC,
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
            PCI_SUBCLASS_STORAGE_SCSI, PCI_SUBCLASS_STORAGE_IDE,
            PCI_SUBCLASS_STORAGE_SATA, PCI_SUBCLASS_STORAGE_NVM,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_intel_vendor_id() {
        assert_eq!(PCI_VENDOR_ID_INTEL, 0x8086);
    }
}
