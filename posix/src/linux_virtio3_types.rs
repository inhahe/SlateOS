//! `<linux/virtio_config.h>` — Additional virtio configuration constants.
//!
//! Supplementary virtio constants covering device status bits,
//! transport feature bits, and configuration change notification.

// ---------------------------------------------------------------------------
// Virtio device status bits
// ---------------------------------------------------------------------------

/// Device acknowledged.
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: u8 = 1;
/// Guest OS driver loaded.
pub const VIRTIO_CONFIG_S_DRIVER: u8 = 2;
/// Driver ready — feature negotiation complete.
pub const VIRTIO_CONFIG_S_DRIVER_OK: u8 = 4;
/// Feature negotiation complete.
pub const VIRTIO_CONFIG_S_FEATURES_OK: u8 = 8;
/// Device needs reset (error).
pub const VIRTIO_CONFIG_S_NEEDS_RESET: u8 = 0x40;
/// Device has failed.
pub const VIRTIO_CONFIG_S_FAILED: u8 = 0x80;

// ---------------------------------------------------------------------------
// Virtio transport feature bits (bits 28-34)
// ---------------------------------------------------------------------------

/// Indirect descriptor table.
pub const VIRTIO_RING_F_INDIRECT_DESC: u32 = 28;
/// Used buffer event index.
pub const VIRTIO_RING_F_EVENT_IDX: u32 = 29;
/// Version 1 of the virtio spec.
pub const VIRTIO_F_VERSION_1: u32 = 32;
/// Access platform — IOMMU support.
pub const VIRTIO_F_ACCESS_PLATFORM: u32 = 33;
/// Ring packed layout.
pub const VIRTIO_F_RING_PACKED: u32 = 34;
/// In-order completions.
pub const VIRTIO_F_IN_ORDER: u32 = 35;
/// Order-preserving platform.
pub const VIRTIO_F_ORDER_PLATFORM: u32 = 36;
/// Single-root I/O virtualization.
pub const VIRTIO_F_SR_IOV: u32 = 37;
/// Notification data.
pub const VIRTIO_F_NOTIFICATION_DATA: u32 = 38;
/// Admin virtqueue.
pub const VIRTIO_F_ADMIN_VQ: u32 = 41;

// ---------------------------------------------------------------------------
// Virtio vring descriptor flags
// ---------------------------------------------------------------------------

/// Descriptor continues via next field.
pub const VRING_DESC_F_NEXT: u16 = 1;
/// Buffer is device-writable (read by device).
pub const VRING_DESC_F_WRITE: u16 = 2;
/// Buffer contains a list of buffer descriptors.
pub const VRING_DESC_F_INDIRECT: u16 = 4;

// ---------------------------------------------------------------------------
// Virtio vring available/used ring flags
// ---------------------------------------------------------------------------

/// No interrupt when buffer consumed.
pub const VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;
/// No notification when buffer added.
pub const VRING_USED_F_NO_NOTIFY: u16 = 1;

// ---------------------------------------------------------------------------
// Virtio device types
// ---------------------------------------------------------------------------

/// Network card.
pub const VIRTIO_ID_NET: u32 = 1;
/// Block device.
pub const VIRTIO_ID_BLOCK: u32 = 2;
/// Console.
pub const VIRTIO_ID_CONSOLE: u32 = 3;
/// Entropy source.
pub const VIRTIO_ID_RNG: u32 = 4;
/// Memory ballooning.
pub const VIRTIO_ID_BALLOON: u32 = 5;
/// SCSI host.
pub const VIRTIO_ID_SCSI: u32 = 8;
/// GPU device.
pub const VIRTIO_ID_GPU: u32 = 16;
/// Input device.
pub const VIRTIO_ID_INPUT: u32 = 18;
/// Vsock device.
pub const VIRTIO_ID_VSOCK: u32 = 19;
/// Crypto device.
pub const VIRTIO_ID_CRYPTO: u32 = 20;
/// Filesystem device.
pub const VIRTIO_ID_FS: u32 = 26;
/// PMEM device.
pub const VIRTIO_ID_PMEM: u32 = 27;
/// MAC80211 hwsim device.
pub const VIRTIO_ID_MAC80211_HWSIM: u32 = 29;
/// BT device.
pub const VIRTIO_ID_BT: u32 = 40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bits_no_overlap() {
        let bits: [u8; 6] = [
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
    fn test_transport_features_distinct() {
        let feats = [
            VIRTIO_RING_F_INDIRECT_DESC, VIRTIO_RING_F_EVENT_IDX,
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

    #[test]
    fn test_desc_flags_power_of_two() {
        assert!(VRING_DESC_F_NEXT.is_power_of_two());
        assert!(VRING_DESC_F_WRITE.is_power_of_two());
        assert!(VRING_DESC_F_INDIRECT.is_power_of_two());
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
    fn test_device_ids_distinct() {
        let ids = [
            VIRTIO_ID_NET, VIRTIO_ID_BLOCK, VIRTIO_ID_CONSOLE,
            VIRTIO_ID_RNG, VIRTIO_ID_BALLOON, VIRTIO_ID_SCSI,
            VIRTIO_ID_GPU, VIRTIO_ID_INPUT, VIRTIO_ID_VSOCK,
            VIRTIO_ID_CRYPTO, VIRTIO_ID_FS, VIRTIO_ID_PMEM,
            VIRTIO_ID_MAC80211_HWSIM, VIRTIO_ID_BT,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }
}
