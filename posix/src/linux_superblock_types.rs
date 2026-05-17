//! `<linux/fs.h>` (superblock subset) — Superblock constants.
//!
//! The superblock represents a mounted filesystem instance. It contains
//! filesystem-wide metadata: block size, total/free blocks/inodes,
//! mount flags, filesystem type, and the root dentry. Each mount point
//! has exactly one superblock. The VFS uses the superblock's operations
//! table to dispatch filesystem-specific operations (allocate inodes,
//! sync, remount, etc.).

// ---------------------------------------------------------------------------
// Superblock flags (MS_* / SB_* from mount)
// ---------------------------------------------------------------------------

/// Read-only mount.
pub const SB_RDONLY: u32 = 1 << 0;
/// Ignore setuid/setgid bits.
pub const SB_NOSUID: u32 = 1 << 1;
/// Disallow access to device special files.
pub const SB_NODEV: u32 = 1 << 2;
/// Disallow program execution.
pub const SB_NOEXEC: u32 = 1 << 3;
/// Writes are synced immediately.
pub const SB_SYNCHRONOUS: u32 = 1 << 4;
/// Mandatory locking enabled.
pub const SB_MANDLOCK: u32 = 1 << 6;
/// Directory modifications are synchronous.
pub const SB_DIRSYNC: u32 = 1 << 7;
/// Do not update access times.
pub const SB_NOATIME: u32 = 1 << 10;
/// Do not update directory access times.
pub const SB_NODIRATIME: u32 = 1 << 11;
/// Update atime relative to mtime/ctime (default since Linux 2.6.30).
pub const SB_RELATIME: u32 = 1 << 12;
/// Silent mount (suppress certain log messages).
pub const SB_SILENT: u32 = 1 << 15;
/// POSIX ACLs supported.
pub const SB_POSIXACL: u32 = 1 << 16;
/// Lazy atime updates (update only in memory, flush on sync).
pub const SB_LAZYTIME: u32 = 1 << 25;

// ---------------------------------------------------------------------------
// Superblock states
// ---------------------------------------------------------------------------

/// Superblock is active and usable.
pub const SB_STATE_ACTIVE: u32 = 0;
/// Superblock is being unmounted.
pub const SB_STATE_UNMOUNTING: u32 = 1;
/// Superblock had errors (needs fsck).
pub const SB_STATE_ERRORS: u32 = 2;

// ---------------------------------------------------------------------------
// Filesystem magic numbers (well-known)
// ---------------------------------------------------------------------------

/// ext4 magic number.
pub const EXT4_SUPER_MAGIC: u32 = 0xEF53;
/// tmpfs magic number.
pub const TMPFS_MAGIC: u32 = 0x0102_1994;
/// proc magic number.
pub const PROC_SUPER_MAGIC: u32 = 0x9FA0;
/// sysfs magic number.
pub const SYSFS_MAGIC: u32 = 0x6273_6673;
/// devtmpfs magic number.
pub const DEVTMPFS_MAGIC: u32 = 0x0102_1994;
/// btrfs magic number.
pub const BTRFS_SUPER_MAGIC: u32 = 0x9123_683E;
/// XFS magic number.
pub const XFS_SUPER_MAGIC: u32 = 0x5846_5346;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            SB_RDONLY, SB_NOSUID, SB_NODEV, SB_NOEXEC,
            SB_SYNCHRONOUS, SB_MANDLOCK, SB_DIRSYNC,
            SB_NOATIME, SB_NODIRATIME, SB_RELATIME,
            SB_SILENT, SB_POSIXACL, SB_LAZYTIME,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [SB_STATE_ACTIVE, SB_STATE_UNMOUNTING, SB_STATE_ERRORS];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_magic_numbers_nonzero() {
        assert_ne!(EXT4_SUPER_MAGIC, 0);
        assert_ne!(TMPFS_MAGIC, 0);
        assert_ne!(PROC_SUPER_MAGIC, 0);
        assert_ne!(SYSFS_MAGIC, 0);
        assert_ne!(BTRFS_SUPER_MAGIC, 0);
        assert_ne!(XFS_SUPER_MAGIC, 0);
    }
}
