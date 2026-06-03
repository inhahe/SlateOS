//! `<linux/affs_fs.h>` — Amiga Fast Filesystem.
//!
//! Linux includes a read/write driver for AmigaOS's FFS, used by
//! emulators (FS-UAE, WinUAE+WSL) and by people archiving original
//! Amiga floppies/HDFs.

// ---------------------------------------------------------------------------
// Filesystem name and magic
// ---------------------------------------------------------------------------

pub const AFFS_FS_NAME: &str = "affs";

/// Not in `linux/magic.h` — Linux returns 0xADFF for `statfs.f_type`.
pub const AFFS_SUPER_MAGIC: u32 = 0xADFF;

// ---------------------------------------------------------------------------
// On-disk geometry
// ---------------------------------------------------------------------------

/// Floppies are 512-byte sectors; HDFs may be larger but Linux normalises.
pub const AFFS_BLOCK_SIZE: usize = 512;

/// Root-block index on a freshly formatted floppy.
pub const AFFS_ROOT_BLOCK_FLOPPY: u32 = 880;

/// Maximum filename component length (`fh.name`).
pub const AFFS_NAME_MAX: usize = 30;

/// Bitmap entries per block on a 512-byte filesystem.
pub const AFFS_BITMAP_ENTRIES_PER_BLOCK: usize = 127;

// ---------------------------------------------------------------------------
// On-disk type / sec-type codes (`fh.primary_type`, `secondary_type`)
// ---------------------------------------------------------------------------

pub const AFFS_T_SHORT: i32 = 2;
pub const AFFS_T_LIST: i32 = 16;
pub const AFFS_T_DATA: i32 = 8;

pub const AFFS_ST_ROOT: i32 = 1;
pub const AFFS_ST_USERDIR: i32 = 2;
pub const AFFS_ST_SOFTLINK: i32 = 3;
pub const AFFS_ST_LINKDIR: i32 = 4;
pub const AFFS_ST_FILE: i32 = -3;
pub const AFFS_ST_LINKFILE: i32 = -4;

// ---------------------------------------------------------------------------
// Filesystem-type signatures (`DOS\0` family, written in big-endian)
// ---------------------------------------------------------------------------

pub const AFFS_SIG_OFS: [u8; 4] = *b"DOS\0"; // Old File System
pub const AFFS_SIG_FFS: [u8; 4] = *b"DOS\x01"; // Fast File System
pub const AFFS_SIG_OFS_INTL: [u8; 4] = *b"DOS\x02"; // OFS, international
pub const AFFS_SIG_FFS_INTL: [u8; 4] = *b"DOS\x03"; // FFS, international
pub const AFFS_SIG_OFS_DC: [u8; 4] = *b"DOS\x04"; // OFS, dir cache
pub const AFFS_SIG_FFS_DC: [u8; 4] = *b"DOS\x05"; // FFS, dir cache

// ---------------------------------------------------------------------------
// Mount options
// ---------------------------------------------------------------------------

pub const AFFS_OPT_MODE: &str = "mode=";
pub const AFFS_OPT_PROTECT: &str = "protect";
pub const AFFS_OPT_RESERVED: &str = "reserved=";
pub const AFFS_OPT_VERBOSE: &str = "verbose";
pub const AFFS_OPT_PREFIX: &str = "prefix=";
pub const AFFS_OPT_VOLUME: &str = "volume=";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_name_and_magic() {
        assert_eq!(AFFS_FS_NAME, "affs");
        assert_eq!(AFFS_SUPER_MAGIC, 0xADFF);
        // Distinct from ADFS's 0xADF5.
        assert_ne!(AFFS_SUPER_MAGIC, 0xADF5);
    }

    #[test]
    fn test_block_geometry() {
        assert_eq!(AFFS_BLOCK_SIZE, 512);
        // Floppies have ~1760 blocks; root sits in the middle at 880.
        assert_eq!(AFFS_ROOT_BLOCK_FLOPPY, 880);
        // 127 bitmap entries × 4 bytes + 4 bytes checksum = 512.
        assert_eq!(AFFS_BITMAP_ENTRIES_PER_BLOCK * 4 + 4, AFFS_BLOCK_SIZE);
    }

    #[test]
    fn test_name_max_30() {
        assert_eq!(AFFS_NAME_MAX, 30);
    }

    #[test]
    fn test_primary_types_distinct() {
        let p = [AFFS_T_SHORT, AFFS_T_LIST, AFFS_T_DATA];
        for i in 0..p.len() {
            for j in (i + 1)..p.len() {
                assert_ne!(p[i], p[j]);
            }
        }
    }

    #[test]
    fn test_secondary_types_positive_dirs_negative_files() {
        // Positive ST_* are directory-like; negative are file-like.
        assert!(AFFS_ST_ROOT > 0);
        assert!(AFFS_ST_USERDIR > 0);
        assert!(AFFS_ST_SOFTLINK > 0);
        assert!(AFFS_ST_LINKDIR > 0);
        assert!(AFFS_ST_FILE < 0);
        assert!(AFFS_ST_LINKFILE < 0);
        // ROOT/USERDIR/SOFTLINK/LINKDIR are dense 1..4.
        assert_eq!(AFFS_ST_ROOT, 1);
        assert_eq!(AFFS_ST_USERDIR, 2);
        assert_eq!(AFFS_ST_SOFTLINK, 3);
        assert_eq!(AFFS_ST_LINKDIR, 4);
    }

    #[test]
    fn test_signatures_start_with_dos() {
        let s = [
            AFFS_SIG_OFS,
            AFFS_SIG_FFS,
            AFFS_SIG_OFS_INTL,
            AFFS_SIG_FFS_INTL,
            AFFS_SIG_OFS_DC,
            AFFS_SIG_FFS_DC,
        ];
        for sig in s {
            assert_eq!(&sig[..3], b"DOS");
        }
        // The last byte is a dense version 0..5.
        for (i, sig) in s.iter().enumerate() {
            assert_eq!(sig[3] as usize, i);
        }
    }

    #[test]
    fn test_mount_option_strings() {
        for o in [
            AFFS_OPT_MODE,
            AFFS_OPT_PROTECT,
            AFFS_OPT_RESERVED,
            AFFS_OPT_VERBOSE,
            AFFS_OPT_PREFIX,
            AFFS_OPT_VOLUME,
        ] {
            assert!(!o.is_empty());
        }
        // mode/reserved/prefix/volume are "key=" options; protect/verbose
        // are bare keywords.
        assert!(AFFS_OPT_MODE.ends_with('='));
        assert!(AFFS_OPT_RESERVED.ends_with('='));
        assert!(!AFFS_OPT_PROTECT.contains('='));
        assert!(!AFFS_OPT_VERBOSE.contains('='));
    }
}
