//! `<linux/qnx4_fs.h>` — QNX4 filesystem constants.
//!
//! QNX4 is the native filesystem of QNX 4.x RTOS.
//! These constants define magic numbers, inode flags,
//! and directory parameters.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// QNX4 superblock magic.
pub const QNX4_SUPER_MAGIC: u32 = 0x002F;
/// QNX4 root inode number.
pub const QNX4_ROOT_INO: u32 = 1;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// QNX4 block size.
pub const QNX4_BLOCK_SIZE: u32 = 512;
/// QNX4 inode size.
pub const QNX4_INODE_SIZE: u32 = 64;
/// Directory entry size.
pub const QNX4_DIR_ENTRY_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// File/inode status flags
// ---------------------------------------------------------------------------

/// File in use.
pub const QNX4_FILE_USED: u8 = 0x01;
/// File is a directory.
pub const QNX4_FILE_DIRECTORY: u8 = 0x02;
/// File is a link.
pub const QNX4_FILE_LINK: u8 = 0x08;
/// File is read-only.
pub const QNX4_FILE_READONLY: u8 = 0x40;

// ---------------------------------------------------------------------------
// Name lengths
// ---------------------------------------------------------------------------

/// Short name length.
pub const QNX4_SHORT_NAME_MAX: u32 = 16;
/// Long name length.
pub const QNX4_NAME_MAX: u32 = 48;

// ---------------------------------------------------------------------------
// Extent limits
// ---------------------------------------------------------------------------

/// Maximum number of extents per inode.
pub const QNX4_MAX_XTNTS_PER_XBLK: u32 = 60;
/// Inode extents.
pub const QNX4_I_NUM_XTNTS: u32 = 8;

// ---------------------------------------------------------------------------
// Permission bits
// ---------------------------------------------------------------------------

/// Owner read.
pub const QNX4_PERM_OREAD: u16 = 0x0100;
/// Owner write.
pub const QNX4_PERM_OWRITE: u16 = 0x0080;
/// Owner exec.
pub const QNX4_PERM_OEXEC: u16 = 0x0040;
/// Group read.
pub const QNX4_PERM_GREAD: u16 = 0x0020;
/// Group write.
pub const QNX4_PERM_GWRITE: u16 = 0x0010;
/// Group exec.
pub const QNX4_PERM_GEXEC: u16 = 0x0008;
/// World read.
pub const QNX4_PERM_WREAD: u16 = 0x0004;
/// World write.
pub const QNX4_PERM_WWRITE: u16 = 0x0002;
/// World exec.
pub const QNX4_PERM_WEXEC: u16 = 0x0001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(QNX4_SUPER_MAGIC, 0x002F);
    }

    #[test]
    fn test_root_inode() {
        assert_eq!(QNX4_ROOT_INO, 1);
    }

    #[test]
    fn test_block_size() {
        assert_eq!(QNX4_BLOCK_SIZE, 512);
        assert!(QNX4_BLOCK_SIZE.is_power_of_two());
    }

    #[test]
    fn test_file_flags_distinct() {
        let flags = [
            QNX4_FILE_USED, QNX4_FILE_DIRECTORY,
            QNX4_FILE_LINK, QNX4_FILE_READONLY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_name_lengths() {
        assert_eq!(QNX4_SHORT_NAME_MAX, 16);
        assert_eq!(QNX4_NAME_MAX, 48);
        assert!(QNX4_SHORT_NAME_MAX < QNX4_NAME_MAX);
    }

    #[test]
    fn test_perms_power_of_two() {
        let perms: [u16; 9] = [
            QNX4_PERM_OREAD, QNX4_PERM_OWRITE, QNX4_PERM_OEXEC,
            QNX4_PERM_GREAD, QNX4_PERM_GWRITE, QNX4_PERM_GEXEC,
            QNX4_PERM_WREAD, QNX4_PERM_WWRITE, QNX4_PERM_WEXEC,
        ];
        for p in &perms {
            assert!(p.is_power_of_two(), "0x{:04x} not power of two", p);
        }
    }

    #[test]
    fn test_perms_distinct() {
        let perms: [u16; 9] = [
            QNX4_PERM_OREAD, QNX4_PERM_OWRITE, QNX4_PERM_OEXEC,
            QNX4_PERM_GREAD, QNX4_PERM_GWRITE, QNX4_PERM_GEXEC,
            QNX4_PERM_WREAD, QNX4_PERM_WWRITE, QNX4_PERM_WEXEC,
        ];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_ne!(perms[i], perms[j]);
            }
        }
    }

    #[test]
    fn test_extent_limits() {
        assert_eq!(QNX4_MAX_XTNTS_PER_XBLK, 60);
        assert_eq!(QNX4_I_NUM_XTNTS, 8);
    }
}
