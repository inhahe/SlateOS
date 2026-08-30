//! `<sys/vfs.h>` / `<sys/statfs.h>` — statfs() constants.
//!
//! `statfs()` and `fstatfs()` return filesystem statistics
//! via `struct statfs`.  These constants define the structure
//! layout and common filesystem magic numbers not already
//! covered by `linux_fs_magic_types`.

// ---------------------------------------------------------------------------
// struct statfs field offsets (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of f_type (filesystem type magic) in struct statfs.
pub const STATFS_OFF_TYPE: u32 = 0;
/// Offset of f_bsize (optimal block size) in struct statfs.
pub const STATFS_OFF_BSIZE: u32 = 8;
/// Offset of f_blocks (total blocks) in struct statfs.
pub const STATFS_OFF_BLOCKS: u32 = 16;
/// Offset of f_bfree (free blocks) in struct statfs.
pub const STATFS_OFF_BFREE: u32 = 24;
/// Offset of f_bavail (available blocks) in struct statfs.
pub const STATFS_OFF_BAVAIL: u32 = 32;
/// Offset of f_files (total inodes) in struct statfs.
pub const STATFS_OFF_FILES: u32 = 40;
/// Offset of f_ffree (free inodes) in struct statfs.
pub const STATFS_OFF_FFREE: u32 = 48;
/// Offset of f_fsid (filesystem ID) in struct statfs.
pub const STATFS_OFF_FSID: u32 = 56;
/// Offset of f_namelen (max filename length) in struct statfs.
pub const STATFS_OFF_NAMELEN: u32 = 64;
/// Offset of f_frsize (fragment size) in struct statfs.
pub const STATFS_OFF_FRSIZE: u32 = 72;
/// Offset of f_flags (mount flags) in struct statfs.
pub const STATFS_OFF_FLAGS: u32 = 80;

/// Size of struct statfs on Linux x86_64 (bytes).
pub const STATFS_SIZE: u32 = 120;

// ---------------------------------------------------------------------------
// Pseudo-filesystem magic numbers
// ---------------------------------------------------------------------------

/// proc filesystem magic.
pub const PROC_SUPER_MAGIC: u32 = 0x9FA0;
/// sysfs magic.
pub const SYSFS_MAGIC: u32 = 0x62656572;
/// devtmpfs / devfs magic.
pub const DEVTMPFS_MAGIC: u32 = 0x9FA0;
/// tmpfs magic.
pub const TMPFS_MAGIC: u32 = 0x01021994;
/// ramfs magic.
pub const RAMFS_MAGIC: u32 = 0x858458F6;
/// securityfs magic.
pub const SECURITYFS_MAGIC: u32 = 0x73636673;
/// cgroup v1 magic.
pub const CGROUP_SUPER_MAGIC: u32 = 0x27E0EB;
/// cgroup v2 magic.
pub const CGROUP2_SUPER_MAGIC: u32 = 0x63677270;
/// debugfs magic.
pub const DEBUGFS_MAGIC: u32 = 0x64626720;
/// binfmt_misc magic.
pub const BINFMTFS_MAGIC: u32 = 0x42494E4D;
/// pipefs magic.
pub const PIPEFS_MAGIC: u32 = 0x50495045;
/// sockfs magic.
pub const SOCKFS_MAGIC: u32 = 0x534F434B;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            STATFS_OFF_TYPE,
            STATFS_OFF_BSIZE,
            STATFS_OFF_BLOCKS,
            STATFS_OFF_BFREE,
            STATFS_OFF_BAVAIL,
            STATFS_OFF_FILES,
            STATFS_OFF_FFREE,
            STATFS_OFF_FSID,
            STATFS_OFF_NAMELEN,
            STATFS_OFF_FRSIZE,
            STATFS_OFF_FLAGS,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(STATFS_OFF_FLAGS < STATFS_SIZE);
    }

    #[test]
    fn test_struct_size() {
        assert_eq!(STATFS_SIZE, 120);
    }

    #[test]
    fn test_type_at_start() {
        assert_eq!(STATFS_OFF_TYPE, 0);
    }

    #[test]
    fn test_magic_numbers_distinct() {
        let magics = [
            TMPFS_MAGIC,
            RAMFS_MAGIC,
            SECURITYFS_MAGIC,
            CGROUP_SUPER_MAGIC,
            CGROUP2_SUPER_MAGIC,
            DEBUGFS_MAGIC,
            BINFMTFS_MAGIC,
            PIPEFS_MAGIC,
            SOCKFS_MAGIC,
        ];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_tmpfs_magic() {
        assert_eq!(TMPFS_MAGIC, 0x01021994);
    }

    #[test]
    fn test_cgroup2_magic() {
        assert_eq!(CGROUP2_SUPER_MAGIC, 0x63677270);
    }
}
