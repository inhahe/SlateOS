//! `<linux/pci_ids.h>` — Common PCI vendor and device ID constants.
//!
//! PCI devices are identified by vendor:device ID pairs. The PCI-SIG
//! assigns vendor IDs; each vendor assigns their own device IDs.
//! These constants cover the most commonly encountered vendors in
//! x86 desktop/server systems.

// ---------------------------------------------------------------------------
// Major PCI vendor IDs
// ---------------------------------------------------------------------------

/// Intel Corporation.
pub const PCI_VENDOR_ID_INTEL: u16 = 0x8086;
/// AMD (Advanced Micro Devices).
pub const PCI_VENDOR_ID_AMD: u16 = 0x1022;
/// NVIDIA Corporation.
pub const PCI_VENDOR_ID_NVIDIA: u16 = 0x10DE;
/// Realtek Semiconductor.
pub const PCI_VENDOR_ID_REALTEK: u16 = 0x10EC;
/// Broadcom (and former Avago/LSI).
pub const PCI_VENDOR_ID_BROADCOM: u16 = 0x14E4;
/// Qualcomm Atheros.
pub const PCI_VENDOR_ID_ATHEROS: u16 = 0x168C;
/// Red Hat / QEMU virtio.
pub const PCI_VENDOR_ID_REDHAT: u16 = 0x1AF4;
/// Samsung Electronics.
pub const PCI_VENDOR_ID_SAMSUNG: u16 = 0x144D;
/// Texas Instruments.
pub const PCI_VENDOR_ID_TI: u16 = 0x104C;
/// VIA Technologies.
pub const PCI_VENDOR_ID_VIA: u16 = 0x1106;
/// Marvell Technology.
pub const PCI_VENDOR_ID_MARVELL: u16 = 0x1B4B;
/// Mellanox Technologies.
pub const PCI_VENDOR_ID_MELLANOX: u16 = 0x15B3;

// ---------------------------------------------------------------------------
// Common PCI device IDs (Intel)
// ---------------------------------------------------------------------------

/// Intel 82574L Gigabit Ethernet (e1000e).
pub const PCI_DEVICE_ID_INTEL_82574L: u16 = 0x10D3;
/// Intel I210 Gigabit Ethernet.
pub const PCI_DEVICE_ID_INTEL_I210: u16 = 0x1533;
/// Intel I350 Gigabit Ethernet.
pub const PCI_DEVICE_ID_INTEL_I350: u16 = 0x1521;
/// Intel ICH9 AHCI controller.
pub const PCI_DEVICE_ID_INTEL_ICH9_AHCI: u16 = 0x2922;

// ---------------------------------------------------------------------------
// Special PCI IDs
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
            PCI_VENDOR_ID_MELLANOX,
        ];
        for i in 0..vendors.len() {
            for j in (i + 1)..vendors.len() {
                assert_ne!(vendors[i], vendors[j]);
            }
        }
    }

    #[test]
    fn test_intel_id() {
        assert_eq!(PCI_VENDOR_ID_INTEL, 0x8086);
    }

    #[test]
    fn test_any_id() {
        assert_eq!(PCI_ANY_ID, 0xFFFF);
    }

    #[test]
    fn test_device_ids_distinct() {
        let devs = [
            PCI_DEVICE_ID_INTEL_82574L, PCI_DEVICE_ID_INTEL_I210,
            PCI_DEVICE_ID_INTEL_I350, PCI_DEVICE_ID_INTEL_ICH9_AHCI,
        ];
        for i in 0..devs.len() {
            for j in (i + 1)..devs.len() {
                assert_ne!(devs[i], devs[j]);
            }
        }
    }
}
