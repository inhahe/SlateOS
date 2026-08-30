//! `<linux/minix_fs.h>` — Minix filesystem on-disk constants.
//!
//! Minix is the classic teaching filesystem still supported by the
//! Linux kernel for floppy images and rescue media. These constants
//! cover superblock magic numbers (V1/V2/V3, 14- and 30-byte name
//! variants), inode/zone limits, and directory-entry sizing.

// ---------------------------------------------------------------------------
// Superblock magic numbers (struct minix_super_block.s_magic)
// ---------------------------------------------------------------------------

/// Minix V1, 14-byte filenames (original 1986 layout).
pub const MINIX_SUPER_MAGIC: u16 = 0x137f;
/// Minix V1, 30-byte filenames.
pub const MINIX_SUPER_MAGIC2: u16 = 0x138f;
/// Minix V2, 14-byte filenames.
pub const MINIX2_SUPER_MAGIC: u16 = 0x2468;
/// Minix V2, 30-byte filenames.
pub const MINIX2_SUPER_MAGIC2: u16 = 0x2478;
/// Minix V3 (block-size field added).
pub const MINIX3_SUPER_MAGIC: u16 = 0x4d5a;

// ---------------------------------------------------------------------------
// On-disk inode dimensions
// ---------------------------------------------------------------------------

/// Number of zone pointers in a V1 inode (7 direct + 1 indirect + 1
/// double-indirect).
pub const MINIX_I_MAP_SLOTS: u32 = 8;
/// Zone-pointers in a V1 inode (7 direct + 2 indirect levels).
pub const MINIX_Z_MAP_SLOTS: u32 = 64;
/// Total direct + indirect zone pointers per V1 inode.
pub const MINIX_V1_ZONES: u32 = 9;
/// Total zone pointers per V2/V3 inode (7 direct + indirect + dindirect
/// + tindirect = 10).
pub const MINIX_V2_ZONES: u32 = 10;

/// Maximum filename length in V1/V2 "short" layout.
pub const MINIX_NAME_LEN: u32 = 14;
/// Maximum filename length in V1/V2 "long" layout.
pub const MINIX_NAME_LEN_LONG: u32 = 30;
/// Maximum filename length in V3 (separate field in dir entry).
pub const MINIX3_NAME_LEN: u32 = 60;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Logical block size for V1/V2 (fixed at 1 KiB).
pub const MINIX_BLOCK_SIZE: u32 = 1024;
/// Power-of-two log of the V1/V2 block size.
pub const MINIX_BLOCK_SIZE_BITS: u32 = 10;
/// Maximum V3 block size (V3 stores this in the superblock).
pub const MINIX3_MAX_BLOCK_SIZE: u32 = 4096;

/// Valid-superblock state flag (`s_state` bit).
pub const MINIX_VALID_FS: u16 = 0x0001;
/// Errors-on-fs flag.
pub const MINIX_ERROR_FS: u16 = 0x0002;

/// First reserved-then-real inode number — inode #1 holds the
/// bad-blocks file, #2 is the root.
pub const MINIX_ROOT_INO: u32 = 1;
/// Bad-blocks inode (Linux follows BSD/minix convention).
pub const MINIX_BAD_INO: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magics_distinct() {
        let m = [
            MINIX_SUPER_MAGIC,
            MINIX_SUPER_MAGIC2,
            MINIX2_SUPER_MAGIC,
            MINIX2_SUPER_MAGIC2,
            MINIX3_SUPER_MAGIC,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_v1_vs_v2_zone_counts() {
        // V2/V3 added the triple-indirect pointer.
        assert_eq!(MINIX_V2_ZONES, MINIX_V1_ZONES + 1);
    }

    #[test]
    fn test_name_lengths_monotonic() {
        assert!(MINIX_NAME_LEN < MINIX_NAME_LEN_LONG);
        assert!(MINIX_NAME_LEN_LONG < MINIX3_NAME_LEN);
    }

    #[test]
    fn test_block_size_consistent_with_bits() {
        assert_eq!(1u32 << MINIX_BLOCK_SIZE_BITS, MINIX_BLOCK_SIZE);
        assert!(MINIX_BLOCK_SIZE.is_power_of_two());
        assert!(MINIX3_MAX_BLOCK_SIZE.is_power_of_two());
        assert!(MINIX3_MAX_BLOCK_SIZE >= MINIX_BLOCK_SIZE);
    }

    #[test]
    fn test_state_flags_distinct() {
        assert!(MINIX_VALID_FS.is_power_of_two());
        assert!(MINIX_ERROR_FS.is_power_of_two());
        assert_ne!(MINIX_VALID_FS, MINIX_ERROR_FS);
    }

    #[test]
    fn test_root_ino_nonzero() {
        // Inode 0 is reserved/unused; root/bad must be >= 1.
        assert!(MINIX_ROOT_INO >= 1);
        assert!(MINIX_BAD_INO >= 1);
    }
}
