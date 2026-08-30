//! `<linux/fs.h>` — core filesystem userspace ABI.
//!
//! Common ioctls every userspace filesystem tool talks to: GETFLAGS
//! for chattr/lsattr (immutable, append-only, …), FITRIM for fstrim,
//! FICLONE/FIDEDUPERANGE for cp --reflink and bees dedupe, FIEMAP for
//! filefrag, BLKDISCARD for blkdiscard, etc.

// ---------------------------------------------------------------------------
// Block-size constants
// ---------------------------------------------------------------------------

/// Filesystem block size (kernel default).
pub const FS_BLOCK_SIZE: u32 = 4096;
/// Maximum lookahead readahead window (256 pages).
pub const FS_DEFAULT_READAHEAD: u32 = 128 * 1024;

// ---------------------------------------------------------------------------
// File-attribute flags (FS_IOC_GETFLAGS / FS_IOC_SETFLAGS)
// ---------------------------------------------------------------------------

/// Secure deletion.
pub const FS_SECRM_FL: u32 = 0x0000_0001;
/// Undelete.
pub const FS_UNRM_FL: u32 = 0x0000_0002;
/// Compress file.
pub const FS_COMPR_FL: u32 = 0x0000_0004;
/// Synchronous updates.
pub const FS_SYNC_FL: u32 = 0x0000_0008;
/// Immutable file.
pub const FS_IMMUTABLE_FL: u32 = 0x0000_0010;
/// Append-only.
pub const FS_APPEND_FL: u32 = 0x0000_0020;
/// Do not dump.
pub const FS_NODUMP_FL: u32 = 0x0000_0040;
/// Do not update atime.
pub const FS_NOATIME_FL: u32 = 0x0000_0080;
/// Directory: index for lookups.
pub const FS_INDEX_FL: u32 = 0x0000_1000;
/// AFS directory.
pub const FS_DIRSYNC_FL: u32 = 0x0001_0000;
/// Top of directory hierarchies.
pub const FS_TOPDIR_FL: u32 = 0x0002_0000;
/// Encryption inode flag.
pub const FS_ENCRYPT_FL: u32 = 0x0000_4000;
/// Reserved for ext2 list.
pub const FS_RESERVED_FL: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Key ioctls (group letter 'f' or 'X')
// ---------------------------------------------------------------------------

/// `FS_IOC_GETFLAGS` — read attribute flags.
pub const FS_IOC_GETFLAGS: u32 = 0x8008_6601;
/// `FS_IOC_SETFLAGS` — write attribute flags.
pub const FS_IOC_SETFLAGS: u32 = 0x4008_6602;
/// `FS_IOC_GETVERSION` — generation number.
pub const FS_IOC_GETVERSION: u32 = 0x8008_6601_u32.wrapping_add(0);
/// `FS_IOC_FIEMAP` — file extents.
pub const FS_IOC_FIEMAP: u32 = 0xC020_660B;
/// `FITRIM` — discard unused blocks.
pub const FITRIM: u32 = 0xC018_5879;
/// `FICLONE` — clone whole file (reflink).
pub const FICLONE: u32 = 0x4004_9409;
/// `FICLONERANGE` — clone a byte range.
pub const FICLONERANGE: u32 = 0x4020_940D;
/// `FIDEDUPERANGE` — deduplicate byte range.
pub const FIDEDUPERANGE: u32 = 0xC018_9436;

// ---------------------------------------------------------------------------
// FIEMAP flags
// ---------------------------------------------------------------------------

/// Sync file before mapping.
pub const FIEMAP_FLAG_SYNC: u32 = 0x0000_0001;
/// Map extended attribute tree.
pub const FIEMAP_FLAG_XATTR: u32 = 0x0000_0002;
/// Cache extents.
pub const FIEMAP_FLAG_CACHE: u32 = 0x0000_0004;

/// Extent is last in file.
pub const FIEMAP_EXTENT_LAST: u32 = 0x0000_0001;
/// Mapping is approximate (compressed/sparse).
pub const FIEMAP_EXTENT_UNKNOWN: u32 = 0x0000_0002;
/// Data is delayed-allocation (not on disk yet).
pub const FIEMAP_EXTENT_DELALLOC: u32 = 0x0000_0004;
/// Data is encoded (compressed/encrypted).
pub const FIEMAP_EXTENT_ENCODED: u32 = 0x0000_0008;
/// Data is encrypted.
pub const FIEMAP_EXTENT_DATA_ENCRYPTED: u32 = 0x0000_0080;
/// Not block aligned.
pub const FIEMAP_EXTENT_NOT_ALIGNED: u32 = 0x0000_0100;
/// Inline data (in inode or block tail).
pub const FIEMAP_EXTENT_DATA_INLINE: u32 = 0x0000_0200;
/// In tail packing.
pub const FIEMAP_EXTENT_DATA_TAIL: u32 = 0x0000_0400;
/// Unwritten preallocated extent.
pub const FIEMAP_EXTENT_UNWRITTEN: u32 = 0x0000_0800;
/// Data is shared by multiple files.
pub const FIEMAP_EXTENT_SHARED: u32 = 0x0000_2000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_sizes() {
        // 4 KiB is the historical default page-equivalent block; readahead
        // sticks at 128 KiB on most filesystems.
        assert_eq!(FS_BLOCK_SIZE, 4096);
        assert_eq!(FS_DEFAULT_READAHEAD, 131_072);
    }

    #[test]
    fn test_attribute_flags_distinct() {
        let f = [
            FS_SECRM_FL,
            FS_UNRM_FL,
            FS_COMPR_FL,
            FS_SYNC_FL,
            FS_IMMUTABLE_FL,
            FS_APPEND_FL,
            FS_NODUMP_FL,
            FS_NOATIME_FL,
            FS_INDEX_FL,
            FS_DIRSYNC_FL,
            FS_TOPDIR_FL,
            FS_ENCRYPT_FL,
            FS_RESERVED_FL,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ops = [
            FS_IOC_GETFLAGS,
            FS_IOC_SETFLAGS,
            FS_IOC_FIEMAP,
            FITRIM,
            FICLONE,
            FICLONERANGE,
            FIDEDUPERANGE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_fiemap_call_flags_pow2() {
        let f = [FIEMAP_FLAG_SYNC, FIEMAP_FLAG_XATTR, FIEMAP_FLAG_CACHE];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_fiemap_extent_flags_distinct() {
        let e = [
            FIEMAP_EXTENT_LAST,
            FIEMAP_EXTENT_UNKNOWN,
            FIEMAP_EXTENT_DELALLOC,
            FIEMAP_EXTENT_ENCODED,
            FIEMAP_EXTENT_DATA_ENCRYPTED,
            FIEMAP_EXTENT_NOT_ALIGNED,
            FIEMAP_EXTENT_DATA_INLINE,
            FIEMAP_EXTENT_DATA_TAIL,
            FIEMAP_EXTENT_UNWRITTEN,
            FIEMAP_EXTENT_SHARED,
        ];
        for &b in &e {
            assert!(b.is_power_of_two());
        }
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }
}
