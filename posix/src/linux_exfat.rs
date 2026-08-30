//! `<linux/exfat_fs.h>` — exFAT filesystem constants.
//!
//! exFAT (Extended File Allocation Table) is Microsoft's filesystem
//! designed for flash media and SD cards. It removes FAT32's 4 GiB
//! file size limit while remaining lightweight. It's the default
//! format for SDXC cards and widely used for USB drives.

// ---------------------------------------------------------------------------
// exFAT magic and boot sector
// ---------------------------------------------------------------------------

/// exFAT filesystem signature ("EXFAT   ").
pub const EXFAT_SIGNATURE: u64 = 0x2020_2054_4146_5845;
/// Boot sector signature (0xAA55).
pub const EXFAT_BOOT_SIGNATURE: u16 = 0xAA55;

// ---------------------------------------------------------------------------
// Directory entry types
// ---------------------------------------------------------------------------

/// Allocation bitmap.
pub const EXFAT_ENTRY_BITMAP: u8 = 0x81;
/// Upcase table.
pub const EXFAT_ENTRY_UPCASE: u8 = 0x82;
/// Volume label.
pub const EXFAT_ENTRY_LABEL: u8 = 0x83;
/// File directory entry.
pub const EXFAT_ENTRY_FILE: u8 = 0x85;
/// Stream extension entry.
pub const EXFAT_ENTRY_STREAM: u8 = 0xC0;
/// File name entry.
pub const EXFAT_ENTRY_NAME: u8 = 0xC1;

// ---------------------------------------------------------------------------
// File attributes
// ---------------------------------------------------------------------------

/// Read-only.
pub const EXFAT_ATTR_READONLY: u16 = 0x0001;
/// Hidden.
pub const EXFAT_ATTR_HIDDEN: u16 = 0x0002;
/// System.
pub const EXFAT_ATTR_SYSTEM: u16 = 0x0004;
/// Directory.
pub const EXFAT_ATTR_DIRECTORY: u16 = 0x0010;
/// Archive.
pub const EXFAT_ATTR_ARCHIVE: u16 = 0x0020;

// ---------------------------------------------------------------------------
// Cluster constants
// ---------------------------------------------------------------------------

/// First valid cluster number.
pub const EXFAT_FIRST_CLUSTER: u32 = 2;
/// End-of-chain marker.
pub const EXFAT_EOF_CLUSTER: u32 = 0xFFFF_FFFF;
/// Bad cluster marker.
pub const EXFAT_BAD_CLUSTER: u32 = 0xFFFF_FFF7;

// ---------------------------------------------------------------------------
// Sector/cluster size limits
// ---------------------------------------------------------------------------

/// Minimum sector size (512 bytes, shift = 9).
pub const EXFAT_MIN_SECTOR_SHIFT: u8 = 9;
/// Maximum sector size (4096 bytes, shift = 12).
pub const EXFAT_MAX_SECTOR_SHIFT: u8 = 12;
/// Maximum cluster size shift (25 = 32 MiB).
pub const EXFAT_MAX_CLUSTER_SHIFT: u8 = 25;

// ---------------------------------------------------------------------------
// Volume flags
// ---------------------------------------------------------------------------

/// Active FAT (second FAT active).
pub const EXFAT_VOL_ACTIVE_FAT: u16 = 1 << 0;
/// Volume dirty.
pub const EXFAT_VOL_DIRTY: u16 = 1 << 1;
/// Media failure.
pub const EXFAT_VOL_MEDIA_FAILURE: u16 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_signature() {
        assert_eq!(EXFAT_BOOT_SIGNATURE, 0xAA55);
    }

    #[test]
    fn test_entry_types_distinct() {
        let types = [
            EXFAT_ENTRY_BITMAP,
            EXFAT_ENTRY_UPCASE,
            EXFAT_ENTRY_LABEL,
            EXFAT_ENTRY_FILE,
            EXFAT_ENTRY_STREAM,
            EXFAT_ENTRY_NAME,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_attributes_no_overlap() {
        let attrs = [
            EXFAT_ATTR_READONLY,
            EXFAT_ATTR_HIDDEN,
            EXFAT_ATTR_SYSTEM,
            EXFAT_ATTR_DIRECTORY,
            EXFAT_ATTR_ARCHIVE,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_cluster_constants() {
        assert_eq!(EXFAT_FIRST_CLUSTER, 2);
        assert_ne!(EXFAT_EOF_CLUSTER, EXFAT_BAD_CLUSTER);
    }

    #[test]
    fn test_sector_shift_ordering() {
        assert!(EXFAT_MIN_SECTOR_SHIFT < EXFAT_MAX_SECTOR_SHIFT);
        assert!(EXFAT_MAX_SECTOR_SHIFT < EXFAT_MAX_CLUSTER_SHIFT);
    }

    #[test]
    fn test_volume_flags_no_overlap() {
        let flags = [
            EXFAT_VOL_ACTIVE_FAT,
            EXFAT_VOL_DIRTY,
            EXFAT_VOL_MEDIA_FAILURE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
