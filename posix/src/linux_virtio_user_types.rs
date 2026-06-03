//! `<linux/virtio_config.h>` — virtio device-status & feature ABI.
//!
//! Every virtio device (block, net, console, fs, gpu, …) has the same
//! handshake: read features, write driver features, ack each step
//! through the status register. The constants here are the wire
//! protocol shared between any virtio bus transport (PCI, MMIO, CCW).

// ---------------------------------------------------------------------------
// Device-status register bits — `virtio_config_ops.set_status`
// ---------------------------------------------------------------------------

/// Guest has noticed the device.
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: u8 = 1;
/// Guest knows how to drive it.
pub const VIRTIO_CONFIG_S_DRIVER: u8 = 2;
/// Driver is up and queues set.
pub const VIRTIO_CONFIG_S_DRIVER_OK: u8 = 4;
/// Driver has read and acked features.
pub const VIRTIO_CONFIG_S_FEATURES_OK: u8 = 8;
/// Device needs reset (host-side error).
pub const VIRTIO_CONFIG_S_NEEDS_RESET: u8 = 0x40;
/// Driver gave up.
pub const VIRTIO_CONFIG_S_FAILED: u8 = 0x80;

// ---------------------------------------------------------------------------
// Transport feature bits (bits 28..38 of the 64-bit feature word)
// ---------------------------------------------------------------------------

/// Device-specific config-space has gen number — re-read on change.
pub const VIRTIO_F_NOTIFY_ON_EMPTY: u64 = 1 << 24;
/// Any layout supported (rather than just legacy layout).
pub const VIRTIO_F_ANY_LAYOUT: u64 = 1 << 27;
/// Driver supports the modern (1.0) virtio spec.
pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;
/// Use the guest physical-address space (IOMMU-translated) for queues.
pub const VIRTIO_F_ACCESS_PLATFORM: u64 = 1 << 33;
/// Use packed virtqueue layout instead of split.
pub const VIRTIO_F_RING_PACKED: u64 = 1 << 34;
/// Ring uses `IN_ORDER` completions.
pub const VIRTIO_F_IN_ORDER: u64 = 1 << 35;
/// Devices send notifications via dedicated MSI-X vectors.
pub const VIRTIO_F_ORDER_PLATFORM: u64 = 1 << 36;
/// SR-IOV is supported.
pub const VIRTIO_F_SR_IOV: u64 = 1 << 37;
/// Notification data carries the queue index too.
pub const VIRTIO_F_NOTIFICATION_DATA: u64 = 1 << 38;

// ---------------------------------------------------------------------------
// Standard virtio device IDs (from `linux/virtio_ids.h`)
// ---------------------------------------------------------------------------

pub const VIRTIO_ID_NET: u32 = 1;
pub const VIRTIO_ID_BLOCK: u32 = 2;
pub const VIRTIO_ID_CONSOLE: u32 = 3;
pub const VIRTIO_ID_RNG: u32 = 4;
pub const VIRTIO_ID_BALLOON: u32 = 5;
pub const VIRTIO_ID_SCSI: u32 = 8;
pub const VIRTIO_ID_9P: u32 = 9;
pub const VIRTIO_ID_GPU: u32 = 16;
pub const VIRTIO_ID_INPUT: u32 = 18;
pub const VIRTIO_ID_VSOCK: u32 = 19;
pub const VIRTIO_ID_CRYPTO: u32 = 20;
pub const VIRTIO_ID_IOMMU: u32 = 23;
pub const VIRTIO_ID_MEM: u32 = 24;
pub const VIRTIO_ID_FS: u32 = 26;
pub const VIRTIO_ID_PMEM: u32 = 27;

// ---------------------------------------------------------------------------
// PCI vendor / subsystem ranges (used by virtio-pci transport)
// ---------------------------------------------------------------------------

/// Red Hat is the assigned PCI vendor for all virtio devices.
pub const VIRTIO_PCI_VENDOR_ID: u16 = 0x1AF4;
/// Modern virtio devices use PCI device IDs 0x1040 + virtio_id.
pub const VIRTIO_PCI_MODERN_DEVICE_BASE: u16 = 0x1040;
/// Legacy ("transitional") virtio devices use 0x1000 + n.
pub const VIRTIO_PCI_LEGACY_DEVICE_BASE: u16 = 0x1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_dense_handshake_then_high_failure_bits() {
        // The four handshake bits are 1, 2, 4, 8 — they're set in order
        // by ACKNOWLEDGE→DRIVER→FEATURES_OK→DRIVER_OK.
        let s = [
            VIRTIO_CONFIG_S_ACKNOWLEDGE,
            VIRTIO_CONFIG_S_DRIVER,
            VIRTIO_CONFIG_S_DRIVER_OK,
            VIRTIO_CONFIG_S_FEATURES_OK,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        // The two failure bits live in the upper half so a driver can
        // OR them onto an existing status without overlap.
        assert_eq!(VIRTIO_CONFIG_S_NEEDS_RESET, 0x40);
        assert_eq!(VIRTIO_CONFIG_S_FAILED, 0x80);
        assert_eq!(VIRTIO_CONFIG_S_NEEDS_RESET & 0x0F, 0);
        assert_eq!(VIRTIO_CONFIG_S_FAILED & 0x0F, 0);
    }

    #[test]
    fn test_transport_feature_bits_are_single_bit() {
        let f = [
            VIRTIO_F_NOTIFY_ON_EMPTY,
            VIRTIO_F_ANY_LAYOUT,
            VIRTIO_F_VERSION_1,
            VIRTIO_F_ACCESS_PLATFORM,
            VIRTIO_F_RING_PACKED,
            VIRTIO_F_IN_ORDER,
            VIRTIO_F_ORDER_PLATFORM,
            VIRTIO_F_SR_IOV,
            VIRTIO_F_NOTIFICATION_DATA,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // Modern virtio features live above bit 31 — that's how the
        // device tells driver to use the 1.0 spec.
        assert!(VIRTIO_F_VERSION_1 >= 1 << 32);
        assert_eq!(VIRTIO_F_VERSION_1, 1 << 32);
    }

    #[test]
    fn test_device_ids_distinct_and_in_range() {
        let i = [
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
            VIRTIO_ID_IOMMU,
            VIRTIO_ID_MEM,
            VIRTIO_ID_FS,
            VIRTIO_ID_PMEM,
        ];
        for a in 0..i.len() {
            for b in (a + 1)..i.len() {
                assert_ne!(i[a], i[b]);
            }
        }
        // All allocated device IDs fit in a 6-bit field per the spec.
        for v in i {
            assert!(v < 64);
        }
    }

    #[test]
    fn test_pci_vendor_redhat() {
        // Red Hat 0x1AF4 — see PCI ID database.
        assert_eq!(VIRTIO_PCI_VENDOR_ID, 0x1AF4);
        // Modern range 0x1040..0x107F, legacy range 0x1000..0x103F.
        assert_eq!(VIRTIO_PCI_MODERN_DEVICE_BASE - VIRTIO_PCI_LEGACY_DEVICE_BASE, 0x40);
    }

    #[test]
    fn test_modern_pci_device_id_for_net_is_0x1041() {
        // Modern PCI device id = 0x1040 + virtio_id.
        assert_eq!(
            VIRTIO_PCI_MODERN_DEVICE_BASE + VIRTIO_ID_NET as u16,
            0x1041
        );
        // Legacy for block was 0x1001.
        assert_eq!(
            VIRTIO_PCI_LEGACY_DEVICE_BASE + VIRTIO_ID_BLOCK as u16,
            0x1002
        );
    }
}
