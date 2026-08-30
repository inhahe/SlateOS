//! BeFS (BeOS filesystem) on-disk constants.
//!
//! Linux carries a read-only `befs` driver for the Be Filesystem used
//! by BeOS and Haiku. The on-disk structures are little-endian and
//! identified by a 4-byte magic at the start of the superblock.

// ---------------------------------------------------------------------------
// Filesystem-name identifier
// ---------------------------------------------------------------------------

pub const BEFS_FSTYPE: &str = "befs";

// ---------------------------------------------------------------------------
// Superblock magics
// ---------------------------------------------------------------------------

/// Primary superblock magic at offset 32.
pub const BEFS_SUPER_MAGIC1: u32 = 0x42465331;
/// Block-shift validation magic 2.
pub const BEFS_SUPER_MAGIC2: u32 = 0xDD121031;
/// Final integrity magic 3.
pub const BEFS_SUPER_MAGIC3: u32 = 0x15B6830E;

/// Superblock starts 512 bytes into the volume.
pub const BEFS_SUPER_BLOCK_OFFSET: u32 = 512;

/// Inode signature ("BFNI").
pub const BEFS_INODE_MAGIC1: u32 = 0x3BBE0AD9;

/// Btree-node signature ("BTNJ").
pub const BEFS_BTREE_MAGIC: u32 = 0x69F6C2E8;

// ---------------------------------------------------------------------------
// Block-size limits (BeFS allows 1 KiB..8 KiB blocks)
// ---------------------------------------------------------------------------

pub const BEFS_MIN_BLOCK_SIZE: u32 = 1024;
pub const BEFS_MAX_BLOCK_SIZE: u32 = 8192;

// ---------------------------------------------------------------------------
// Inode flag bits (`befs_inode.flags`)
// ---------------------------------------------------------------------------

pub const BEFS_INODE_IN_USE: u32 = 0x0000_0001;
pub const BEFS_ATTR_INODE: u32 = 0x0000_0004;
pub const BEFS_INODE_LOGGED: u32 = 0x0000_0008;
pub const BEFS_INODE_DELETED: u32 = 0x0000_0010;
pub const BEFS_PERMANENT_FLAG: u32 = 0x0000_0001;
pub const BEFS_INODE_NO_CREATE_TIME: u32 = 0x0001_0000;
pub const BEFS_INODE_WAS_WRITTEN: u32 = 0x0002_0000;
pub const BEFS_NO_TRANSACTION: u32 = 0x0004_0000;

// ---------------------------------------------------------------------------
// File-name and symlink-target maxima
// ---------------------------------------------------------------------------

pub const BEFS_NAME_LEN: usize = 255;
pub const BEFS_SYMLINK_LEN: usize = 160;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filesystem_token() {
        assert_eq!(BEFS_FSTYPE, "befs");
    }

    #[test]
    fn test_super_magics_distinct_and_first_is_ascii_bfs1() {
        // Magic 1 spells "BFS1" in big-endian ASCII (the on-disk bytes
        // happen to be a little-endian u32 of 0x42465331 == 'B','F','S','1').
        assert_eq!(BEFS_SUPER_MAGIC1, 0x42465331);
        let bytes = BEFS_SUPER_MAGIC1.to_be_bytes();
        assert_eq!(&bytes, b"BFS1");
        // All three super magics are distinct.
        let m = [BEFS_SUPER_MAGIC1, BEFS_SUPER_MAGIC2, BEFS_SUPER_MAGIC3];
        for (i, &a) in m.iter().enumerate() {
            for &b in &m[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // SB sits 512 B into the device (after the boot sector).
        assert_eq!(BEFS_SUPER_BLOCK_OFFSET, 512);
    }

    #[test]
    fn test_inode_and_btree_magics_distinct_from_super() {
        let all = [
            BEFS_SUPER_MAGIC1,
            BEFS_SUPER_MAGIC2,
            BEFS_SUPER_MAGIC3,
            BEFS_INODE_MAGIC1,
            BEFS_BTREE_MAGIC,
        ];
        for (i, &a) in all.iter().enumerate() {
            for &b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_block_size_bounds_are_powers_of_two() {
        assert!(BEFS_MIN_BLOCK_SIZE.is_power_of_two());
        assert!(BEFS_MAX_BLOCK_SIZE.is_power_of_two());
        assert_eq!(BEFS_MIN_BLOCK_SIZE, 1024);
        assert_eq!(BEFS_MAX_BLOCK_SIZE, 8192);
        assert!(BEFS_MIN_BLOCK_SIZE < BEFS_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_inode_flag_bits_are_single_bits() {
        let f = [
            BEFS_INODE_IN_USE,
            BEFS_ATTR_INODE,
            BEFS_INODE_LOGGED,
            BEFS_INODE_DELETED,
            BEFS_INODE_NO_CREATE_TIME,
            BEFS_INODE_WAS_WRITTEN,
            BEFS_NO_TRANSACTION,
        ];
        for &v in &f {
            assert!(v.is_power_of_two());
        }
        // PERMANENT_FLAG aliases IN_USE — these are the same on-disk bit
        // viewed as different abstractions.
        assert_eq!(BEFS_PERMANENT_FLAG, BEFS_INODE_IN_USE);
    }

    #[test]
    fn test_name_and_symlink_lengths() {
        // Matches Linux NAME_MAX for portability with native paths.
        assert_eq!(BEFS_NAME_LEN, 255);
        // Inline symlink storage (160 B fits inside the inode).
        assert_eq!(BEFS_SYMLINK_LEN, 160);
        assert!(BEFS_NAME_LEN > BEFS_SYMLINK_LEN);
    }
}
