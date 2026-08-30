//! `<linux/virtio_types.h>` — virtio base types and device IDs.
//!
//! Virtio is the standard paravirtualized I/O framework for virtual
//! machines. These types and device IDs are used by all virtio device
//! drivers (virtio-net, virtio-blk, virtio-gpu, etc.).

// ---------------------------------------------------------------------------
// Virtio device IDs (from virtio_ids.h)
// ---------------------------------------------------------------------------

/// Network device.
pub const VIRTIO_ID_NET: u32 = 1;
/// Block device.
pub const VIRTIO_ID_BLOCK: u32 = 2;
/// Console.
pub const VIRTIO_ID_CONSOLE: u32 = 3;
/// Entropy source (RNG).
pub const VIRTIO_ID_RNG: u32 = 4;
/// Memory balloon.
pub const VIRTIO_ID_BALLOON: u32 = 5;
/// SCSI host.
pub const VIRTIO_ID_SCSI: u32 = 8;
/// 9p transport.
pub const VIRTIO_ID_9P: u32 = 9;
/// GPU device.
pub const VIRTIO_ID_GPU: u32 = 16;
/// Input device.
pub const VIRTIO_ID_INPUT: u32 = 18;
/// Socket device (vsock).
pub const VIRTIO_ID_VSOCK: u32 = 19;
/// Crypto device.
pub const VIRTIO_ID_CRYPTO: u32 = 20;
/// Sound device.
pub const VIRTIO_ID_SOUND: u32 = 25;
/// Filesystem device (virtiofs).
pub const VIRTIO_ID_FS: u32 = 26;
/// PMEM device.
pub const VIRTIO_ID_PMEM: u32 = 27;
/// Bluetooth device.
pub const VIRTIO_ID_BT: u32 = 28;
/// GPIO device.
pub const VIRTIO_ID_GPIO: u32 = 29;

// ---------------------------------------------------------------------------
// Virtio device status bits
// ---------------------------------------------------------------------------

/// Guest OS has found the device.
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: u8 = 1;
/// Guest OS knows how to drive the device.
pub const VIRTIO_CONFIG_S_DRIVER: u8 = 2;
/// Driver is set up and ready.
pub const VIRTIO_CONFIG_S_DRIVER_OK: u8 = 4;
/// Driver has acknowledged all features.
pub const VIRTIO_CONFIG_S_FEATURES_OK: u8 = 8;
/// Device has experienced an error.
pub const VIRTIO_CONFIG_S_NEEDS_RESET: u8 = 0x40;
/// Something went wrong (device or driver).
pub const VIRTIO_CONFIG_S_FAILED: u8 = 0x80;

// ---------------------------------------------------------------------------
// Virtio feature bits (transport-level)
// ---------------------------------------------------------------------------

/// Device supports indirect descriptors.
pub const VIRTIO_F_INDIRECT_DESC: u32 = 28;
/// Device supports used-buffer notifications.
pub const VIRTIO_F_EVENT_IDX: u32 = 29;
/// Device supports version 1 (modern).
pub const VIRTIO_F_VERSION_1: u32 = 32;
/// Access platform (IOMMU) support.
pub const VIRTIO_F_ACCESS_PLATFORM: u32 = 33;
/// Ring can be packed (virtio 1.1).
pub const VIRTIO_F_RING_PACKED: u32 = 34;
/// In-order completion of requests.
pub const VIRTIO_F_IN_ORDER: u32 = 35;
/// Device supports driver notifications via order.
pub const VIRTIO_F_ORDER_PLATFORM: u32 = 36;
/// Admin queue support.
pub const VIRTIO_F_ADMIN_VQ: u32 = 41;

// ---------------------------------------------------------------------------
// Virtqueue descriptor flags
// ---------------------------------------------------------------------------

/// Buffer continues via `next` field.
pub const VRING_DESC_F_NEXT: u16 = 1;
/// Buffer is write-only (for device).
pub const VRING_DESC_F_WRITE: u16 = 2;
/// Buffer contains a list of buffer descriptors (indirect).
pub const VRING_DESC_F_INDIRECT: u16 = 4;

// ---------------------------------------------------------------------------
// Virtqueue used flags
// ---------------------------------------------------------------------------

/// Device does not need notifications.
pub const VRING_USED_F_NO_NOTIFY: u16 = 1;
/// Driver does not need interrupts.
pub const VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;

// ---------------------------------------------------------------------------
// Virtqueue descriptor
// ---------------------------------------------------------------------------

/// Virtqueue descriptor (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VringDesc {
    /// Guest physical address.
    pub addr: u64,
    /// Length.
    pub len: u32,
    /// Flags (VRING_DESC_F_*).
    pub flags: u16,
    /// Next descriptor index (if VRING_DESC_F_NEXT).
    pub next: u16,
}

impl VringDesc {
    /// Create a zeroed descriptor.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_ids_distinct() {
        let ids = [
            VIRTIO_ID_NET,
            VIRTIO_ID_BLOCK,
            VIRTIO_ID_CONSOLE,
            VIRTIO_ID_RNG,
            VIRTIO_ID_BALLOON,
            VIRTIO_ID_SCSI,
            VIRTIO_ID_9P,
            VIRTIO_ID_GPU,
            VIRTIO_ID_INPUT,
            VIRTIO_ID_VSOCK,
            VIRTIO_ID_CRYPTO,
            VIRTIO_ID_SOUND,
            VIRTIO_ID_FS,
            VIRTIO_ID_PMEM,
            VIRTIO_ID_BT,
            VIRTIO_ID_GPIO,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_status_bits() {
        assert_eq!(VIRTIO_CONFIG_S_ACKNOWLEDGE, 1);
        assert_eq!(VIRTIO_CONFIG_S_DRIVER, 2);
        assert_eq!(VIRTIO_CONFIG_S_DRIVER_OK, 4);
        assert_eq!(VIRTIO_CONFIG_S_FEATURES_OK, 8);
        assert_eq!(VIRTIO_CONFIG_S_FAILED, 0x80);
    }

    #[test]
    fn test_vring_desc_size() {
        assert_eq!(core::mem::size_of::<VringDesc>(), 16);
    }

    #[test]
    fn test_vring_desc_flags() {
        assert_eq!(VRING_DESC_F_NEXT, 1);
        assert_eq!(VRING_DESC_F_WRITE, 2);
        assert_eq!(VRING_DESC_F_INDIRECT, 4);
        // All flags should be distinct single bits.
        assert_eq!(VRING_DESC_F_NEXT & VRING_DESC_F_WRITE, 0);
        assert_eq!(VRING_DESC_F_WRITE & VRING_DESC_F_INDIRECT, 0);
    }

    #[test]
    fn test_feature_bits_distinct() {
        let feats = [
            VIRTIO_F_INDIRECT_DESC,
            VIRTIO_F_EVENT_IDX,
            VIRTIO_F_VERSION_1,
            VIRTIO_F_ACCESS_PLATFORM,
            VIRTIO_F_RING_PACKED,
            VIRTIO_F_IN_ORDER,
            VIRTIO_F_ORDER_PLATFORM,
            VIRTIO_F_ADMIN_VQ,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_vring_desc_zeroed() {
        let desc = VringDesc::zeroed();
        assert_eq!(desc.addr, 0);
        assert_eq!(desc.len, 0);
        assert_eq!(desc.flags, 0);
        assert_eq!(desc.next, 0);
    }
}
