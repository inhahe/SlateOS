//! `<linux/vhost.h>` — Additional vhost constants.
//!
//! Supplementary vhost constants covering IOTLB message types,
//! backend feature flags, and vring state operations.

// ---------------------------------------------------------------------------
// vhost IOTLB message types
// ---------------------------------------------------------------------------

/// IOTLB miss.
pub const VHOST_IOTLB_MISS: u32 = 1;
/// IOTLB update.
pub const VHOST_IOTLB_UPDATE: u32 = 2;
/// IOTLB invalidate.
pub const VHOST_IOTLB_INVALIDATE: u32 = 3;
/// IOTLB access failed.
pub const VHOST_IOTLB_ACCESS_FAIL: u32 = 4;
/// IOTLB batch begin.
pub const VHOST_IOTLB_BATCH_BEGIN: u32 = 5;
/// IOTLB batch end.
pub const VHOST_IOTLB_BATCH_END: u32 = 6;

// ---------------------------------------------------------------------------
// vhost IOTLB permission flags
// ---------------------------------------------------------------------------

/// Read permission.
pub const VHOST_ACCESS_RO: u32 = 0;
/// Write permission.
pub const VHOST_ACCESS_WO: u32 = 1;
/// Read/write permission.
pub const VHOST_ACCESS_RW: u32 = 2;

// ---------------------------------------------------------------------------
// vhost backend features
// ---------------------------------------------------------------------------

/// IOTLB v2 message format.
pub const VHOST_BACKEND_F_IOTLB_MSG_V2: u32 = 1;
/// IOTLB batching.
pub const VHOST_BACKEND_F_IOTLB_BATCH: u32 = 2;
/// IOTLB ASID support.
pub const VHOST_BACKEND_F_IOTLB_ASID: u32 = 3;
/// Suspend support.
pub const VHOST_BACKEND_F_SUSPEND: u32 = 4;
/// Resume support.
pub const VHOST_BACKEND_F_RESUME: u32 = 5;
/// Enable queue after reset.
pub const VHOST_BACKEND_F_ENABLE_AFTER_DRIVER_OK: u32 = 6;
/// Descriptor mapping support.
pub const VHOST_BACKEND_F_DESC_ASID: u32 = 7;

// ---------------------------------------------------------------------------
// vhost vring file flags
// ---------------------------------------------------------------------------

/// Poll start.
pub const VHOST_FILE_UNBIND: i32 = -1;

// ---------------------------------------------------------------------------
// vhost-net feature flags
// ---------------------------------------------------------------------------

/// Virtio net mergeable receive buffers.
pub const VHOST_NET_F_VIRTIO_NET_HDR: u32 = 27;

// ---------------------------------------------------------------------------
// vhost-scsi feature flags
// ---------------------------------------------------------------------------

/// SCSI target hotplug.
pub const VHOST_SCSI_ABI_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// vhost-vsock CID
// ---------------------------------------------------------------------------

/// Default vhost-vsock CID range start.
pub const VHOST_VSOCK_SET_GUEST_CID: u32 = 0x6001;
/// Start vhost-vsock.
pub const VHOST_VSOCK_SET_RUNNING: u32 = 0x6101;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iotlb_types_distinct() {
        let types = [
            VHOST_IOTLB_MISS,
            VHOST_IOTLB_UPDATE,
            VHOST_IOTLB_INVALIDATE,
            VHOST_IOTLB_ACCESS_FAIL,
            VHOST_IOTLB_BATCH_BEGIN,
            VHOST_IOTLB_BATCH_END,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_access_perms_distinct() {
        let perms = [VHOST_ACCESS_RO, VHOST_ACCESS_WO, VHOST_ACCESS_RW];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_ne!(perms[i], perms[j]);
            }
        }
    }

    #[test]
    fn test_backend_features_distinct() {
        let feats = [
            VHOST_BACKEND_F_IOTLB_MSG_V2,
            VHOST_BACKEND_F_IOTLB_BATCH,
            VHOST_BACKEND_F_IOTLB_ASID,
            VHOST_BACKEND_F_SUSPEND,
            VHOST_BACKEND_F_RESUME,
            VHOST_BACKEND_F_ENABLE_AFTER_DRIVER_OK,
            VHOST_BACKEND_F_DESC_ASID,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_vsock_ioctls_distinct() {
        assert_ne!(VHOST_VSOCK_SET_GUEST_CID, VHOST_VSOCK_SET_RUNNING);
    }

    #[test]
    fn test_file_unbind_negative() {
        assert!(VHOST_FILE_UNBIND < 0);
    }
}
