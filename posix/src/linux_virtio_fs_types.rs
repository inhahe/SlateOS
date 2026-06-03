//! `<linux/virtio_fs.h>` — VirtIO filesystem device (virtiofs) constants.
//!
//! virtio-fs provides a FUSE-based shared filesystem between a host
//! and guest. Unlike 9P, it uses DAX (direct access) window mapping
//! for zero-copy file access and supports metadata caching. The
//! device exposes FUSE requests over virtqueues.

// ---------------------------------------------------------------------------
// VirtIO-FS virtqueue indices
// ---------------------------------------------------------------------------

/// High-priority request queue (notifications).
pub const VIRTIO_FS_VQ_HIPRIO: u32 = 0;
/// First normal request queue.
pub const VIRTIO_FS_VQ_REQUEST_BASE: u32 = 1;

// ---------------------------------------------------------------------------
// VirtIO-FS feature bits
// ---------------------------------------------------------------------------

/// Device supports DAX window (shared memory region for mmap).
pub const VIRTIO_FS_F_NOTIFICATION: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// FUSE operation codes (subset used by virtio-fs)
// ---------------------------------------------------------------------------

/// Look up a directory entry.
pub const FUSE_LOOKUP: u32 = 1;
/// Get file attributes.
pub const FUSE_GETATTR: u32 = 3;
/// Set file attributes.
pub const FUSE_SETATTR: u32 = 4;
/// Read symbolic link.
pub const FUSE_READLINK: u32 = 5;
/// Create file.
pub const FUSE_MKNOD: u32 = 8;
/// Create directory.
pub const FUSE_MKDIR: u32 = 9;
/// Remove file.
pub const FUSE_UNLINK: u32 = 10;
/// Remove directory.
pub const FUSE_RMDIR: u32 = 11;
/// Rename file.
pub const FUSE_RENAME: u32 = 12;
/// Open file.
pub const FUSE_OPEN: u32 = 14;
/// Read file data.
pub const FUSE_READ: u32 = 15;
/// Write file data.
pub const FUSE_WRITE: u32 = 16;
/// Flush (close) file.
pub const FUSE_FLUSH: u32 = 25;
/// Read directory entries.
pub const FUSE_READDIR: u32 = 28;
/// Sync file data to storage.
pub const FUSE_FSYNC: u32 = 20;
/// Initialize FUSE connection.
pub const FUSE_INIT: u32 = 26;
/// Destroy FUSE connection.
pub const FUSE_DESTROY: u32 = 38;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vq_indices() {
        assert_eq!(VIRTIO_FS_VQ_HIPRIO, 0);
        assert_eq!(VIRTIO_FS_VQ_REQUEST_BASE, 1);
    }

    #[test]
    fn test_fuse_ops_distinct() {
        let ops = [
            FUSE_LOOKUP,
            FUSE_GETATTR,
            FUSE_SETATTR,
            FUSE_READLINK,
            FUSE_MKNOD,
            FUSE_MKDIR,
            FUSE_UNLINK,
            FUSE_RMDIR,
            FUSE_RENAME,
            FUSE_OPEN,
            FUSE_READ,
            FUSE_WRITE,
            FUSE_FLUSH,
            FUSE_READDIR,
            FUSE_FSYNC,
            FUSE_INIT,
            FUSE_DESTROY,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_fuse_ops_nonzero() {
        assert!(FUSE_LOOKUP > 0);
        assert!(FUSE_INIT > 0);
    }
}
