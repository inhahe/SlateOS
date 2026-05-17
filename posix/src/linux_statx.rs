//! `<linux/stat.h>` — statx() extended file attributes constants.
//!
//! statx() is the modern replacement for stat/lstat/fstat, returning
//! richer metadata in a versioned structure. It supports querying
//! only specific fields (saving kernel work), birth time, mount ID,
//! DAX status, and file attributes unavailable via traditional stat().

// ---------------------------------------------------------------------------
// statx mask flags (which fields to request)
// ---------------------------------------------------------------------------

/// Want stx_mode & S_IFMT.
pub const STATX_TYPE: u32 = 0x0000_0001;
/// Want stx_mode & ~S_IFMT.
pub const STATX_MODE: u32 = 0x0000_0002;
/// Want stx_nlink.
pub const STATX_NLINK: u32 = 0x0000_0004;
/// Want stx_uid.
pub const STATX_UID: u32 = 0x0000_0008;
/// Want stx_gid.
pub const STATX_GID: u32 = 0x0000_0010;
/// Want stx_atime.
pub const STATX_ATIME: u32 = 0x0000_0020;
/// Want stx_mtime.
pub const STATX_MTIME: u32 = 0x0000_0040;
/// Want stx_ctime.
pub const STATX_CTIME: u32 = 0x0000_0080;
/// Want stx_ino.
pub const STATX_INO: u32 = 0x0000_0100;
/// Want stx_size.
pub const STATX_SIZE: u32 = 0x0000_0200;
/// Want stx_blocks.
pub const STATX_BLOCKS: u32 = 0x0000_0400;
/// Basic stats (all of the above).
pub const STATX_BASIC_STATS: u32 = 0x0000_07FF;
/// Want stx_btime (birth/creation time).
pub const STATX_BTIME: u32 = 0x0000_0800;
/// Want stx_mnt_id.
pub const STATX_MNT_ID: u32 = 0x0000_1000;
/// Want stx_dio_mem_align and stx_dio_offset_align.
pub const STATX_DIOALIGN: u32 = 0x0000_2000;

// ---------------------------------------------------------------------------
// statx attributes (stx_attributes field)
// ---------------------------------------------------------------------------

/// File is compressed.
pub const STATX_ATTR_COMPRESSED: u64 = 0x0000_0004;
/// File is immutable.
pub const STATX_ATTR_IMMUTABLE: u64 = 0x0000_0010;
/// File is append-only.
pub const STATX_ATTR_APPEND: u64 = 0x0000_0020;
/// File is not backed up (nodump).
pub const STATX_ATTR_NODUMP: u64 = 0x0000_0040;
/// File is encrypted.
pub const STATX_ATTR_ENCRYPTED: u64 = 0x0000_0800;
/// File has fs-verity enabled.
pub const STATX_ATTR_VERITY: u64 = 0x0010_0000;
/// File is DAX (direct access, no page cache).
pub const STATX_ATTR_DAX: u64 = 0x0020_0000;
/// File has mount ID in stx_mnt_id.
pub const STATX_ATTR_MOUNT_ROOT: u64 = 0x2000_0000;

// ---------------------------------------------------------------------------
// AT_ flags used with statx (dirfd-relative operations)
// ---------------------------------------------------------------------------

/// Empty pathname (use dirfd itself).
pub const AT_EMPTY_PATH: u32 = 0x1000;
/// Don't follow symlinks.
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x0100;
/// Don't trigger automounts.
pub const AT_NO_AUTOMOUNT: u32 = 0x0800;
/// Force sync of attributes from server.
pub const AT_STATX_FORCE_SYNC: u32 = 0x2000;
/// Don't sync (use cached data).
pub const AT_STATX_DONT_SYNC: u32 = 0x4000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_flags_no_overlap() {
        let masks = [
            STATX_TYPE, STATX_MODE, STATX_NLINK, STATX_UID,
            STATX_GID, STATX_ATIME, STATX_MTIME, STATX_CTIME,
            STATX_INO, STATX_SIZE, STATX_BLOCKS, STATX_BTIME,
            STATX_MNT_ID, STATX_DIOALIGN,
        ];
        for i in 0..masks.len() {
            assert!(masks[i].is_power_of_two());
            for j in (i + 1)..masks.len() {
                assert_eq!(masks[i] & masks[j], 0);
            }
        }
    }

    #[test]
    fn test_basic_stats_combines() {
        let expected = STATX_TYPE | STATX_MODE | STATX_NLINK | STATX_UID
            | STATX_GID | STATX_ATIME | STATX_MTIME | STATX_CTIME
            | STATX_INO | STATX_SIZE | STATX_BLOCKS;
        assert_eq!(STATX_BASIC_STATS, expected);
    }

    #[test]
    fn test_attr_flags_no_overlap() {
        let attrs = [
            STATX_ATTR_COMPRESSED, STATX_ATTR_IMMUTABLE, STATX_ATTR_APPEND,
            STATX_ATTR_NODUMP, STATX_ATTR_ENCRYPTED, STATX_ATTR_VERITY,
            STATX_ATTR_DAX, STATX_ATTR_MOUNT_ROOT,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_at_flags_distinct() {
        let flags = [
            AT_EMPTY_PATH, AT_SYMLINK_NOFOLLOW, AT_NO_AUTOMOUNT,
            AT_STATX_FORCE_SYNC, AT_STATX_DONT_SYNC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
