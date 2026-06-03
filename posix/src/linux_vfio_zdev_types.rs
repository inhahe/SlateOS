//! `<linux/vfio_zdev.h>` — VFIO zPCI device-info constants (s390).
//!
//! Constants describing per-device IRQ and PCI-function information
//! returned by VFIO_DEVICE_GET_INFO for s390 zPCI devices. KVM tools
//! and userspace s390 emulators consume these.

// ---------------------------------------------------------------------------
// Capability identifiers (struct vfio_info_cap_header.id)
// ---------------------------------------------------------------------------

/// Basic zPCI identifier (function/PCHID/PFGID).
pub const VFIO_DEVICE_INFO_CAP_ZPCI_BASE: u16 = 1;
/// PCI group capabilities (DMA limits etc.).
pub const VFIO_DEVICE_INFO_CAP_ZPCI_GROUP: u16 = 2;
/// Per-utility-string (function description).
pub const VFIO_DEVICE_INFO_CAP_ZPCI_UTIL: u16 = 3;
/// Per-function PFIP (programming-interface) identifiers.
pub const VFIO_DEVICE_INFO_CAP_ZPCI_PFIP: u16 = 4;

// ---------------------------------------------------------------------------
// Capability versions (struct vfio_info_cap_header.version)
// ---------------------------------------------------------------------------

/// Base capability version 1.
pub const VFIO_DEVICE_INFO_CAP_ZPCI_BASE_VERSION: u16 = 1;
/// Group capability version 1.
pub const VFIO_DEVICE_INFO_CAP_ZPCI_GROUP_VERSION: u16 = 1;
/// Group capability version 2 (with reserved/maxstbl).
pub const VFIO_DEVICE_INFO_CAP_ZPCI_GROUP_VERSION_2: u16 = 2;

// ---------------------------------------------------------------------------
// zPCI utility-string and PFIP sizes
// ---------------------------------------------------------------------------

/// Length of the zPCI utility string in bytes.
pub const CLP_UTIL_STR_LEN: u32 = 64;
/// Length of the zPCI PFIP array in bytes.
pub const CLP_PFIP_NR_SEGMENTS: u32 = 4;

// ---------------------------------------------------------------------------
// zPCI base-capability flag bits
// ---------------------------------------------------------------------------

/// MIO (mapped I/O) is supported.
pub const VFIO_ZPCI_FLAG_MIO_SUPPORTED: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_ids_distinct() {
        let ids = [
            VFIO_DEVICE_INFO_CAP_ZPCI_BASE,
            VFIO_DEVICE_INFO_CAP_ZPCI_GROUP,
            VFIO_DEVICE_INFO_CAP_ZPCI_UTIL,
            VFIO_DEVICE_INFO_CAP_ZPCI_PFIP,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_versions_monotonic() {
        // Version-2 must succeed version-1 to satisfy struct-layout
        // forward-compatibility (newer fields appended).
        assert_eq!(VFIO_DEVICE_INFO_CAP_ZPCI_BASE_VERSION, 1);
        assert!(
            VFIO_DEVICE_INFO_CAP_ZPCI_GROUP_VERSION_2
                > VFIO_DEVICE_INFO_CAP_ZPCI_GROUP_VERSION
        );
    }

    #[test]
    fn test_clp_sizes_sane() {
        assert_eq!(CLP_UTIL_STR_LEN, 64);
        assert!(CLP_PFIP_NR_SEGMENTS >= 1);
    }

    #[test]
    fn test_flag_bit_single() {
        assert!(VFIO_ZPCI_FLAG_MIO_SUPPORTED.is_power_of_two());
    }
}
