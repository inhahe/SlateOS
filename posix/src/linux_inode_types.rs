//! `<linux/fs.h>` (inode subset) — Inode constants.
//!
//! An inode represents a filesystem object (file, directory, symlink,
//! device node, socket, FIFO). It contains the object's metadata
//! (ownership, permissions, timestamps, size) and pointers to data
//! blocks. Multiple dentries (hard links) can point to the same inode.
//! The VFS inode is an in-memory representation; the on-disk format
//! is filesystem-specific (ext4_inode, btrfs_inode_item, etc.).

// ---------------------------------------------------------------------------
// Inode mode type bits (S_IFMT mask = 0o170000)
// ---------------------------------------------------------------------------

/// Mask for file type bits.
pub const S_IFMT: u32 = 0o170000;
/// Socket.
pub const S_IFSOCK: u32 = 0o140000;
/// Symbolic link.
pub const S_IFLNK: u32 = 0o120000;
/// Regular file.
pub const S_IFREG: u32 = 0o100000;
/// Block device.
pub const S_IFBLK: u32 = 0o060000;
/// Directory.
pub const S_IFDIR: u32 = 0o040000;
/// Character device.
pub const S_IFCHR: u32 = 0o020000;
/// FIFO (named pipe).
pub const S_IFIFO: u32 = 0o010000;

// ---------------------------------------------------------------------------
// Inode permission bits
// ---------------------------------------------------------------------------

/// Set-user-ID on execution.
pub const S_ISUID: u32 = 0o4000;
/// Set-group-ID on execution.
pub const S_ISGID: u32 = 0o2000;
/// Sticky bit (restricted deletion).
pub const S_ISVTX: u32 = 0o1000;
/// Owner read.
pub const S_IRUSR: u32 = 0o0400;
/// Owner write.
pub const S_IWUSR: u32 = 0o0200;
/// Owner execute.
pub const S_IXUSR: u32 = 0o0100;
/// Group read.
pub const S_IRGRP: u32 = 0o0040;
/// Group write.
pub const S_IWGRP: u32 = 0o0020;
/// Group execute.
pub const S_IXGRP: u32 = 0o0010;
/// Others read.
pub const S_IROTH: u32 = 0o0004;
/// Others write.
pub const S_IWOTH: u32 = 0o0002;
/// Others execute.
pub const S_IXOTH: u32 = 0o0001;

// ---------------------------------------------------------------------------
// Inode flags (FS_*_FL, via ioctl FS_IOC_GETFLAGS)
// ---------------------------------------------------------------------------

/// Secure deletion (not implemented by most FS).
pub const FS_SECRM_FL: u32 = 0x0000_0001;
/// Undelete (not implemented).
pub const FS_UNRM_FL: u32 = 0x0000_0002;
/// Compress file.
pub const FS_COMPR_FL: u32 = 0x0000_0004;
/// Synchronous updates.
pub const FS_SYNC_FL: u32 = 0x0000_0008;
/// Immutable file (cannot be modified, deleted, or renamed).
pub const FS_IMMUTABLE_FL: u32 = 0x0000_0010;
/// Append only.
pub const FS_APPEND_FL: u32 = 0x0000_0020;
/// Don't include in dump.
pub const FS_NODUMP_FL: u32 = 0x0000_0040;
/// Don't update atime.
pub const FS_NOATIME_FL: u32 = 0x0000_0080;
/// Encrypted file.
pub const FS_ENCRYPT_FL: u32 = 0x0000_0800;
/// Verity-protected file.
pub const FS_VERITY_FL: u32 = 0x0010_0000;
/// Case-insensitive directory.
pub const FS_CASEFOLD_FL: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_types_distinct() {
        let types = [
            S_IFSOCK, S_IFLNK, S_IFREG, S_IFBLK,
            S_IFDIR, S_IFCHR, S_IFIFO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_file_types_within_mask() {
        let types = [S_IFSOCK, S_IFLNK, S_IFREG, S_IFBLK, S_IFDIR, S_IFCHR, S_IFIFO];
        for t in &types {
            assert_eq!(*t & !S_IFMT, 0, "type bits should be within S_IFMT");
        }
    }

    #[test]
    fn test_permission_bits() {
        // rwx for user, group, others are in distinct bit positions
        let perms = [
            S_IRUSR, S_IWUSR, S_IXUSR,
            S_IRGRP, S_IWGRP, S_IXGRP,
            S_IROTH, S_IWOTH, S_IXOTH,
        ];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_inode_flags_power_of_two() {
        let flags = [
            FS_SECRM_FL, FS_UNRM_FL, FS_COMPR_FL, FS_SYNC_FL,
            FS_IMMUTABLE_FL, FS_APPEND_FL, FS_NODUMP_FL,
            FS_NOATIME_FL, FS_ENCRYPT_FL, FS_VERITY_FL, FS_CASEFOLD_FL,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }
}
