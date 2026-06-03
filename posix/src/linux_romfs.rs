//! `<linux/romfs_fs.h>` — ROMFS (ROM File System) constants.
//!
//! ROMFS is a minimal read-only filesystem used in initramfs and
//! embedded systems where space is extremely limited. It has no
//! compression but minimal per-file overhead (just 16 bytes + name).
//! Files are packed sequentially with 16-byte alignment.

// ---------------------------------------------------------------------------
// ROMFS magic
// ---------------------------------------------------------------------------

/// ROMFS magic string ("-rom1fs-").
pub const ROMFS_MAGIC: &str = "-rom1fs-";
/// ROMFS magic as first 8 bytes (big-endian).
pub const ROMFS_MAGIC_WORD0: u32 = 0x2D72_6F6D;
/// Second word of magic.
pub const ROMFS_MAGIC_WORD1: u32 = 0x3166_732D;

// ---------------------------------------------------------------------------
// File types (stored in next_filehdr field, bits 2:0)
// ---------------------------------------------------------------------------

/// Hard link.
pub const ROMFS_TYPE_HARDLINK: u8 = 0;
/// Directory.
pub const ROMFS_TYPE_DIRECTORY: u8 = 1;
/// Regular file.
pub const ROMFS_TYPE_REGULAR: u8 = 2;
/// Symbolic link.
pub const ROMFS_TYPE_SYMLINK: u8 = 3;
/// Block device.
pub const ROMFS_TYPE_BLKDEV: u8 = 4;
/// Character device.
pub const ROMFS_TYPE_CHRDEV: u8 = 5;
/// Socket.
pub const ROMFS_TYPE_SOCKET: u8 = 6;
/// FIFO.
pub const ROMFS_TYPE_FIFO: u8 = 7;

// ---------------------------------------------------------------------------
// File header flags
// ---------------------------------------------------------------------------

/// File is executable.
pub const ROMFS_FLAG_EXEC: u8 = 1 << 3;

// ---------------------------------------------------------------------------
// Alignment and limits
// ---------------------------------------------------------------------------

/// Header alignment (16 bytes).
pub const ROMFS_ALIGN: u32 = 16;
/// Mask for next-file pointer (clears type bits).
pub const ROMFS_NEXT_MASK: u32 = 0xFFFF_FFF0;
/// Maximum filename length.
pub const ROMFS_MAX_NAME_LEN: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(ROMFS_MAGIC, "-rom1fs-");
        assert_ne!(ROMFS_MAGIC_WORD0, ROMFS_MAGIC_WORD1);
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            ROMFS_TYPE_HARDLINK,
            ROMFS_TYPE_DIRECTORY,
            ROMFS_TYPE_REGULAR,
            ROMFS_TYPE_SYMLINK,
            ROMFS_TYPE_BLKDEV,
            ROMFS_TYPE_CHRDEV,
            ROMFS_TYPE_SOCKET,
            ROMFS_TYPE_FIFO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_alignment() {
        assert_eq!(ROMFS_ALIGN, 16);
        assert!(ROMFS_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_next_mask() {
        // The mask should clear the bottom 4 bits (type + exec flag)
        assert_eq!(ROMFS_NEXT_MASK & 0xF, 0);
    }
}
