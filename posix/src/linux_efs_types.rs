//! `<linux/efs_fs.h>` — EFS (Extent File System) constants.
//!
//! EFS is the original SGI IRIX filesystem.
//! These constants define magic numbers, superblock fields,
//! and inode parameters.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// EFS superblock magic.
pub const EFS_SUPER_MAGIC: u32 = 0x00072959;
/// EFS new superblock magic.
pub const EFS_NEWMAGIC: u32 = 0x0007295A;
/// Linux VFS super magic.
pub const EFS_MAGIC: u32 = 0x00414A53;

// ---------------------------------------------------------------------------
// Superblock parameters
// ---------------------------------------------------------------------------

/// Block size (512 bytes).
pub const EFS_BLOCKSIZE: u32 = 512;
/// Maximum file name length.
pub const EFS_MAXNAMELEN: u32 = 255;
/// Direct extents per inode.
pub const EFS_DIRECTEXTENTS: u32 = 12;

// ---------------------------------------------------------------------------
// Inode constants
// ---------------------------------------------------------------------------

/// Root inode number.
pub const EFS_ROOTINO: u32 = 2;
/// First non-reserved inode.
pub const EFS_FIRST_INO: u32 = 2;
/// Inode size.
pub const EFS_INODE_SIZE: u32 = 128;
/// Inodes per block.
pub const EFS_INOPBB: u32 = EFS_BLOCKSIZE / EFS_INODE_SIZE;

// ---------------------------------------------------------------------------
// Filesystem state
// ---------------------------------------------------------------------------

/// Clean.
pub const EFS_CLEAN: u32 = 0x0000;
/// Dirty.
pub const EFS_DIRTY: u32 = 0x0001;
/// Active (mounted).
pub const EFS_ACTIVE: u32 = 0x0002;
/// Bad (needs repair).
pub const EFS_BAD: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Extent parameters
// ---------------------------------------------------------------------------

/// Maximum extent length (in basic blocks).
pub const EFS_MAX_EXTENT: u32 = 0x00FFFFFF;
/// Extent offset mask.
pub const EFS_EXTENT_OFFSET_MASK: u32 = 0x00FFFFFF;
/// Extent length mask.
pub const EFS_EXTENT_LENGTH_MASK: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Allocation group
// ---------------------------------------------------------------------------

/// Cylinder group size (in basic blocks).
pub const EFS_CGSIZE_DEFAULT: u32 = 16384;
/// Bitmap blocks per CG.
pub const EFS_BITMAP_BLOCKS: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_values_distinct() {
        let magics = [EFS_SUPER_MAGIC, EFS_NEWMAGIC, EFS_MAGIC];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_blocksize() {
        assert_eq!(EFS_BLOCKSIZE, 512);
        assert!(EFS_BLOCKSIZE.is_power_of_two());
    }

    #[test]
    fn test_inodes_per_block() {
        assert_eq!(EFS_INOPBB, 4);
        assert_eq!(EFS_INOPBB, EFS_BLOCKSIZE / EFS_INODE_SIZE);
    }

    #[test]
    fn test_root_inode() {
        assert_eq!(EFS_ROOTINO, 2);
    }

    #[test]
    fn test_fs_states_distinct() {
        let states = [EFS_CLEAN, EFS_DIRTY, EFS_ACTIVE, EFS_BAD];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_extent_max() {
        assert_eq!(EFS_MAX_EXTENT, 0x00FFFFFF);
    }

    #[test]
    fn test_namelen() {
        assert_eq!(EFS_MAXNAMELEN, 255);
    }

    #[test]
    fn test_direct_extents() {
        assert_eq!(EFS_DIRECTEXTENTS, 12);
    }
}
