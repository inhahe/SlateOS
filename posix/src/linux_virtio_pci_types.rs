//! `<linux/virtio_pci.h>` — VirtIO PCI transport constants.
//!
//! VirtIO devices on PCI use specific capability structures and
//! register offsets for device discovery, configuration, and
//! virtqueue management. The modern (1.0+) PCI layout uses PCI
//! capabilities to locate configuration regions.

// ---------------------------------------------------------------------------
// VirtIO PCI capability types (virtio_pci_cap.cfg_type)
// ---------------------------------------------------------------------------

/// Common configuration space.
pub const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
/// Notification area.
pub const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
/// ISR status.
pub const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
/// Device-specific configuration.
pub const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
/// PCI configuration access.
pub const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;
/// Shared memory region.
pub const VIRTIO_PCI_CAP_SHARED_MEMORY_CFG: u8 = 8;

// ---------------------------------------------------------------------------
// VirtIO PCI ISR bits
// ---------------------------------------------------------------------------

/// Queue interrupt (at least one used buffer ready).
pub const VIRTIO_PCI_ISR_QUEUE: u32 = 0x01;
/// Configuration change interrupt.
pub const VIRTIO_PCI_ISR_CONFIG: u32 = 0x02;

// ---------------------------------------------------------------------------
// VirtIO PCI device status bits
// ---------------------------------------------------------------------------

/// Guest OS has acknowledged the device.
pub const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
/// Guest OS driver knows how to drive the device.
pub const VIRTIO_STATUS_DRIVER: u8 = 2;
/// Driver is ready (virtqueues set up).
pub const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
/// Feature negotiation complete.
pub const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
/// Device needs reset (unrecoverable error).
pub const VIRTIO_STATUS_DEVICE_NEEDS_RESET: u8 = 64;
/// Driver has given up on the device.
pub const VIRTIO_STATUS_FAILED: u8 = 128;

// ---------------------------------------------------------------------------
// VirtIO PCI vendor/device IDs
// ---------------------------------------------------------------------------

/// VirtIO PCI vendor ID (Red Hat/QEMU).
pub const VIRTIO_PCI_VENDOR_ID: u16 = 0x1AF4;
/// Transitional device ID range start.
pub const VIRTIO_PCI_DEVICE_ID_TRANS_START: u16 = 0x1000;
/// Transitional device ID range end.
pub const VIRTIO_PCI_DEVICE_ID_TRANS_END: u16 = 0x103F;
/// Modern device ID range start (1.0+).
pub const VIRTIO_PCI_DEVICE_ID_MODERN_START: u16 = 0x1040;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_types_distinct() {
        let caps = [
            VIRTIO_PCI_CAP_COMMON_CFG,
            VIRTIO_PCI_CAP_NOTIFY_CFG,
            VIRTIO_PCI_CAP_ISR_CFG,
            VIRTIO_PCI_CAP_DEVICE_CFG,
            VIRTIO_PCI_CAP_PCI_CFG,
            VIRTIO_PCI_CAP_SHARED_MEMORY_CFG,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_isr_bits_no_overlap() {
        assert!(VIRTIO_PCI_ISR_QUEUE.is_power_of_two());
        assert!(VIRTIO_PCI_ISR_CONFIG.is_power_of_two());
        assert_eq!(VIRTIO_PCI_ISR_QUEUE & VIRTIO_PCI_ISR_CONFIG, 0);
    }

    #[test]
    fn test_status_progression() {
        // Normal init: acknowledge → driver → features_ok → driver_ok
        assert!(VIRTIO_STATUS_ACKNOWLEDGE < VIRTIO_STATUS_DRIVER);
        assert!(VIRTIO_STATUS_DRIVER < VIRTIO_STATUS_DRIVER_OK);
        assert!(VIRTIO_STATUS_DRIVER_OK < VIRTIO_STATUS_FEATURES_OK);
    }

    #[test]
    fn test_device_id_ranges() {
        assert!(VIRTIO_PCI_DEVICE_ID_TRANS_START < VIRTIO_PCI_DEVICE_ID_TRANS_END);
        assert!(VIRTIO_PCI_DEVICE_ID_TRANS_END < VIRTIO_PCI_DEVICE_ID_MODERN_START);
    }

    #[test]
    fn test_vendor_id() {
        assert_eq!(VIRTIO_PCI_VENDOR_ID, 0x1AF4);
    }
}
