//! `<linux/romfs_fs.h>` — ROM filesystem constants.
//!
//! RomFS is a simple read-only filesystem for embedded devices
//! and initramfs. These constants define magic numbers,
//! file type modes, and alignment parameters.

// ---------------------------------------------------------------------------
// Magic and version
// ---------------------------------------------------------------------------

/// RomFS magic string bytes ("-rom1fs-").
pub const ROMFS_MAGIC_BYTE0: u8 = 0x2D;
/// RomFS magic byte 1.
pub const ROMFS_MAGIC_BYTE1: u8 = 0x72;
/// RomFS magic byte 2.
pub const ROMFS_MAGIC_BYTE2: u8 = 0x6F;
/// RomFS magic byte 3.
pub const ROMFS_MAGIC_BYTE3: u8 = 0x6D;
/// RomFS super magic.
pub const ROMFS_SUPER_MAGIC: u32 = 0x7275;

// ---------------------------------------------------------------------------
// File type flags (in next field)
// ---------------------------------------------------------------------------

/// Hard link.
pub const ROMFS_TYPE_HARD_LINK: u32 = 0;
/// Directory.
pub const ROMFS_TYPE_DIRECTORY: u32 = 1;
/// Regular file.
pub const ROMFS_TYPE_REGULAR: u32 = 2;
/// Symlink.
pub const ROMFS_TYPE_SYMLINK: u32 = 3;
/// Block device.
pub const ROMFS_TYPE_BLOCK_DEV: u32 = 4;
/// Character device.
pub const ROMFS_TYPE_CHAR_DEV: u32 = 5;
/// Socket.
pub const ROMFS_TYPE_SOCKET: u32 = 6;
/// FIFO.
pub const ROMFS_TYPE_FIFO: u32 = 7;

// ---------------------------------------------------------------------------
// File header flags
// ---------------------------------------------------------------------------

/// Type mask in next field.
pub const ROMFS_TYPE_MASK: u32 = 0x07;
/// Executable flag.
pub const ROMFS_EXEC_FLAG: u32 = 0x08;

// ---------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------

/// Header alignment (16 bytes).
pub const ROMFS_ALIGN: u32 = 16;
/// Alignment mask.
pub const ROMFS_ALIGN_MASK: u32 = ROMFS_ALIGN - 1;
/// Maximum file name length.
pub const ROMFS_MAXFN: u32 = 128;

// ---------------------------------------------------------------------------
// Superblock header size
// ---------------------------------------------------------------------------

/// Minimum superblock size (magic + size + checksum + name).
pub const ROMFS_MIN_HEADER_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Checksum
// ---------------------------------------------------------------------------

/// Checksum size (bytes).
pub const ROMFS_CHECKSUM_SIZE: u32 = 4;
/// Number of longwords to checksum in header.
pub const ROMFS_CHECKSUM_WORDS: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(ROMFS_SUPER_MAGIC, 0x7275);
    }

    #[test]
    fn test_magic_bytes() {
        // "-rom1fs-" starts with '-' = 0x2D
        assert_eq!(ROMFS_MAGIC_BYTE0, b'-');
        assert_eq!(ROMFS_MAGIC_BYTE1, b'r');
        assert_eq!(ROMFS_MAGIC_BYTE2, b'o');
        assert_eq!(ROMFS_MAGIC_BYTE3, b'm');
    }

    #[test]
    fn test_file_types_sequential() {
        assert_eq!(ROMFS_TYPE_HARD_LINK, 0);
        assert_eq!(ROMFS_TYPE_DIRECTORY, 1);
        assert_eq!(ROMFS_TYPE_REGULAR, 2);
        assert_eq!(ROMFS_TYPE_SYMLINK, 3);
        assert_eq!(ROMFS_TYPE_BLOCK_DEV, 4);
        assert_eq!(ROMFS_TYPE_CHAR_DEV, 5);
        assert_eq!(ROMFS_TYPE_SOCKET, 6);
        assert_eq!(ROMFS_TYPE_FIFO, 7);
    }

    #[test]
    fn test_type_mask() {
        assert_eq!(ROMFS_TYPE_MASK, 0x07);
        assert_eq!(ROMFS_TYPE_FIFO & ROMFS_TYPE_MASK, ROMFS_TYPE_FIFO);
    }

    #[test]
    fn test_exec_flag() {
        assert_eq!(ROMFS_EXEC_FLAG, 0x08);
        assert_eq!(ROMFS_EXEC_FLAG & ROMFS_TYPE_MASK, 0);
    }

    #[test]
    fn test_alignment() {
        assert!(ROMFS_ALIGN.is_power_of_two());
        assert_eq!(ROMFS_ALIGN_MASK, 15);
    }

    #[test]
    fn test_maxfn() {
        assert_eq!(ROMFS_MAXFN, 128);
    }

    #[test]
    fn test_min_header() {
        assert_eq!(ROMFS_MIN_HEADER_SIZE, 32);
    }
}
