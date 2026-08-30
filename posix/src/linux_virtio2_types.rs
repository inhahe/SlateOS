//! `<linux/virtio_config.h>` — Virtio device constants (extended).
//!
//! Extended virtio constants covering device status bits,
//! feature bits, virtqueue flags, and common configuration
//! structure offsets.

// ---------------------------------------------------------------------------
// Virtio device status bits
// ---------------------------------------------------------------------------

/// Reset (initial state).
pub const VIRTIO_STATUS_RESET: u8 = 0;
/// Guest OS acknowledges device.
pub const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
/// Guest OS knows how to drive device.
pub const VIRTIO_STATUS_DRIVER: u8 = 2;
/// Driver is ready.
pub const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
/// Feature negotiation complete.
pub const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
/// Device needs reset (error).
pub const VIRTIO_STATUS_DEVICE_NEEDS_RESET: u8 = 64;
/// Device failed (unrecoverable).
pub const VIRTIO_STATUS_FAILED: u8 = 128;

// ---------------------------------------------------------------------------
// Virtio transport feature bits (common)
// ---------------------------------------------------------------------------

/// Indirect descriptors.
pub const VIRTIO_F_INDIRECT_DESC: u32 = 28;
/// Event index.
pub const VIRTIO_F_EVENT_IDX: u32 = 29;
/// Virtio version 1.
pub const VIRTIO_F_VERSION_1: u32 = 32;
/// Access platform (IOMMU).
pub const VIRTIO_F_ACCESS_PLATFORM: u32 = 33;
/// Ring packed layout.
pub const VIRTIO_F_RING_PACKED: u32 = 34;
/// In-order completion.
pub const VIRTIO_F_IN_ORDER: u32 = 35;
/// Order platform (memory ordering).
pub const VIRTIO_F_ORDER_PLATFORM: u32 = 36;
/// SR-IOV support.
pub const VIRTIO_F_SR_IOV: u32 = 37;
/// Notification data.
pub const VIRTIO_F_NOTIFICATION_DATA: u32 = 38;

// ---------------------------------------------------------------------------
// Virtio device types
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
/// 9P transport.
pub const VIRTIO_ID_9P: u32 = 9;
/// Input device.
pub const VIRTIO_ID_INPUT: u32 = 18;
/// Vsock transport.
pub const VIRTIO_ID_VSOCK: u32 = 19;
/// Crypto device.
pub const VIRTIO_ID_CRYPTO: u32 = 20;
/// GPU device.
pub const VIRTIO_ID_GPU: u32 = 16;
/// Filesystem.
pub const VIRTIO_ID_FS: u32 = 26;
/// PMEM device.
pub const VIRTIO_ID_PMEM: u32 = 27;

// ---------------------------------------------------------------------------
// Virtqueue descriptor flags
// ---------------------------------------------------------------------------

/// Buffer continues via next field.
pub const VRING_DESC_F_NEXT: u16 = 1;
/// Buffer is device-writable (for read by device).
pub const VRING_DESC_F_WRITE: u16 = 2;
/// Buffer contains a list of indirect descriptors.
pub const VRING_DESC_F_INDIRECT: u16 = 4;

// ---------------------------------------------------------------------------
// Virtqueue available/used ring flags
// ---------------------------------------------------------------------------

/// No interrupt when buffer consumed.
pub const VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;
/// No notify when buffer added.
pub const VRING_USED_F_NO_NOTIFY: u16 = 1;

// ---------------------------------------------------------------------------
// Virtqueue sizes
// ---------------------------------------------------------------------------

/// Default virtqueue size.
pub const VIRTIO_QUEUE_SIZE_DEFAULT: u16 = 256;
/// Maximum virtqueue size.
pub const VIRTIO_QUEUE_SIZE_MAX: u16 = 32768;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bits_distinct() {
        let bits = [
            VIRTIO_STATUS_ACKNOWLEDGE,
            VIRTIO_STATUS_DRIVER,
            VIRTIO_STATUS_DRIVER_OK,
            VIRTIO_STATUS_FEATURES_OK,
            VIRTIO_STATUS_DEVICE_NEEDS_RESET,
            VIRTIO_STATUS_FAILED,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_reset_is_zero() {
        assert_eq!(VIRTIO_STATUS_RESET, 0);
    }

    #[test]
    fn test_transport_features_distinct() {
        let feats = [
            VIRTIO_F_INDIRECT_DESC,
            VIRTIO_F_EVENT_IDX,
            VIRTIO_F_VERSION_1,
            VIRTIO_F_ACCESS_PLATFORM,
            VIRTIO_F_RING_PACKED,
            VIRTIO_F_IN_ORDER,
            VIRTIO_F_ORDER_PLATFORM,
            VIRTIO_F_SR_IOV,
            VIRTIO_F_NOTIFICATION_DATA,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_device_types_distinct() {
        let types = [
            VIRTIO_ID_NET,
            VIRTIO_ID_BLOCK,
            VIRTIO_ID_CONSOLE,
            VIRTIO_ID_RNG,
            VIRTIO_ID_BALLOON,
            VIRTIO_ID_SCSI,
            VIRTIO_ID_9P,
            VIRTIO_ID_INPUT,
            VIRTIO_ID_VSOCK,
            VIRTIO_ID_CRYPTO,
            VIRTIO_ID_GPU,
            VIRTIO_ID_FS,
            VIRTIO_ID_PMEM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_desc_flags_powers_of_two() {
        let flags = [VRING_DESC_F_NEXT, VRING_DESC_F_WRITE, VRING_DESC_F_INDIRECT];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_desc_flags_no_overlap() {
        let flags = [VRING_DESC_F_NEXT, VRING_DESC_F_WRITE, VRING_DESC_F_INDIRECT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_queue_size_defaults() {
        assert_eq!(VIRTIO_QUEUE_SIZE_DEFAULT, 256);
        assert!(VIRTIO_QUEUE_SIZE_MAX > VIRTIO_QUEUE_SIZE_DEFAULT);
    }

    #[test]
    fn test_net_is_one() {
        assert_eq!(VIRTIO_ID_NET, 1);
    }
}
