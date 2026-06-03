//! `<linux/blkpg.h>` — block device partition management.
//!
//! The BLKPG ioctl allows userspace to add, delete, and resize
//! partitions on block devices without re-reading the partition table.

// ---------------------------------------------------------------------------
// BLKPG ioctl
// ---------------------------------------------------------------------------

/// Block device partition management ioctl.
pub const BLKPG: u64 = 0x1269;

// ---------------------------------------------------------------------------
// BLKPG operations
// ---------------------------------------------------------------------------

/// Add a partition.
pub const BLKPG_ADD_PARTITION: i32 = 1;
/// Delete a partition.
pub const BLKPG_DEL_PARTITION: i32 = 2;
/// Resize a partition.
pub const BLKPG_RESIZE_PARTITION: i32 = 3;

// ---------------------------------------------------------------------------
// Other block device ioctls
// ---------------------------------------------------------------------------

/// Get device size in 512-byte sectors.
pub const BLKGETSIZE: u64 = 0x1260;
/// Flush buffer cache.
pub const BLKFLSBUF: u64 = 0x1261;
/// Set read-ahead.
pub const BLKRASET: u64 = 0x1262;
/// Get read-ahead.
pub const BLKRAGET: u64 = 0x1263;
/// Get sector size.
pub const BLKSSZGET: u64 = 0x1268;
/// Get device size in bytes (u64).
pub const BLKGETSIZE64: u64 = 0x80081272;
/// Set block size.
pub const BLKBSZSET: u64 = 0x40081271;
/// Get block size.
pub const BLKBSZGET: u64 = 0x80081270;
/// Discard sectors.
pub const BLKDISCARD: u64 = 0x1277;
/// Secure discard sectors.
pub const BLKSECDISCARD: u64 = 0x127D;
/// Zero out sectors.
pub const BLKZEROOUT: u64 = 0x127F;
/// Re-read partition table.
pub const BLKRRPART: u64 = 0x125F;

// ---------------------------------------------------------------------------
// BlkpgPartition struct
// ---------------------------------------------------------------------------

/// Maximum partition name length.
pub const BLKPG_DEVNAMELTH: usize = 64;
/// Maximum volume name length.
pub const BLKPG_VOLNAMELTH: usize = 64;

/// Block device partition descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BlkpgPartition {
    /// Start offset (bytes).
    pub start: i64,
    /// Length (bytes).
    pub length: i64,
    /// Partition number.
    pub pno: i32,
    /// Device name.
    pub devname: [u8; BLKPG_DEVNAMELTH],
    /// Volume name.
    pub volname: [u8; BLKPG_VOLNAMELTH],
}

impl BlkpgPartition {
    /// Create a zeroed partition descriptor.
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
    fn test_blkpg_operations() {
        assert_eq!(BLKPG_ADD_PARTITION, 1);
        assert_eq!(BLKPG_DEL_PARTITION, 2);
        assert_eq!(BLKPG_RESIZE_PARTITION, 3);
    }

    #[test]
    fn test_block_ioctls_distinct() {
        let cmds = [
            BLKPG,
            BLKGETSIZE,
            BLKFLSBUF,
            BLKRASET,
            BLKRAGET,
            BLKSSZGET,
            BLKGETSIZE64,
            BLKBSZSET,
            BLKBSZGET,
            BLKDISCARD,
            BLKSECDISCARD,
            BLKZEROOUT,
            BLKRRPART,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_partition_zeroed() {
        let part = BlkpgPartition::zeroed();
        assert_eq!(part.start, 0);
        assert_eq!(part.length, 0);
        assert_eq!(part.pno, 0);
    }

    #[test]
    fn test_name_lengths() {
        assert_eq!(BLKPG_DEVNAMELTH, 64);
        assert_eq!(BLKPG_VOLNAMELTH, 64);
    }
}
