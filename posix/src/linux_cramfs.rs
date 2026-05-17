//! `<linux/cramfs_fs.h>` — cramfs (Compressed ROM File System) constants.
//!
//! cramfs is a simple, space-efficient, read-only filesystem designed
//! for embedded devices with limited flash/ROM. It uses zlib compression
//! on a per-page basis and has minimal metadata overhead. Files are
//! limited to 16 MiB and the filesystem to 256 MiB.

// ---------------------------------------------------------------------------
// cramfs magic and version
// ---------------------------------------------------------------------------

/// cramfs magic number.
pub const CRAMFS_MAGIC: u32 = 0x28CD_3D45;
/// cramfs signature ("Compressed ROMFS").
pub const CRAMFS_SIGNATURE: &str = "Compressed ROMFS";

// ---------------------------------------------------------------------------
// Superblock flags
// ---------------------------------------------------------------------------

/// FSID version 2 (with time/machine fields).
pub const CRAMFS_FLAG_FSID_VERSION_2: u32 = 1 << 0;
/// Sorted directory entries.
pub const CRAMFS_FLAG_SORTED_DIRS: u32 = 1 << 1;
/// Holes allowed (sparse files).
pub const CRAMFS_FLAG_HOLES: u32 = 1 << 8;
/// Wrong endianness (need byte-swap).
pub const CRAMFS_FLAG_WRONG_SIGNATURE: u32 = 1 << 9;
/// Shifted root offset.
pub const CRAMFS_FLAG_SHIFTED_ROOT_OFFSET: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// Inode mode bits (file type in upper 4 bits of namelen field)
// ---------------------------------------------------------------------------

/// Regular file.
pub const CRAMFS_MODE_REG: u16 = 0o100000;
/// Directory.
pub const CRAMFS_MODE_DIR: u16 = 0o040000;
/// Symbolic link.
pub const CRAMFS_MODE_LNK: u16 = 0o120000;
/// Block device.
pub const CRAMFS_MODE_BLK: u16 = 0o060000;
/// Character device.
pub const CRAMFS_MODE_CHR: u16 = 0o020000;
/// FIFO.
pub const CRAMFS_MODE_FIFO: u16 = 0o010000;
/// Socket.
pub const CRAMFS_MODE_SOCK: u16 = 0o140000;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum file size (16 MiB).
pub const CRAMFS_MAX_FILE_SIZE: u32 = 16 * 1024 * 1024;
/// Maximum filesystem size (256 MiB).
pub const CRAMFS_MAX_FS_SIZE: u32 = 256 * 1024 * 1024;
/// Block size (page size, 4096).
pub const CRAMFS_BLOCK_SIZE: u32 = 4096;
/// Maximum filename length.
pub const CRAMFS_MAX_NAME_LEN: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(CRAMFS_MAGIC, 0x28CD_3D45);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CRAMFS_FLAG_FSID_VERSION_2, CRAMFS_FLAG_SORTED_DIRS,
            CRAMFS_FLAG_HOLES, CRAMFS_FLAG_WRONG_SIGNATURE,
            CRAMFS_FLAG_SHIFTED_ROOT_OFFSET,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            CRAMFS_MODE_REG, CRAMFS_MODE_DIR, CRAMFS_MODE_LNK,
            CRAMFS_MODE_BLK, CRAMFS_MODE_CHR, CRAMFS_MODE_FIFO,
            CRAMFS_MODE_SOCK,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert_eq!(CRAMFS_MAX_FILE_SIZE, 16 * 1024 * 1024);
        assert!(CRAMFS_MAX_FILE_SIZE < CRAMFS_MAX_FS_SIZE);
        assert_eq!(CRAMFS_BLOCK_SIZE, 4096);
    }
}
