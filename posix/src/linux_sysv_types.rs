//! `<linux/sysv_fs.h>` — System V filesystem constants.
//!
//! The System V filesystem was used in AT&T Unix System V.
//! These constants define magic numbers, block sizes,
//! and inode parameters for SystemV/Coherent/Xenix variants.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// System V FS magic (big-endian).
pub const SYSV_MAGIC: u32 = 0xFD187E20;
/// Coherent FS magic.
pub const COH_MAGIC: u32 = 0x012FF7B7;
/// Xenix FS magic.
pub const XENIX_MAGIC: u32 = 0x012FF7B4;

// ---------------------------------------------------------------------------
// Filesystem types
// ---------------------------------------------------------------------------

/// System V Release 4.
pub const FSTYPE_SYSV4: u32 = 1;
/// System V Release 2.
pub const FSTYPE_SYSV2: u32 = 2;
/// Coherent.
pub const FSTYPE_COH: u32 = 3;
/// Xenix v2.
pub const FSTYPE_XENIX: u32 = 4;
/// AFS/EAFS.
pub const FSTYPE_AFS: u32 = 5;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// System V block size (1 KiB).
pub const SYSV_BLOCK_SIZE: u32 = 1024;
/// Xenix block size (1 KiB).
pub const XENIX_BLOCK_SIZE: u32 = 1024;
/// Coherent block size (512 bytes).
pub const COH_BLOCK_SIZE: u32 = 512;

// ---------------------------------------------------------------------------
// Inode parameters
// ---------------------------------------------------------------------------

/// System V inode size.
pub const SYSV_INODE_SIZE: u32 = 64;
/// Maximum file name length.
pub const SYSV_NAMELEN: u32 = 14;
/// Max links per inode.
pub const SYSV_LINK_MAX: u32 = 1000;

// ---------------------------------------------------------------------------
// Superblock parameters
// ---------------------------------------------------------------------------

/// Number of blocks for superblock.
pub const SYSV_SB_FSIZE: u32 = 1;
/// Inode list block offset.
pub const SYSV_SB_INODE_OFFSET: u32 = 2;
/// Root inode number.
pub const SYSV_ROOT_INO: u32 = 2;
/// First data zone.
pub const SYSV_FIRST_DATA_ZONE: u32 = 2;

// ---------------------------------------------------------------------------
// Free list sizes
// ---------------------------------------------------------------------------

/// Number of free block entries in superblock.
pub const SYSV_NICFREE: u32 = 50;
/// Number of free inode entries in superblock.
pub const SYSV_NICINOD: u32 = 100;

// ---------------------------------------------------------------------------
// Filesystem state
// ---------------------------------------------------------------------------

/// Clean.
pub const SYSV_FS_CLEAN: u32 = 0x0001;
/// Error.
pub const SYSV_FS_ERROR: u32 = 0x0002;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_values_distinct() {
        let magics = [SYSV_MAGIC, COH_MAGIC, XENIX_MAGIC];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_fs_types_sequential() {
        assert_eq!(FSTYPE_SYSV4, 1);
        assert_eq!(FSTYPE_SYSV2, 2);
        assert_eq!(FSTYPE_COH, 3);
        assert_eq!(FSTYPE_XENIX, 4);
        assert_eq!(FSTYPE_AFS, 5);
    }

    #[test]
    fn test_block_sizes() {
        assert_eq!(SYSV_BLOCK_SIZE, 1024);
        assert_eq!(XENIX_BLOCK_SIZE, 1024);
        assert_eq!(COH_BLOCK_SIZE, 512);
    }

    #[test]
    fn test_block_sizes_power_of_two() {
        assert!(SYSV_BLOCK_SIZE.is_power_of_two());
        assert!(XENIX_BLOCK_SIZE.is_power_of_two());
        assert!(COH_BLOCK_SIZE.is_power_of_two());
    }

    #[test]
    fn test_inode_params() {
        assert_eq!(SYSV_INODE_SIZE, 64);
        assert_eq!(SYSV_NAMELEN, 14);
    }

    #[test]
    fn test_root_inode() {
        assert_eq!(SYSV_ROOT_INO, 2);
    }

    #[test]
    fn test_free_list_sizes() {
        assert_eq!(SYSV_NICFREE, 50);
        assert_eq!(SYSV_NICINOD, 100);
        assert!(SYSV_NICFREE < SYSV_NICINOD);
    }

    #[test]
    fn test_fs_state_flags() {
        assert_eq!(SYSV_FS_CLEAN, 1);
        assert_eq!(SYSV_FS_ERROR, 2);
        assert_eq!(SYSV_FS_CLEAN & SYSV_FS_ERROR, 0);
    }

    #[test]
    fn test_link_max() {
        assert_eq!(SYSV_LINK_MAX, 1000);
    }
}
