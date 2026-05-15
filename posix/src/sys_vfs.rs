//! `<sys/vfs.h>` — virtual filesystem information (Linux extension).
//!
//! Provides the `Statfs` structure and `statfs`/`fstatfs` stubs for
//! querying filesystem statistics.  This is the Linux-specific
//! interface; POSIX specifies `<sys/statvfs.h>` instead.

use crate::errno;
use crate::types::{Fd, SizeT};

// ---------------------------------------------------------------------------
// Filesystem magic numbers
// ---------------------------------------------------------------------------

/// ext2/ext3/ext4.
pub const EXT2_SUPER_MAGIC: i64 = 0xEF53;

/// tmpfs.
pub const TMPFS_MAGIC: i64 = 0x01021994;

/// proc filesystem.
pub const PROC_SUPER_MAGIC: i64 = 0x9FA0;

/// sysfs.
pub const SYSFS_MAGIC: i64 = 0x62656572;

/// devtmpfs / devfs.
pub const DEVTMPFS_MAGIC: i64 = 0x1373;

/// NFS.
pub const NFS_SUPER_MAGIC: i64 = 0x6969;

/// Btrfs.
pub const BTRFS_SUPER_MAGIC: i64 = 0x9123683E;

/// XFS.
pub const XFS_SUPER_MAGIC: i64 = 0x58465342;

/// FAT (MSDOS).
pub const MSDOS_SUPER_MAGIC: i64 = 0x4D44;

/// ISO 9660 (CDROM).
pub const ISOFS_SUPER_MAGIC: i64 = 0x9660;

// ---------------------------------------------------------------------------
// Statfs structure
// ---------------------------------------------------------------------------

/// Filesystem statistics (Linux `statfs`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Statfs {
    /// Filesystem type magic number.
    pub f_type: i64,
    /// Optimal transfer block size.
    pub f_bsize: i64,
    /// Total data blocks.
    pub f_blocks: u64,
    /// Free blocks.
    pub f_bfree: u64,
    /// Free blocks available to unprivileged user.
    pub f_bavail: u64,
    /// Total file nodes (inodes).
    pub f_files: u64,
    /// Free file nodes.
    pub f_ffree: u64,
    /// Filesystem ID.
    pub f_fsid: [i32; 2],
    /// Maximum length of filenames.
    pub f_namelen: i64,
    /// Fragment size.
    pub f_frsize: i64,
    /// Mount flags.
    pub f_flags: i64,
    /// Padding.
    _spare: [i64; 4],
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Get filesystem statistics by path.
///
/// Stub — always returns -1 with `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statfs(_path: *const u8, _buf: *mut Statfs) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get filesystem statistics by file descriptor.
///
/// Stub — always returns -1 with `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatfs(_fd: Fd, _buf: *mut Statfs) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statfs_struct_size() {
        // 7 i64/u64 fields (56) + 2 i32 fsid (8) + 3 i64 (24) + 4 i64 spare (32) = 120
        assert!(core::mem::size_of::<Statfs>() > 100);
    }

    #[test]
    fn test_magic_numbers_distinct() {
        let magic = [
            EXT2_SUPER_MAGIC, TMPFS_MAGIC, PROC_SUPER_MAGIC,
            SYSFS_MAGIC, DEVTMPFS_MAGIC, NFS_SUPER_MAGIC,
            BTRFS_SUPER_MAGIC, XFS_SUPER_MAGIC,
            MSDOS_SUPER_MAGIC, ISOFS_SUPER_MAGIC,
        ];
        for i in 0..magic.len() {
            for j in (i + 1)..magic.len() {
                assert_ne!(magic[i], magic[j], "magic numbers must be distinct");
            }
        }
    }

    #[test]
    fn test_ext2_magic() {
        assert_eq!(EXT2_SUPER_MAGIC, 0xEF53);
    }

    #[test]
    fn test_statfs_stub() {
        let ret = statfs(b"/\0".as_ptr(), core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_fstatfs_stub() {
        let ret = fstatfs(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_statfs_zeroed() {
        // SAFETY: Statfs is repr(C) with all-numeric fields.
        let st: Statfs = unsafe { core::mem::zeroed() };
        assert_eq!(st.f_type, 0);
        assert_eq!(st.f_bsize, 0);
        assert_eq!(st.f_blocks, 0);
    }
}
