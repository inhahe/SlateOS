//! `<linux/magic.h>` — Filesystem magic number constants.
//!
//! Every filesystem type has a unique magic number stored in the
//! superblock. These are returned by `statfs()` in the `f_type`
//! field to identify the filesystem type of a mounted volume.

// ---------------------------------------------------------------------------
// Common filesystem magic numbers
// ---------------------------------------------------------------------------

/// ext2/ext3/ext4 filesystem.
pub const EXT4_SUPER_MAGIC: u64 = 0xEF53;
/// XFS filesystem.
pub const XFS_SUPER_MAGIC: u64 = 0x5846_5342;
/// Btrfs filesystem.
pub const BTRFS_SUPER_MAGIC: u64 = 0x9123_683E;
/// FAT (VFAT) filesystem.
pub const MSDOS_SUPER_MAGIC: u64 = 0x4D44;
/// NTFS filesystem.
pub const NTFS_SUPER_MAGIC: u64 = 0x5346_544E;
/// NFS filesystem.
pub const NFS_SUPER_MAGIC: u64 = 0x6969;
/// CIFS/SMB filesystem.
pub const CIFS_SUPER_MAGIC: u64 = 0xFF53_4D42;
/// ISO 9660 (CD-ROM) filesystem.
pub const ISOFS_SUPER_MAGIC: u64 = 0x9660;
/// SquashFS filesystem.
pub const SQUASHFS_SUPER_MAGIC: u64 = 0x7371_7368;
/// FUSE filesystem.
pub const FUSE_SUPER_MAGIC: u64 = 0x6555_4346;
/// OverlayFS.
pub const OVERLAYFS_SUPER_MAGIC: u64 = 0x794C_7630;
/// F2FS filesystem.
pub const F2FS_SUPER_MAGIC: u64 = 0xF2F5_2010;
/// ZFS filesystem.
pub const ZFS_SUPER_MAGIC: u64 = 0x2FC1_2FC1;

// ---------------------------------------------------------------------------
// Virtual/pseudo filesystem magic numbers
// ---------------------------------------------------------------------------

/// procfs.
pub const PROC_SUPER_MAGIC: u64 = 0x9FA0;
/// sysfs.
pub const SYSFS_MAGIC: u64 = 0x6279_6465;
/// devpts.
pub const DEVPTS_SUPER_MAGIC: u64 = 0x1CD1;
/// cgroup v2.
pub const CGROUP2_SUPER_MAGIC: u64 = 0x6367_7270;
/// cgroup v1.
pub const CGROUP_SUPER_MAGIC: u64 = 0x0027_E0EB;
/// BPF filesystem.
pub const BPF_FS_MAGIC: u64 = 0xCAFE_4A11;
/// Security filesystem.
pub const SECURITYFS_MAGIC: u64 = 0x7365_6366;
/// Pipe filesystem.
pub const PIPEFS_MAGIC: u64 = 0x5049_5045;
/// Socket filesystem.
pub const SOCKFS_MAGIC: u64 = 0x534F_434B;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_magics_distinct() {
        let magics = [
            EXT4_SUPER_MAGIC,
            XFS_SUPER_MAGIC,
            BTRFS_SUPER_MAGIC,
            MSDOS_SUPER_MAGIC,
            NTFS_SUPER_MAGIC,
            NFS_SUPER_MAGIC,
            CIFS_SUPER_MAGIC,
            ISOFS_SUPER_MAGIC,
            SQUASHFS_SUPER_MAGIC,
            FUSE_SUPER_MAGIC,
            OVERLAYFS_SUPER_MAGIC,
            F2FS_SUPER_MAGIC,
            ZFS_SUPER_MAGIC,
        ];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_virtual_magics_distinct() {
        let magics = [
            PROC_SUPER_MAGIC,
            SYSFS_MAGIC,
            DEVPTS_SUPER_MAGIC,
            CGROUP2_SUPER_MAGIC,
            CGROUP_SUPER_MAGIC,
            BPF_FS_MAGIC,
            SECURITYFS_MAGIC,
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
    fn test_ext4_magic() {
        assert_eq!(EXT4_SUPER_MAGIC, 0xEF53);
    }

    #[test]
    fn test_proc_magic() {
        assert_eq!(PROC_SUPER_MAGIC, 0x9FA0);
    }
}
