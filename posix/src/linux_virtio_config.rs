//! `<linux/virtio_config.h>` — Virtio configuration space constants.
//!
//! Virtio defines a common configuration interface for virtual
//! devices. The config space contains device status, feature bits,
//! and device-specific configuration. This module covers the
//! transport-independent parts of the virtio spec.

// ---------------------------------------------------------------------------
// Device status bits
// ---------------------------------------------------------------------------

/// Driver has acknowledged the device.
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: u8 = 1;
/// Driver knows how to drive the device.
pub const VIRTIO_CONFIG_S_DRIVER: u8 = 2;
/// Feature negotiation complete.
pub const VIRTIO_CONFIG_S_FEATURES_OK: u8 = 8;
/// Driver is ready.
pub const VIRTIO_CONFIG_S_DRIVER_OK: u8 = 4;
/// Device has experienced an error.
pub const VIRTIO_CONFIG_S_NEEDS_RESET: u8 = 0x40;
/// Device has failed.
pub const VIRTIO_CONFIG_S_FAILED: u8 = 0x80;

// ---------------------------------------------------------------------------
// Common feature bits (transport-independent)
// ---------------------------------------------------------------------------

/// Device supports indirect descriptors.
pub const VIRTIO_F_INDIRECT_DESC: u32 = 28;
/// Device supports used buffer notification suppression.
pub const VIRTIO_F_EVENT_IDX: u32 = 29;
/// Virtio 1.0+ (non-legacy).
pub const VIRTIO_F_VERSION_1: u32 = 32;
/// Device supports access platform (IOMMU).
pub const VIRTIO_F_ACCESS_PLATFORM: u32 = 33;
/// Device supports packed virtqueue layout.
pub const VIRTIO_F_RING_PACKED: u32 = 34;
/// Device supports in-order completion.
pub const VIRTIO_F_IN_ORDER: u32 = 35;
/// Device supports order platform operations.
pub const VIRTIO_F_ORDER_PLATFORM: u32 = 36;
/// Device supports single-root I/O virtualization.
pub const VIRTIO_F_SR_IOV: u32 = 37;
/// Device supports notification data.
pub const VIRTIO_F_NOTIFICATION_DATA: u32 = 38;

// ---------------------------------------------------------------------------
// Device IDs
// ---------------------------------------------------------------------------

/// Network device.
pub const VIRTIO_ID_NET: u32 = 1;
/// Block device.
pub const VIRTIO_ID_BLOCK: u32 = 2;
/// Console device.
pub const VIRTIO_ID_CONSOLE: u32 = 3;
/// Entropy source.
pub const VIRTIO_ID_RNG: u32 = 4;
/// Memory balloon.
pub const VIRTIO_ID_BALLOON: u32 = 5;
/// SCSI host.
pub const VIRTIO_ID_SCSI: u32 = 8;
/// 9P filesystem.
pub const VIRTIO_ID_9P: u32 = 9;
/// GPU device.
pub const VIRTIO_ID_GPU: u32 = 16;
/// Input device.
pub const VIRTIO_ID_INPUT: u32 = 18;
/// Vsock device.
pub const VIRTIO_ID_VSOCK: u32 = 19;
/// Crypto device.
pub const VIRTIO_ID_CRYPTO: u32 = 20;
/// Filesystem device (virtio-fs).
pub const VIRTIO_ID_FS: u32 = 26;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bits_no_zero_overlap() {
        let bits = [
            VIRTIO_CONFIG_S_ACKNOWLEDGE, VIRTIO_CONFIG_S_DRIVER,
            VIRTIO_CONFIG_S_FEATURES_OK, VIRTIO_CONFIG_S_DRIVER_OK,
            VIRTIO_CONFIG_S_NEEDS_RESET, VIRTIO_CONFIG_S_FAILED,
        ];
        for i in 0..bits.len() {
            assert_ne!(bits[i], 0);
        }
    }

    #[test]
    fn test_feature_bits_distinct() {
        let features = [
            VIRTIO_F_INDIRECT_DESC, VIRTIO_F_EVENT_IDX,
            VIRTIO_F_VERSION_1, VIRTIO_F_ACCESS_PLATFORM,
            VIRTIO_F_RING_PACKED, VIRTIO_F_IN_ORDER,
            VIRTIO_F_ORDER_PLATFORM, VIRTIO_F_SR_IOV,
            VIRTIO_F_NOTIFICATION_DATA,
        ];
        for i in 0..features.len() {
            for j in (i + 1)..features.len() {
                assert_ne!(features[i], features[j]);
            }
        }
    }

    #[test]
    fn test_device_ids_distinct() {
        let ids = [
            VIRTIO_ID_NET, VIRTIO_ID_BLOCK, VIRTIO_ID_CONSOLE,
            VIRTIO_ID_RNG, VIRTIO_ID_BALLOON, VIRTIO_ID_SCSI,
            VIRTIO_ID_9P, VIRTIO_ID_GPU, VIRTIO_ID_INPUT,
            VIRTIO_ID_VSOCK, VIRTIO_ID_CRYPTO, VIRTIO_ID_FS,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_version_1_feature() {
        assert_eq!(VIRTIO_F_VERSION_1, 32);
    }
}
