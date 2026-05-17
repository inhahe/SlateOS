//! `<linux/virtio_config.h>` — VirtIO device configuration constants.
//!
//! VirtIO is the standard paravirtualized I/O framework for virtual
//! machines. The config space defines how guest drivers negotiate
//! features, configure devices, and communicate status. Each device
//! advertises features; the guest acknowledges what it supports.
//! The status register tracks the driver/device negotiation state
//! machine. Used by QEMU/KVM, cloud hypervisors, and embedded
//! virtualization (e.g., Xen, Firecracker).

// ---------------------------------------------------------------------------
// VirtIO status bits (VIRTIO_CONFIG_S_*)
// ---------------------------------------------------------------------------

/// Guest OS has acknowledged the device.
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: u32 = 1;
/// Guest OS driver has been found for the device.
pub const VIRTIO_CONFIG_S_DRIVER: u32 = 2;
/// Driver is ready to drive the device.
pub const VIRTIO_CONFIG_S_DRIVER_OK: u32 = 4;
/// Feature negotiation is complete.
pub const VIRTIO_CONFIG_S_FEATURES_OK: u32 = 8;
/// Device has experienced an error (needs reset).
pub const VIRTIO_CONFIG_S_NEEDS_RESET: u32 = 64;
/// Driver has given up on the device.
pub const VIRTIO_CONFIG_S_FAILED: u32 = 128;

// ---------------------------------------------------------------------------
// VirtIO transport feature bits (VIRTIO_F_*)
// ---------------------------------------------------------------------------

/// Device supports indirect descriptors.
pub const VIRTIO_F_INDIRECT_DESC: u32 = 28;
/// Device supports event index (avail/used event suppression).
pub const VIRTIO_F_EVENT_IDX: u32 = 29;
/// Device supports VirtIO 1.0+ (modern).
pub const VIRTIO_F_VERSION_1: u32 = 32;
/// Device supports access platform (IOMMU).
pub const VIRTIO_F_ACCESS_PLATFORM: u32 = 33;
/// Device supports packed virtqueue layout.
pub const VIRTIO_F_RING_PACKED: u32 = 34;
/// Device supports in-order descriptor use.
pub const VIRTIO_F_IN_ORDER: u32 = 35;
/// Device supports order platform operations.
pub const VIRTIO_F_ORDER_PLATFORM: u32 = 36;
/// Device supports single-root I/O virtualization.
pub const VIRTIO_F_SR_IOV: u32 = 37;
/// Device supports notification data.
pub const VIRTIO_F_NOTIFICATION_DATA: u32 = 38;
/// Device supports admin virtqueue.
pub const VIRTIO_F_ADMIN_VQ: u32 = 41;

// ---------------------------------------------------------------------------
// VirtIO device IDs
// ---------------------------------------------------------------------------

/// Network device.
pub const VIRTIO_ID_NET: u32 = 1;
/// Block device.
pub const VIRTIO_ID_BLOCK: u32 = 2;
/// Console device.
pub const VIRTIO_ID_CONSOLE: u32 = 3;
/// Entropy source (RNG).
pub const VIRTIO_ID_RNG: u32 = 4;
/// Memory balloon.
pub const VIRTIO_ID_BALLOON: u32 = 5;
/// SCSI host.
pub const VIRTIO_ID_SCSI: u32 = 8;
/// 9P transport (Plan 9 file sharing).
pub const VIRTIO_ID_9P: u32 = 9;
/// GPU device.
pub const VIRTIO_ID_GPU: u32 = 16;
/// Input device.
pub const VIRTIO_ID_INPUT: u32 = 18;
/// vsock transport.
pub const VIRTIO_ID_VSOCK: u32 = 19;
/// Crypto device.
pub const VIRTIO_ID_CRYPTO: u32 = 20;
/// Sound device.
pub const VIRTIO_ID_SOUND: u32 = 25;
/// Filesystem (virtio-fs).
pub const VIRTIO_ID_FS: u32 = 26;
/// PMEM (persistent memory).
pub const VIRTIO_ID_PMEM: u32 = 27;
/// Bluetooth device.
pub const VIRTIO_ID_BT: u32 = 40;
/// GPIO device.
pub const VIRTIO_ID_GPIO: u32 = 41;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bits_no_overlap() {
        let bits = [
            VIRTIO_CONFIG_S_ACKNOWLEDGE, VIRTIO_CONFIG_S_DRIVER,
            VIRTIO_CONFIG_S_DRIVER_OK, VIRTIO_CONFIG_S_FEATURES_OK,
            VIRTIO_CONFIG_S_NEEDS_RESET, VIRTIO_CONFIG_S_FAILED,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_device_ids_distinct() {
        let ids = [
            VIRTIO_ID_NET, VIRTIO_ID_BLOCK, VIRTIO_ID_CONSOLE,
            VIRTIO_ID_RNG, VIRTIO_ID_BALLOON, VIRTIO_ID_SCSI,
            VIRTIO_ID_9P, VIRTIO_ID_GPU, VIRTIO_ID_INPUT,
            VIRTIO_ID_VSOCK, VIRTIO_ID_CRYPTO, VIRTIO_ID_SOUND,
            VIRTIO_ID_FS, VIRTIO_ID_PMEM, VIRTIO_ID_BT, VIRTIO_ID_GPIO,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_transport_features_distinct() {
        let feats = [
            VIRTIO_F_INDIRECT_DESC, VIRTIO_F_EVENT_IDX,
            VIRTIO_F_VERSION_1, VIRTIO_F_ACCESS_PLATFORM,
            VIRTIO_F_RING_PACKED, VIRTIO_F_IN_ORDER,
            VIRTIO_F_ORDER_PLATFORM, VIRTIO_F_SR_IOV,
            VIRTIO_F_NOTIFICATION_DATA, VIRTIO_F_ADMIN_VQ,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }
}
