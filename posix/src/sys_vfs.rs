//! `<sys/vfs.h>` — virtual filesystem information (Linux extension).
//!
//! Re-exports `Statfs`, `statfs`, and `fstatfs` from the `statvfs`
//! module and adds filesystem magic number constants.  This is the
//! Linux-specific interface; POSIX specifies `<sys/statvfs.h>`.

// ---------------------------------------------------------------------------
// Re-exports from statvfs
// ---------------------------------------------------------------------------

pub use crate::statvfs::Statfs;
pub use crate::statvfs::statfs;
pub use crate::statvfs::fstatfs;

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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statfs_struct_size() {
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
    fn test_cross_module() {
        assert_eq!(
            core::mem::size_of::<Statfs>(),
            core::mem::size_of::<crate::statvfs::Statfs>()
        );
    }
}
