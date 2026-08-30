//! `<linux/vhost.h>` — Vhost (virtio host) framework constants.
//!
//! Vhost implements the virtio device backend in the kernel for
//! high-performance I/O virtualization. Instead of emulating devices
//! in userspace (QEMU), vhost handles virtqueue processing directly
//! in the kernel, reducing context switches and copies.

// ---------------------------------------------------------------------------
// Vhost backend types
// ---------------------------------------------------------------------------

/// Vhost-net (network).
pub const VHOST_BACKEND_NET: u8 = 0;
/// Vhost-scsi (SCSI target).
pub const VHOST_BACKEND_SCSI: u8 = 1;
/// Vhost-vsock (AF_VSOCK).
pub const VHOST_BACKEND_VSOCK: u8 = 2;
/// Vhost-user (userspace backend, delegated).
pub const VHOST_BACKEND_USER: u8 = 3;
/// Vhost-vDPA (virtual data path acceleration).
pub const VHOST_BACKEND_VDPA: u8 = 4;

// ---------------------------------------------------------------------------
// Vhost ioctl types
// ---------------------------------------------------------------------------

/// Get API version.
pub const VHOST_GET_FEATURES: u32 = 0x8008AF00;
/// Set features.
pub const VHOST_SET_FEATURES: u32 = 0x4008AF00;
/// Set owner (current process).
pub const VHOST_SET_OWNER: u32 = 0x0000AF01;
/// Reset owner.
pub const VHOST_RESET_OWNER: u32 = 0x0000AF02;
/// Set memory table.
pub const VHOST_SET_MEM_TABLE: u32 = 0x4008AF03;
/// Set vring num (queue size).
pub const VHOST_SET_VRING_NUM: u32 = 0x4008AF10;
/// Set vring base (avail index).
pub const VHOST_SET_VRING_BASE: u32 = 0x4008AF12;
/// Set vring kick (eventfd).
pub const VHOST_SET_VRING_KICK: u32 = 0x4008AF20;
/// Set vring call (eventfd).
pub const VHOST_SET_VRING_CALL: u32 = 0x4008AF21;

// ---------------------------------------------------------------------------
// Vhost-user protocol features
// ---------------------------------------------------------------------------

/// Multi-queue support.
pub const VHOST_USER_F_MQ: u64 = 1 << 0;
/// Log all writes (for migration).
pub const VHOST_USER_F_LOG_ALL: u64 = 1 << 1;
/// Backend can remap.
pub const VHOST_USER_F_REMAP: u64 = 1 << 2;
/// Reply acknowledgement.
pub const VHOST_USER_F_REPLY_ACK: u64 = 1 << 3;
/// Cross-endian support.
pub const VHOST_USER_F_CROSS_ENDIAN: u64 = 1 << 6;

// ---------------------------------------------------------------------------
// Vhost IOTLB message types
// ---------------------------------------------------------------------------

/// IOTLB miss.
pub const VHOST_IOTLB_MISS: u8 = 1;
/// IOTLB update.
pub const VHOST_IOTLB_UPDATE: u8 = 2;
/// IOTLB invalidate.
pub const VHOST_IOTLB_INVALIDATE: u8 = 3;
/// Access failed.
pub const VHOST_IOTLB_ACCESS_FAIL: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_types_distinct() {
        let types = [
            VHOST_BACKEND_NET,
            VHOST_BACKEND_SCSI,
            VHOST_BACKEND_VSOCK,
            VHOST_BACKEND_USER,
            VHOST_BACKEND_VDPA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_user_features_no_overlap() {
        let feats = [
            VHOST_USER_F_MQ,
            VHOST_USER_F_LOG_ALL,
            VHOST_USER_F_REMAP,
            VHOST_USER_F_REPLY_ACK,
            VHOST_USER_F_CROSS_ENDIAN,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_iotlb_types_distinct() {
        let types = [
            VHOST_IOTLB_MISS,
            VHOST_IOTLB_UPDATE,
            VHOST_IOTLB_INVALIDATE,
            VHOST_IOTLB_ACCESS_FAIL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
