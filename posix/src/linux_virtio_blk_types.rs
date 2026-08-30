//! `<linux/virtio_blk.h>` — VirtIO block device constants.
//!
//! virtio-blk provides virtual block storage to guest VMs. The
//! guest sends I/O requests (read, write, flush, discard) through
//! a virtqueue; the hypervisor processes them against a backing
//! store (disk image, LVM volume, network storage). It's the
//! primary storage interface for KVM/QEMU VMs, Firecracker
//! microVMs, and cloud instances.

// ---------------------------------------------------------------------------
// VirtIO block request types (VIRTIO_BLK_T_*)
// ---------------------------------------------------------------------------

/// Read request.
pub const VIRTIO_BLK_T_IN: u32 = 0;
/// Write request.
pub const VIRTIO_BLK_T_OUT: u32 = 1;
/// Flush (write barrier/sync).
pub const VIRTIO_BLK_T_FLUSH: u32 = 4;
/// Discard (trim/unmap).
pub const VIRTIO_BLK_T_DISCARD: u32 = 11;
/// Write zeros (efficient zero-fill).
pub const VIRTIO_BLK_T_WRITE_ZEROES: u32 = 13;
/// Secure erase.
pub const VIRTIO_BLK_T_SECURE_ERASE: u32 = 14;
/// Get device ID string.
pub const VIRTIO_BLK_T_GET_ID: u32 = 8;

// ---------------------------------------------------------------------------
// VirtIO block status codes
// ---------------------------------------------------------------------------

/// Request completed successfully.
pub const VIRTIO_BLK_S_OK: u32 = 0;
/// I/O error.
pub const VIRTIO_BLK_S_IOERR: u32 = 1;
/// Unsupported request type.
pub const VIRTIO_BLK_S_UNSUPP: u32 = 2;

// ---------------------------------------------------------------------------
// VirtIO block feature bits (VIRTIO_BLK_F_*)
// ---------------------------------------------------------------------------

/// Maximum size of any single segment.
pub const VIRTIO_BLK_F_SIZE_MAX: u32 = 1;
/// Maximum number of segments per request.
pub const VIRTIO_BLK_F_SEG_MAX: u32 = 2;
/// Disk geometry available.
pub const VIRTIO_BLK_F_GEOMETRY: u32 = 4;
/// Read-only device.
pub const VIRTIO_BLK_F_RO: u32 = 5;
/// Block size available.
pub const VIRTIO_BLK_F_BLK_SIZE: u32 = 6;
/// Flush command supported.
pub const VIRTIO_BLK_F_FLUSH: u32 = 9;
/// Device supports topology info.
pub const VIRTIO_BLK_F_TOPOLOGY: u32 = 10;
/// Device supports multi-queue.
pub const VIRTIO_BLK_F_MQ: u32 = 12;
/// Device supports discard.
pub const VIRTIO_BLK_F_DISCARD: u32 = 13;
/// Device supports write zeroes.
pub const VIRTIO_BLK_F_WRITE_ZEROES: u32 = 14;
/// Device supports secure erase.
pub const VIRTIO_BLK_F_SECURE_ERASE: u32 = 16;
/// Device supports zoned storage.
pub const VIRTIO_BLK_F_ZONED: u32 = 17;

// ---------------------------------------------------------------------------
// Device ID string length
// ---------------------------------------------------------------------------

/// Maximum length of virtio block device ID.
pub const VIRTIO_BLK_ID_BYTES: u32 = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_types_distinct() {
        let types = [
            VIRTIO_BLK_T_IN,
            VIRTIO_BLK_T_OUT,
            VIRTIO_BLK_T_FLUSH,
            VIRTIO_BLK_T_GET_ID,
            VIRTIO_BLK_T_DISCARD,
            VIRTIO_BLK_T_WRITE_ZEROES,
            VIRTIO_BLK_T_SECURE_ERASE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [VIRTIO_BLK_S_OK, VIRTIO_BLK_S_IOERR, VIRTIO_BLK_S_UNSUPP];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_features_distinct() {
        let feats = [
            VIRTIO_BLK_F_SIZE_MAX,
            VIRTIO_BLK_F_SEG_MAX,
            VIRTIO_BLK_F_GEOMETRY,
            VIRTIO_BLK_F_RO,
            VIRTIO_BLK_F_BLK_SIZE,
            VIRTIO_BLK_F_FLUSH,
            VIRTIO_BLK_F_TOPOLOGY,
            VIRTIO_BLK_F_MQ,
            VIRTIO_BLK_F_DISCARD,
            VIRTIO_BLK_F_WRITE_ZEROES,
            VIRTIO_BLK_F_SECURE_ERASE,
            VIRTIO_BLK_F_ZONED,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_ok_is_zero() {
        assert_eq!(VIRTIO_BLK_S_OK, 0);
    }

    #[test]
    fn test_id_bytes() {
        assert_eq!(VIRTIO_BLK_ID_BYTES, 20);
    }
}
