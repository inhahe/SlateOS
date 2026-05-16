//! `<linux/virtio_pci.h>` — Virtio PCI transport constants.
//!
//! The virtio PCI transport is the standard way to expose virtio
//! devices in QEMU/KVM virtual machines. It uses PCI capabilities
//! to locate configuration, notification, and ISR registers.

// ---------------------------------------------------------------------------
// PCI vendor/device
// ---------------------------------------------------------------------------

/// Virtio PCI vendor ID.
pub const VIRTIO_PCI_VENDOR_ID: u16 = 0x1AF4;
/// Legacy device ID range start (add virtio device ID).
pub const VIRTIO_PCI_DEVICE_ID_LEGACY_BASE: u16 = 0x1000;
/// Modern device ID range start (add virtio device ID + 0x1040).
pub const VIRTIO_PCI_DEVICE_ID_MODERN_BASE: u16 = 0x1040;

// ---------------------------------------------------------------------------
// PCI capability types
// ---------------------------------------------------------------------------

/// Common configuration capability.
pub const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
/// Notification capability.
pub const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
/// ISR status capability.
pub const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
/// Device-specific configuration capability.
pub const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
/// PCI config access capability.
pub const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;
/// Shared memory region capability.
pub const VIRTIO_PCI_CAP_SHARED_MEMORY_CFG: u8 = 8;

// ---------------------------------------------------------------------------
// Legacy register offsets
// ---------------------------------------------------------------------------

/// Host features (read).
pub const VIRTIO_PCI_HOST_FEATURES: u32 = 0;
/// Guest features (write).
pub const VIRTIO_PCI_GUEST_FEATURES: u32 = 4;
/// Queue address.
pub const VIRTIO_PCI_QUEUE_PFN: u32 = 8;
/// Queue size.
pub const VIRTIO_PCI_QUEUE_NUM: u32 = 12;
/// Queue selector.
pub const VIRTIO_PCI_QUEUE_SEL: u32 = 14;
/// Queue notify.
pub const VIRTIO_PCI_QUEUE_NOTIFY: u32 = 16;
/// Device status.
pub const VIRTIO_PCI_STATUS: u32 = 18;
/// ISR status.
pub const VIRTIO_PCI_ISR: u32 = 19;

// ---------------------------------------------------------------------------
// ISR bits
// ---------------------------------------------------------------------------

/// Virtqueue interrupt.
pub const VIRTIO_PCI_ISR_QUEUE: u8 = 1;
/// Configuration change interrupt.
pub const VIRTIO_PCI_ISR_CONFIG: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vendor_id() {
        assert_eq!(VIRTIO_PCI_VENDOR_ID, 0x1AF4);
    }

    #[test]
    fn test_device_id_ranges() {
        assert!(VIRTIO_PCI_DEVICE_ID_MODERN_BASE > VIRTIO_PCI_DEVICE_ID_LEGACY_BASE);
    }

    #[test]
    fn test_cap_types_distinct() {
        let caps = [
            VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_NOTIFY_CFG,
            VIRTIO_PCI_CAP_ISR_CFG, VIRTIO_PCI_CAP_DEVICE_CFG,
            VIRTIO_PCI_CAP_PCI_CFG, VIRTIO_PCI_CAP_SHARED_MEMORY_CFG,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_legacy_offsets_distinct() {
        let offsets = [
            VIRTIO_PCI_HOST_FEATURES, VIRTIO_PCI_GUEST_FEATURES,
            VIRTIO_PCI_QUEUE_PFN, VIRTIO_PCI_QUEUE_NUM,
            VIRTIO_PCI_QUEUE_SEL, VIRTIO_PCI_QUEUE_NOTIFY,
            VIRTIO_PCI_STATUS, VIRTIO_PCI_ISR,
        ];
        for i in 0..offsets.len() {
            for j in (i + 1)..offsets.len() {
                assert_ne!(offsets[i], offsets[j]);
            }
        }
    }

    #[test]
    fn test_isr_bits_distinct() {
        assert_ne!(VIRTIO_PCI_ISR_QUEUE, VIRTIO_PCI_ISR_CONFIG);
        assert_eq!(VIRTIO_PCI_ISR_QUEUE & VIRTIO_PCI_ISR_CONFIG, 0);
    }
}
