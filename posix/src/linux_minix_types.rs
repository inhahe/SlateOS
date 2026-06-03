//! `<linux/minix_fs.h>` — Minix filesystem constants.
//!
//! The Minix filesystem is one of the original Linux filesystems.
//! These constants define magic numbers, limits, and
//! inode structure parameters.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// Minix v1 magic (14 char names).
pub const MINIX_SUPER_MAGIC: u32 = 0x137F;
/// Minix v1 magic (30 char names).
pub const MINIX_SUPER_MAGIC2: u32 = 0x138F;
/// Minix v2 magic (14 char names).
pub const MINIX2_SUPER_MAGIC: u32 = 0x2468;
/// Minix v2 magic (30 char names).
pub const MINIX2_SUPER_MAGIC2: u32 = 0x2478;
/// Minix v3 magic.
pub const MINIX3_SUPER_MAGIC: u32 = 0x4D5A;

// ---------------------------------------------------------------------------
// Inode limits
// ---------------------------------------------------------------------------

/// Minix v1 max inode links.
pub const MINIX_LINK_MAX: u32 = 250;
/// Minix v2 max inode links.
pub const MINIX2_LINK_MAX: u32 = 65530;
/// Minix v1 max file name length (14).
pub const MINIX_NAME_MAX_14: u32 = 14;
/// Minix v1 max file name length (30).
pub const MINIX_NAME_MAX_30: u32 = 30;
/// Minix v3 max file name length (60).
pub const MINIX3_NAME_MAX: u32 = 60;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Minix v1/v2 block size.
pub const MINIX_BLOCK_SIZE: u32 = 1024;
/// Minix v3 block size.
pub const MINIX3_BLOCK_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Inode sizes
// ---------------------------------------------------------------------------

/// Minix v1 inode size.
pub const MINIX_INODE_SIZE: u32 = 32;
/// Minix v2 inode size.
pub const MINIX2_INODE_SIZE: u32 = 64;
/// Minix v3 inode size.
pub const MINIX3_INODE_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Superblock offsets
// ---------------------------------------------------------------------------

/// Superblock offset (bytes from start).
pub const MINIX_SB_OFFSET: u32 = 1024;
/// Boot block size.
pub const MINIX_BOOT_BLOCK_SIZE: u32 = 1024;

// ---------------------------------------------------------------------------
// Directory entry sizes
// ---------------------------------------------------------------------------

/// Minix v1 dir entry size (14 char names).
pub const MINIX_DIRENT_SIZE_14: u32 = 16;
/// Minix v1 dir entry size (30 char names).
pub const MINIX_DIRENT_SIZE_30: u32 = 32;
/// Minix v3 dir entry size (60 char names).
pub const MINIX3_DIRENT_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Number of direct/indirect blocks
// ---------------------------------------------------------------------------

/// Direct blocks per inode (v1).
pub const MINIX_NR_DZONES: u32 = 7;
/// Total zones per inode (v1).
pub const MINIX_NR_TZONES: u32 = 9;
/// Direct blocks per inode (v2).
pub const MINIX2_NR_DZONES: u32 = 7;
/// Total zones per inode (v2).
pub const MINIX2_NR_TZONES: u32 = 10;

// ---------------------------------------------------------------------------
// Valid FS state
// ---------------------------------------------------------------------------

/// Filesystem is clean.
pub const MINIX_VALID_FS: u32 = 0x0001;
/// Filesystem has errors.
pub const MINIX_ERROR_FS: u32 = 0x0002;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_values_distinct() {
        let magics = [
            MINIX_SUPER_MAGIC,
            MINIX_SUPER_MAGIC2,
            MINIX2_SUPER_MAGIC,
            MINIX2_SUPER_MAGIC2,
            MINIX3_SUPER_MAGIC,
        ];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_name_lengths() {
        assert_eq!(MINIX_NAME_MAX_14, 14);
        assert_eq!(MINIX_NAME_MAX_30, 30);
        assert_eq!(MINIX3_NAME_MAX, 60);
    }

    #[test]
    fn test_block_sizes_power_of_two() {
        assert!(MINIX_BLOCK_SIZE.is_power_of_two());
        assert!(MINIX3_BLOCK_SIZE.is_power_of_two());
    }

    #[test]
    fn test_inode_sizes() {
        assert_eq!(MINIX_INODE_SIZE, 32);
        assert_eq!(MINIX2_INODE_SIZE, 64);
        assert_eq!(MINIX3_INODE_SIZE, 64);
    }

    #[test]
    fn test_dirent_sizes() {
        assert_eq!(MINIX_DIRENT_SIZE_14, 16);
        assert_eq!(MINIX_DIRENT_SIZE_30, 32);
        assert_eq!(MINIX3_DIRENT_SIZE, 64);
    }

    #[test]
    fn test_zones_v1() {
        assert_eq!(MINIX_NR_DZONES, 7);
        assert_eq!(MINIX_NR_TZONES, 9);
        assert!(MINIX_NR_DZONES < MINIX_NR_TZONES);
    }

    #[test]
    fn test_zones_v2() {
        assert_eq!(MINIX2_NR_DZONES, 7);
        assert_eq!(MINIX2_NR_TZONES, 10);
        assert!(MINIX2_NR_DZONES < MINIX2_NR_TZONES);
    }

    #[test]
    fn test_valid_error_flags() {
        assert_eq!(MINIX_VALID_FS, 1);
        assert_eq!(MINIX_ERROR_FS, 2);
        assert_eq!(MINIX_VALID_FS & MINIX_ERROR_FS, 0);
    }

    #[test]
    fn test_link_limits() {
        assert!(MINIX_LINK_MAX < MINIX2_LINK_MAX);
    }

    #[test]
    fn test_sb_offset() {
        assert_eq!(MINIX_SB_OFFSET, 1024);
    }
}
