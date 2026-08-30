//! `<linux/adfs_fs.h>` — ADFS (Acorn Disc Filing System) constants.
//!
//! ADFS is the native filesystem of Acorn RISC OS machines.
//! These constants define magic numbers, disc record fields,
//! and file type parameters.

// ---------------------------------------------------------------------------
// Magic / disc record
// ---------------------------------------------------------------------------

/// ADFS superblock magic.
pub const ADFS_SUPER_MAGIC: u32 = 0xADF5;
/// Disc record offset in boot block.
pub const ADFS_DR_OFFSET: u32 = 0x01C0;
/// Boot block offset (sector 0).
pub const ADFS_BOOT_OFFSET: u32 = 0x0C00;

// ---------------------------------------------------------------------------
// Disc format types
// ---------------------------------------------------------------------------

/// Old directory format (small disc).
pub const ADFS_OLD_DIR: u32 = 0;
/// New directory format (large disc).
pub const ADFS_NEW_DIR: u32 = 1;
/// F+ directory format (RISC OS 4+).
pub const ADFS_FPLUS_DIR: u32 = 2;

// ---------------------------------------------------------------------------
// File type bits
// ---------------------------------------------------------------------------

/// File type mask (12 bits).
pub const ADFS_FILETYPE_MASK: u32 = 0xFFF00;
/// File type shift.
pub const ADFS_FILETYPE_SHIFT: u32 = 8;
/// Load/exec flag (typed file marker).
pub const ADFS_FILETYPE_MARKER: u32 = 0xFFF00000;

// ---------------------------------------------------------------------------
// Common file types
// ---------------------------------------------------------------------------

/// Text file type.
pub const ADFS_FILETYPE_TEXT: u32 = 0xFFF;
/// Data file type.
pub const ADFS_FILETYPE_DATA: u32 = 0xFFD;
/// Command (Obey) file.
pub const ADFS_FILETYPE_COMMAND: u32 = 0xFEB;
/// BASIC program.
pub const ADFS_FILETYPE_BASIC: u32 = 0xFFB;
/// Utility.
pub const ADFS_FILETYPE_UTILITY: u32 = 0xFFC;

// ---------------------------------------------------------------------------
// Access permissions
// ---------------------------------------------------------------------------

/// Owner read.
pub const ADFS_OWNER_READ: u32 = 0x01;
/// Owner write.
pub const ADFS_OWNER_WRITE: u32 = 0x02;
/// Owner lock.
pub const ADFS_OWNER_LOCK: u32 = 0x04;
/// Public read.
pub const ADFS_PUBLIC_READ: u32 = 0x10;
/// Public write.
pub const ADFS_PUBLIC_WRITE: u32 = 0x20;

// ---------------------------------------------------------------------------
// Sector/zone sizes
// ---------------------------------------------------------------------------

/// Map bits per zone entry.
pub const ADFS_MAP_BITS: u32 = 1;
/// Minimum zone size.
pub const ADFS_MIN_ZONE_SIZE: u32 = 512;
/// Maximum zones.
pub const ADFS_MAX_ZONES: u32 = 127;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(ADFS_SUPER_MAGIC, 0xADF5);
    }

    #[test]
    fn test_dir_formats_distinct() {
        let formats = [ADFS_OLD_DIR, ADFS_NEW_DIR, ADFS_FPLUS_DIR];
        for i in 0..formats.len() {
            for j in (i + 1)..formats.len() {
                assert_ne!(formats[i], formats[j]);
            }
        }
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            ADFS_FILETYPE_TEXT,
            ADFS_FILETYPE_DATA,
            ADFS_FILETYPE_COMMAND,
            ADFS_FILETYPE_BASIC,
            ADFS_FILETYPE_UTILITY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_permissions_no_overlap() {
        assert_eq!(ADFS_OWNER_READ & ADFS_OWNER_WRITE, 0);
        assert_eq!(ADFS_OWNER_READ & ADFS_PUBLIC_READ, 0);
    }

    #[test]
    fn test_permissions_power_of_two() {
        let perms = [
            ADFS_OWNER_READ,
            ADFS_OWNER_WRITE,
            ADFS_OWNER_LOCK,
            ADFS_PUBLIC_READ,
            ADFS_PUBLIC_WRITE,
        ];
        for p in &perms {
            assert!(p.is_power_of_two(), "0x{:02x} not power of two", p);
        }
    }

    #[test]
    fn test_zone_limits() {
        assert!(ADFS_MIN_ZONE_SIZE.is_power_of_two());
        assert_eq!(ADFS_MAX_ZONES, 127);
    }
}
