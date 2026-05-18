//! `<linux/virtio_ids.h>` — VirtIO device type identifiers.
//!
//! Each VirtIO device is identified by a device ID that tells the
//! driver what kind of device it is (network, block, console, etc.).
//! These IDs are standardised by the OASIS VirtIO specification and
//! used during device discovery and driver binding.

// ---------------------------------------------------------------------------
// VirtIO device IDs
// ---------------------------------------------------------------------------

/// Network device (virtio-net).
pub const VIRTIO_ID_NET: u32 = 1;
/// Block device (virtio-blk).
pub const VIRTIO_ID_BLOCK: u32 = 2;
/// Console device (virtio-console).
pub const VIRTIO_ID_CONSOLE: u32 = 3;
/// Entropy source (virtio-rng).
pub const VIRTIO_ID_RNG: u32 = 4;
/// Memory balloon (virtio-balloon).
pub const VIRTIO_ID_BALLOON: u32 = 5;
/// SCSI host adapter (virtio-scsi).
pub const VIRTIO_ID_SCSI: u32 = 8;
/// 9P filesystem (virtio-9p).
pub const VIRTIO_ID_9P: u32 = 9;
/// GPU device (virtio-gpu).
pub const VIRTIO_ID_GPU: u32 = 16;
/// Input device (virtio-input).
pub const VIRTIO_ID_INPUT: u32 = 18;
/// Vsock device (virtio-vsock).
pub const VIRTIO_ID_VSOCK: u32 = 19;
/// Crypto device (virtio-crypto).
pub const VIRTIO_ID_CRYPTO: u32 = 20;
/// IOMMU device (virtio-iommu).
pub const VIRTIO_ID_IOMMU: u32 = 23;
/// Memory device (virtio-mem).
pub const VIRTIO_ID_MEM: u32 = 24;
/// Sound device (virtio-snd).
pub const VIRTIO_ID_SOUND: u32 = 25;
/// Filesystem device (virtio-fs).
pub const VIRTIO_ID_FS: u32 = 26;
/// PMEM device (virtio-pmem).
pub const VIRTIO_ID_PMEM: u32 = 27;
/// Bluetooth device (virtio-bt).
pub const VIRTIO_ID_BT: u32 = 40;
/// GPIO device (virtio-gpio).
pub const VIRTIO_ID_GPIO: u32 = 41;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_ids_distinct() {
        let ids = [
            VIRTIO_ID_NET, VIRTIO_ID_BLOCK, VIRTIO_ID_CONSOLE,
            VIRTIO_ID_RNG, VIRTIO_ID_BALLOON, VIRTIO_ID_SCSI,
            VIRTIO_ID_9P, VIRTIO_ID_GPU, VIRTIO_ID_INPUT,
            VIRTIO_ID_VSOCK, VIRTIO_ID_CRYPTO, VIRTIO_ID_IOMMU,
            VIRTIO_ID_MEM, VIRTIO_ID_SOUND, VIRTIO_ID_FS,
            VIRTIO_ID_PMEM, VIRTIO_ID_BT, VIRTIO_ID_GPIO,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_common_ids() {
        assert_eq!(VIRTIO_ID_NET, 1);
        assert_eq!(VIRTIO_ID_BLOCK, 2);
        assert_eq!(VIRTIO_ID_GPU, 16);
    }

    #[test]
    fn test_all_nonzero() {
        let ids = [
            VIRTIO_ID_NET, VIRTIO_ID_BLOCK, VIRTIO_ID_CONSOLE,
            VIRTIO_ID_RNG, VIRTIO_ID_FS,
        ];
        for &id in &ids {
            assert_ne!(id, 0);
        }
    }
}
