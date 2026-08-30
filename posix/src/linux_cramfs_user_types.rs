//! `<linux/cramfs_fs.h>` — Compressed ROM filesystem on-disk format.
//!
//! cramfs is a read-only zlib-compressed filesystem historically used
//! for initramfs and embedded firmware images. The superblock is fixed
//! at 64 bytes and the maximum image size is 256 MiB.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// "Cram" little-endian → 0x28cd3d45 (kernel CRAMFS_MAGIC).
pub const CRAMFS_MAGIC: u32 = 0x28cd_3d45;
/// Misuse magic — same as above with bytes swapped (BE machine).
pub const CRAMFS_MAGIC_WEND: u32 = 0x453d_cd28;
/// Signature string in superblock.
pub const CRAMFS_SIGNATURE: &[u8; 16] = b"Compressed ROMFS";

// ---------------------------------------------------------------------------
// Layout sizes (bytes)
// ---------------------------------------------------------------------------

/// Superblock size (struct cramfs_super).
pub const CRAMFS_SUPER_SIZE: usize = 64;
/// On-disk inode size (struct cramfs_inode).
pub const CRAMFS_INODE_SIZE: usize = 12;
/// Block size for the compressed payload chunks.
pub const CRAMFS_BLOCK_SIZE: usize = 4096;
/// Maximum image size — 256 MiB.
pub const CRAMFS_MAXSIZE: u64 = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// flags field (superblock + inode)
// ---------------------------------------------------------------------------

pub const CRAMFS_FLAG_FSID_VERSION_2: u32 = 0x0000_0001;
pub const CRAMFS_FLAG_SORTED_DIRS: u32 = 0x0000_0002;
pub const CRAMFS_FLAG_HOLES: u32 = 0x0000_0100;
pub const CRAMFS_FLAG_WRONG_SIGNATURE: u32 = 0x0000_0200;
pub const CRAMFS_FLAG_SHIFTED_ROOT_OFFSET: u32 = 0x0000_0400;
pub const CRAMFS_FLAG_EXT_BLOCK_POINTERS: u32 = 0x0000_0800;

/// Maximum legal value of flags (bitwise OR of the known bits).
pub const CRAMFS_SUPPORTED_FLAGS: u32 = 0x0000_0FFF;

// ---------------------------------------------------------------------------
// Inode mode field widths
// ---------------------------------------------------------------------------

/// Width (bits) of namelen in cramfs_inode.
pub const CRAMFS_NAMELEN_WIDTH: u32 = 6;
/// Maximum filename length (in 4-byte chunks: 63 * 4 = 252).
pub const CRAMFS_MAXNAMELEN: usize = ((1usize << CRAMFS_NAMELEN_WIDTH) - 1) * 4;
/// Width (bits) of offset.
pub const CRAMFS_OFFSET_WIDTH: u32 = 26;
/// Maximum offset (in 4-byte chunks).
pub const CRAMFS_MAXOFFSET: u64 = ((1u64 << CRAMFS_OFFSET_WIDTH) - 1) * 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_is_well_known_value() {
        assert_eq!(CRAMFS_MAGIC, 0x28cd_3d45);
        assert_eq!(CRAMFS_MAGIC_WEND.swap_bytes(), CRAMFS_MAGIC);
    }

    #[test]
    fn test_signature_is_compressed_romfs() {
        assert_eq!(CRAMFS_SIGNATURE.len(), 16);
        assert_eq!(CRAMFS_SIGNATURE, b"Compressed ROMFS");
    }

    #[test]
    fn test_layout_sizes_match_spec() {
        assert_eq!(CRAMFS_SUPER_SIZE, 64);
        assert_eq!(CRAMFS_INODE_SIZE, 12);
        assert_eq!(CRAMFS_BLOCK_SIZE, 4096);
        // Inode and super both fit in a single block.
        assert!(CRAMFS_SUPER_SIZE + CRAMFS_INODE_SIZE < CRAMFS_BLOCK_SIZE);
    }

    #[test]
    fn test_maxsize_256mib() {
        assert_eq!(CRAMFS_MAXSIZE, 256 * 1024 * 1024);
        assert!(CRAMFS_MAXSIZE.is_power_of_two());
    }

    #[test]
    fn test_flag_bits_single() {
        let f = [
            CRAMFS_FLAG_FSID_VERSION_2,
            CRAMFS_FLAG_SORTED_DIRS,
            CRAMFS_FLAG_HOLES,
            CRAMFS_FLAG_WRONG_SIGNATURE,
            CRAMFS_FLAG_SHIFTED_ROOT_OFFSET,
            CRAMFS_FLAG_EXT_BLOCK_POINTERS,
        ];
        for &x in &f {
            assert!(x.is_power_of_two());
            // Every known flag is within the supported mask.
            assert_eq!(x & CRAMFS_SUPPORTED_FLAGS, x);
        }
    }

    #[test]
    fn test_namelen_and_offset_widths() {
        assert_eq!(CRAMFS_NAMELEN_WIDTH, 6);
        assert_eq!(CRAMFS_OFFSET_WIDTH, 26);
        // The two fields together fit in 32 bits (with mode and gid/uid).
        assert!(CRAMFS_NAMELEN_WIDTH + CRAMFS_OFFSET_WIDTH <= 32);
    }

    #[test]
    fn test_maxnamelen_252() {
        // 63 * 4 == 252; this is the practical filename cap.
        assert_eq!(CRAMFS_MAXNAMELEN, 252);
    }

    #[test]
    fn test_maxoffset_just_under_maxsize() {
        // Offset field is 26 bits scaled by 4 → ((2^26 - 1) * 4) = MAXSIZE - 4.
        // i.e., the on-disk offset width is exactly tuned to address the
        // entire 256 MiB image minus one 4-byte chunk.
        assert_eq!(CRAMFS_MAXOFFSET, CRAMFS_MAXSIZE - 4);
    }
}
