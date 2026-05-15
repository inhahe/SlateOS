//! `<linux/magic.h>` — filesystem magic numbers.
//!
//! Provides magic numbers for all common Linux filesystem types.
//! These can be compared against the `f_type` field of `Statfs`
//! to identify the filesystem type.

// Re-export the ones already defined in sys_vfs.
pub use crate::sys_vfs::EXT2_SUPER_MAGIC;
pub use crate::sys_vfs::TMPFS_MAGIC;
pub use crate::sys_vfs::PROC_SUPER_MAGIC;
pub use crate::sys_vfs::SYSFS_MAGIC;
pub use crate::sys_vfs::DEVTMPFS_MAGIC;
pub use crate::sys_vfs::NFS_SUPER_MAGIC;
pub use crate::sys_vfs::BTRFS_SUPER_MAGIC;
pub use crate::sys_vfs::XFS_SUPER_MAGIC;
pub use crate::sys_vfs::MSDOS_SUPER_MAGIC;
pub use crate::sys_vfs::ISOFS_SUPER_MAGIC;

// ---------------------------------------------------------------------------
// Additional magic numbers
// ---------------------------------------------------------------------------

/// ext4 uses the same magic as ext2.
pub const EXT4_SUPER_MAGIC: i64 = EXT2_SUPER_MAGIC;

/// squashfs.
pub const SQUASHFS_MAGIC: i64 = 0x73717368;

/// FUSE.
pub const FUSE_SUPER_MAGIC: i64 = 0x65735546;

/// overlayfs.
pub const OVERLAYFS_SUPER_MAGIC: i64 = 0x794C7630;

/// cgroup filesystem.
pub const CGROUP_SUPER_MAGIC: i64 = 0x27E0EB;

/// cgroup2 filesystem.
pub const CGROUP2_SUPER_MAGIC: i64 = 0x63677270;

/// debugfs.
pub const DEBUGFS_MAGIC: i64 = 0x64626720;

/// tracefs.
pub const TRACEFS_MAGIC: i64 = 0x74726163;

/// securityfs.
pub const SECURITYFS_MAGIC: i64 = 0x73636673;

/// sockfs (internal).
pub const SOCKFS_MAGIC: i64 = 0x534F434B;

/// pipefs (internal).
pub const PIPEFS_MAGIC: i64 = 0x50495045;

/// devpts.
pub const DEVPTS_SUPER_MAGIC: i64 = 0x1CD1;

/// hugetlbfs.
pub const HUGETLBFS_MAGIC: i64 = 0x958458F6;

/// ramfs.
pub const RAMFS_MAGIC: i64 = 0x858458F6;

/// autofs.
pub const AUTOFS_SUPER_MAGIC: i64 = 0x0187;

/// pstore.
pub const PSTOREFS_MAGIC: i64 = 0x6165676C;

/// efivarfs.
pub const EFIVARFS_MAGIC: i64 = 0xDE5E81E4;

/// BPF filesystem.
pub const BPF_FS_MAGIC: i64 = 0xCAFE4A11;

/// NTFS.
pub const NTFS_SB_MAGIC: i64 = 0x5346544E;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ext4_equals_ext2() {
        assert_eq!(EXT4_SUPER_MAGIC, EXT2_SUPER_MAGIC);
    }

    #[test]
    fn test_new_magics_distinct() {
        let magic = [
            SQUASHFS_MAGIC, FUSE_SUPER_MAGIC, OVERLAYFS_SUPER_MAGIC,
            CGROUP_SUPER_MAGIC, CGROUP2_SUPER_MAGIC,
            DEBUGFS_MAGIC, TRACEFS_MAGIC, SECURITYFS_MAGIC,
            SOCKFS_MAGIC, PIPEFS_MAGIC, DEVPTS_SUPER_MAGIC,
            HUGETLBFS_MAGIC, RAMFS_MAGIC, AUTOFS_SUPER_MAGIC,
            PSTOREFS_MAGIC, EFIVARFS_MAGIC, BPF_FS_MAGIC,
            NTFS_SB_MAGIC,
        ];
        for i in 0..magic.len() {
            for j in (i + 1)..magic.len() {
                assert_ne!(magic[i], magic[j], "magic numbers must be distinct");
            }
        }
    }

    #[test]
    fn test_all_nonzero() {
        let magic = [
            EXT2_SUPER_MAGIC, TMPFS_MAGIC, PROC_SUPER_MAGIC,
            SYSFS_MAGIC, NFS_SUPER_MAGIC, BTRFS_SUPER_MAGIC,
            XFS_SUPER_MAGIC, SQUASHFS_MAGIC, FUSE_SUPER_MAGIC,
        ];
        for &m in &magic {
            assert_ne!(m, 0, "magic number should not be zero");
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(EXT2_SUPER_MAGIC, crate::sys_vfs::EXT2_SUPER_MAGIC);
        assert_eq!(TMPFS_MAGIC, crate::sys_vfs::TMPFS_MAGIC);
    }
}
