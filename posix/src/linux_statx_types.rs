//! `<linux/stat.h>` — statx() attribute and mask constants.
//!
//! The statx() system call is an extended stat replacement providing
//! more metadata about files. It uses bitmask fields to indicate
//! which attributes are requested (mask) and which are supported/set
//! (attributes). This avoids the ambiguity of traditional stat().

// ---------------------------------------------------------------------------
// statx mask bits (what to query)
// ---------------------------------------------------------------------------

/// Want stx_mode and stx_ino.
pub const STATX_TYPE: u32 = 0x0000_0001;
/// Want stx_mode (permission bits).
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
/// Want basic stat fields (same as traditional stat).
pub const STATX_BASIC_STATS: u32 = 0x0000_07FF;
/// Want stx_btime (birth/creation time).
pub const STATX_BTIME: u32 = 0x0000_0800;
/// Want stx_mnt_id.
pub const STATX_MNT_ID: u32 = 0x0000_1000;
/// Want stx_dio_mem_align and stx_dio_offset_align.
pub const STATX_DIOALIGN: u32 = 0x0000_2000;

// ---------------------------------------------------------------------------
// statx attribute flags (stx_attributes / stx_attributes_mask)
// ---------------------------------------------------------------------------

/// File is compressed.
pub const STATX_ATTR_COMPRESSED: u64 = 0x0000_0004;
/// File is immutable.
pub const STATX_ATTR_IMMUTABLE: u64 = 0x0000_0010;
/// File is append-only.
pub const STATX_ATTR_APPEND: u64 = 0x0000_0020;
/// File is not a candidate for backup.
pub const STATX_ATTR_NODUMP: u64 = 0x0000_0040;
/// File is encrypted.
pub const STATX_ATTR_ENCRYPTED: u64 = 0x0000_0800;
/// Directory is automount trigger.
pub const STATX_ATTR_AUTOMOUNT: u64 = 0x0000_1000;
/// Directory/file is a mount root.
pub const STATX_ATTR_MOUNT_ROOT: u64 = 0x0000_2000;
/// File has fs-verity enabled.
pub const STATX_ATTR_VERITY: u64 = 0x0010_0000;
/// File is DAX (direct access, no page cache).
pub const STATX_ATTR_DAX: u64 = 0x0020_0000;

// ---------------------------------------------------------------------------
// statx() flags (AT_* flags passed in the flags argument)
// ---------------------------------------------------------------------------

/// Synchronize with server (network FS, force fresh data).
pub const AT_STATX_FORCE_SYNC: u32 = 0x2000;
/// Don't synchronize (use cached data if available).
pub const AT_STATX_DONT_SYNC: u32 = 0x4000;
/// Synchronize as needed (default).
pub const AT_STATX_SYNC_AS_STAT: u32 = 0x0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_bits_no_overlap() {
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
    fn test_attr_flags_distinct() {
        let attrs = [
            STATX_ATTR_COMPRESSED, STATX_ATTR_IMMUTABLE,
            STATX_ATTR_APPEND, STATX_ATTR_NODUMP,
            STATX_ATTR_ENCRYPTED, STATX_ATTR_AUTOMOUNT,
            STATX_ATTR_MOUNT_ROOT, STATX_ATTR_VERITY,
            STATX_ATTR_DAX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_sync_flags_distinct() {
        assert_ne!(AT_STATX_FORCE_SYNC, AT_STATX_DONT_SYNC);
        assert_ne!(AT_STATX_FORCE_SYNC, AT_STATX_SYNC_AS_STAT);
    }
}
