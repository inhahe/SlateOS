//! `<sys/statvfs.h>` — Filesystem statistics constants.
//!
//! `statvfs()` and `fstatvfs()` return information about a
//! mounted filesystem.  These constants define the flag bits
//! and field offsets for `struct statvfs`.

// ---------------------------------------------------------------------------
// statvfs flag bits (f_flag field)
// ---------------------------------------------------------------------------

/// Mount is read-only.
pub const ST_RDONLY: u32 = 1;
/// Set-UID/set-GID bits are ignored.
pub const ST_NOSUID: u32 = 2;
/// Do not allow device access.
pub const ST_NODEV: u32 = 4;
/// Do not allow program execution.
pub const ST_NOEXEC: u32 = 8;
/// Writes are synced immediately.
pub const ST_SYNCHRONOUS: u32 = 16;
/// Mandatory locking is permitted.
pub const ST_MANDLOCK: u32 = 64;
/// Do not update access times.
pub const ST_NOATIME: u32 = 1024;
/// Do not update directory access times.
pub const ST_NODIRATIME: u32 = 2048;
/// Update atime relative to mtime/ctime.
pub const ST_RELATIME: u32 = 4096;
/// Do not follow symlinks.
pub const ST_NOSYMFOLLOW: u32 = 8192;

// ---------------------------------------------------------------------------
// struct statvfs field offsets (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of f_bsize (block size) in struct statvfs.
pub const STATVFS_OFF_BSIZE: u32 = 0;
/// Offset of f_frsize (fragment size) in struct statvfs.
pub const STATVFS_OFF_FRSIZE: u32 = 8;
/// Offset of f_blocks (total blocks) in struct statvfs.
pub const STATVFS_OFF_BLOCKS: u32 = 16;
/// Offset of f_bfree (free blocks) in struct statvfs.
pub const STATVFS_OFF_BFREE: u32 = 24;
/// Offset of f_bavail (available blocks for unprivileged users) in struct statvfs.
pub const STATVFS_OFF_BAVAIL: u32 = 32;
/// Offset of f_files (total inodes) in struct statvfs.
pub const STATVFS_OFF_FILES: u32 = 40;
/// Offset of f_ffree (free inodes) in struct statvfs.
pub const STATVFS_OFF_FFREE: u32 = 48;
/// Offset of f_favail (available inodes for unprivileged users) in struct statvfs.
pub const STATVFS_OFF_FAVAIL: u32 = 56;
/// Offset of f_fsid (filesystem ID) in struct statvfs.
pub const STATVFS_OFF_FSID: u32 = 64;
/// Offset of f_flag (mount flags) in struct statvfs.
pub const STATVFS_OFF_FLAG: u32 = 72;
/// Offset of f_namemax (max filename length) in struct statvfs.
pub const STATVFS_OFF_NAMEMAX: u32 = 80;

/// Size of struct statvfs on Linux x86_64 (bytes).
pub const STATVFS_SIZE: u32 = 112;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct() {
        let flags = [
            ST_RDONLY, ST_NOSUID, ST_NODEV, ST_NOEXEC,
            ST_SYNCHRONOUS, ST_MANDLOCK, ST_NOATIME,
            ST_NODIRATIME, ST_RELATIME, ST_NOSYMFOLLOW,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_rdonly_is_one() {
        assert_eq!(ST_RDONLY, 1);
    }

    #[test]
    fn test_nosuid_is_two() {
        assert_eq!(ST_NOSUID, 2);
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            STATVFS_OFF_BSIZE, STATVFS_OFF_FRSIZE, STATVFS_OFF_BLOCKS,
            STATVFS_OFF_BFREE, STATVFS_OFF_BAVAIL, STATVFS_OFF_FILES,
            STATVFS_OFF_FFREE, STATVFS_OFF_FAVAIL, STATVFS_OFF_FSID,
            STATVFS_OFF_FLAG, STATVFS_OFF_NAMEMAX,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(STATVFS_OFF_NAMEMAX < STATVFS_SIZE);
    }

    #[test]
    fn test_struct_size() {
        assert_eq!(STATVFS_SIZE, 112);
    }

    #[test]
    fn test_bsize_at_start() {
        assert_eq!(STATVFS_OFF_BSIZE, 0);
    }
}
