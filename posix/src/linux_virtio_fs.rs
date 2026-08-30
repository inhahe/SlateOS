//! `<linux/virtio_fs.h>` — Virtio filesystem device constants.
//!
//! Virtio-fs provides shared filesystem access in VMs using FUSE
//! protocol over virtqueues. Allows guests to mount host directories
//! with near-native performance via DAX (direct access).

pub use crate::linux_virtio_types::VIRTIO_ID_FS;

// ---------------------------------------------------------------------------
// Feature bits
// ---------------------------------------------------------------------------

/// DAX window (direct memory mapping of files).
pub const VIRTIO_FS_F_NOTIFICATION: u32 = 0;

// ---------------------------------------------------------------------------
// Request queue indices
// ---------------------------------------------------------------------------

/// High-priority request queue.
pub const VIRTIO_FS_QUEUE_HIPRIO: u32 = 0;
/// Normal request queue (first).
pub const VIRTIO_FS_QUEUE_REQUEST: u32 = 1;

// ---------------------------------------------------------------------------
// DAX window constants
// ---------------------------------------------------------------------------

/// FUSE SETUPMAPPING opcode.
pub const FUSE_SETUPMAPPING: u32 = 48;
/// FUSE REMOVEMAPPING opcode.
pub const FUSE_REMOVEMAPPING: u32 = 49;

/// FUSE setupmapping flag: read.
pub const FUSE_SETUPMAPPING_FLAG_READ: u64 = 1 << 0;
/// FUSE setupmapping flag: write.
pub const FUSE_SETUPMAPPING_FLAG_WRITE: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Tag length
// ---------------------------------------------------------------------------

/// Maximum filesystem tag length.
pub const VIRTIO_FS_TAG_LEN: usize = 36;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_indices() {
        assert_ne!(VIRTIO_FS_QUEUE_HIPRIO, VIRTIO_FS_QUEUE_REQUEST);
        assert_eq!(VIRTIO_FS_QUEUE_HIPRIO, 0);
        assert_eq!(VIRTIO_FS_QUEUE_REQUEST, 1);
    }

    #[test]
    fn test_fuse_opcodes() {
        assert_ne!(FUSE_SETUPMAPPING, FUSE_REMOVEMAPPING);
    }

    #[test]
    fn test_mapping_flags() {
        assert!(FUSE_SETUPMAPPING_FLAG_READ.is_power_of_two());
        assert!(FUSE_SETUPMAPPING_FLAG_WRITE.is_power_of_two());
        assert_ne!(FUSE_SETUPMAPPING_FLAG_READ, FUSE_SETUPMAPPING_FLAG_WRITE);
    }

    #[test]
    fn test_tag_len() {
        assert_eq!(VIRTIO_FS_TAG_LEN, 36);
    }

    #[test]
    fn test_virtio_id() {
        assert_eq!(VIRTIO_ID_FS, 26);
    }
}
