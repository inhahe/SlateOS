//! `<linux/stat.h>` — Additional statx constants.
//!
//! Supplementary statx constants covering attribute masks,
//! attribute flags, and additional mode flags.

// ---------------------------------------------------------------------------
// statx mask (STATX_*)
// ---------------------------------------------------------------------------

/// Type (mode & S_IFMT).
pub const STATX_TYPE: u32 = 0x00000001;
/// Mode (permissions).
pub const STATX_MODE: u32 = 0x00000002;
/// Number of hard links.
pub const STATX_NLINK: u32 = 0x00000004;
/// User ID.
pub const STATX_UID: u32 = 0x00000008;
/// Group ID.
pub const STATX_GID: u32 = 0x00000010;
/// Access time.
pub const STATX_ATIME: u32 = 0x00000020;
/// Modification time.
pub const STATX_MTIME: u32 = 0x00000040;
/// Change time.
pub const STATX_CTIME: u32 = 0x00000080;
/// Inode number.
pub const STATX_INO: u32 = 0x00000100;
/// Size.
pub const STATX_SIZE: u32 = 0x00000200;
/// Number of 512B blocks.
pub const STATX_BLOCKS: u32 = 0x00000400;
/// All basic stats.
pub const STATX_BASIC_STATS: u32 = 0x000007FF;
/// Birth time.
pub const STATX_BTIME: u32 = 0x00000800;
/// Mount ID.
pub const STATX_MNT_ID: u32 = 0x00001000;
/// DIOALIGN.
pub const STATX_DIOALIGN: u32 = 0x00002000;
/// Mount ID unique.
pub const STATX_MNT_ID_UNIQUE: u32 = 0x00004000;
/// Subvolume.
pub const STATX_SUBVOL: u32 = 0x00008000;

// ---------------------------------------------------------------------------
// statx attributes (STATX_ATTR_*)
// ---------------------------------------------------------------------------

/// Compressed.
pub const STATX_ATTR_COMPRESSED: u64 = 0x00000004;
/// Immutable.
pub const STATX_ATTR_IMMUTABLE: u64 = 0x00000010;
/// Append only.
pub const STATX_ATTR_APPEND: u64 = 0x00000020;
/// No dump.
pub const STATX_ATTR_NODUMP: u64 = 0x00000040;
/// Encrypted.
pub const STATX_ATTR_ENCRYPTED: u64 = 0x00000800;
/// Automount.
pub const STATX_ATTR_AUTOMOUNT: u64 = 0x00001000;
/// Mount root.
pub const STATX_ATTR_MOUNT_ROOT: u64 = 0x00002000;
/// Verity.
pub const STATX_ATTR_VERITY: u64 = 0x00100000;
/// DAX.
pub const STATX_ATTR_DAX: u64 = 0x00200000;

// ---------------------------------------------------------------------------
// statx flags (AT_*)
// ---------------------------------------------------------------------------

/// No automount.
pub const AT_STATX_SYNC_TYPE: u32 = 0x6000;
/// Force sync.
pub const AT_STATX_FORCE_SYNC: u32 = 0x2000;
/// Don't sync.
pub const AT_STATX_DONT_SYNC: u32 = 0x4000;
/// Sync as stat.
pub const AT_STATX_SYNC_AS_STAT: u32 = 0x0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_power_of_two() {
        let masks = [
            STATX_TYPE, STATX_MODE, STATX_NLINK, STATX_UID,
            STATX_GID, STATX_ATIME, STATX_MTIME, STATX_CTIME,
            STATX_INO, STATX_SIZE, STATX_BLOCKS, STATX_BTIME,
            STATX_MNT_ID, STATX_DIOALIGN, STATX_MNT_ID_UNIQUE,
            STATX_SUBVOL,
        ];
        for m in &masks {
            assert!(m.is_power_of_two(), "0x{:08x} not power of two", m);
        }
    }

    #[test]
    fn test_basic_stats() {
        // BASIC_STATS should include TYPE through BLOCKS (bits 0-10)
        assert_eq!(STATX_BASIC_STATS, 0x7FF);
        assert_eq!(
            STATX_BASIC_STATS,
            STATX_TYPE | STATX_MODE | STATX_NLINK | STATX_UID
                | STATX_GID | STATX_ATIME | STATX_MTIME | STATX_CTIME
                | STATX_INO | STATX_SIZE | STATX_BLOCKS
        );
    }

    #[test]
    fn test_attr_distinct() {
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
        let flags = [
            AT_STATX_SYNC_AS_STAT, AT_STATX_FORCE_SYNC,
            AT_STATX_DONT_SYNC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
