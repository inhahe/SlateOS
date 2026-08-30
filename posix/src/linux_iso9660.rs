//! `<linux/iso_fs.h>` — ISO 9660 (CD-ROM filesystem) constants.
//!
//! ISO 9660 is the standard filesystem for CD-ROMs and DVD-ROMs.
//! It has several extension levels: Rock Ridge (POSIX attributes),
//! Joliet (Unicode filenames), and El Torito (bootable media).
//! Linux's isofs driver supports all common extensions.

// ---------------------------------------------------------------------------
// ISO 9660 identification
// ---------------------------------------------------------------------------

/// Standard identifier in volume descriptor ("CD001").
pub const ISO9660_STANDARD_ID: &str = "CD001";
/// Volume descriptor version.
pub const ISO9660_VD_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Volume descriptor types
// ---------------------------------------------------------------------------

/// Boot record.
pub const ISO9660_VD_BOOT_RECORD: u8 = 0;
/// Primary volume descriptor.
pub const ISO9660_VD_PRIMARY: u8 = 1;
/// Supplementary volume descriptor (Joliet).
pub const ISO9660_VD_SUPPLEMENTARY: u8 = 2;
/// Volume partition descriptor.
pub const ISO9660_VD_PARTITION: u8 = 3;
/// Volume descriptor set terminator.
pub const ISO9660_VD_TERMINATOR: u8 = 255;

// ---------------------------------------------------------------------------
// File flags (directory record)
// ---------------------------------------------------------------------------

/// File is hidden.
pub const ISO9660_FILE_HIDDEN: u8 = 1 << 0;
/// Entry is a directory.
pub const ISO9660_FILE_DIRECTORY: u8 = 1 << 1;
/// Associated file.
pub const ISO9660_FILE_ASSOCIATED: u8 = 1 << 2;
/// Record format information in xattr.
pub const ISO9660_FILE_RECORD: u8 = 1 << 3;
/// Owner/group/permission in xattr.
pub const ISO9660_FILE_PROTECTION: u8 = 1 << 4;
/// Not the final directory record for this file.
pub const ISO9660_FILE_MULTI_EXTENT: u8 = 1 << 7;

// ---------------------------------------------------------------------------
// Sector/block sizes
// ---------------------------------------------------------------------------

/// Logical sector size.
pub const ISO9660_SECTOR_SIZE: u16 = 2048;
/// System area size (first 16 sectors).
pub const ISO9660_SYSTEM_AREA_SIZE: u32 = 16 * 2048;

// ---------------------------------------------------------------------------
// Rock Ridge extension signatures
// ---------------------------------------------------------------------------

/// POSIX file attributes (PX).
pub const RR_SIG_PX: &str = "PX";
/// POSIX device numbers (PN).
pub const RR_SIG_PN: &str = "PN";
/// Symbolic link (SL).
pub const RR_SIG_SL: &str = "SL";
/// Alternate name (NM).
pub const RR_SIG_NM: &str = "NM";
/// Relocated directory (CL).
pub const RR_SIG_CL: &str = "CL";
/// Timestamps (TF).
pub const RR_SIG_TF: &str = "TF";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_id() {
        assert_eq!(ISO9660_STANDARD_ID, "CD001");
        assert_eq!(ISO9660_STANDARD_ID.len(), 5);
    }

    #[test]
    fn test_vd_types_distinct() {
        let types = [
            ISO9660_VD_BOOT_RECORD,
            ISO9660_VD_PRIMARY,
            ISO9660_VD_SUPPLEMENTARY,
            ISO9660_VD_PARTITION,
            ISO9660_VD_TERMINATOR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_file_flags_no_overlap() {
        let flags = [
            ISO9660_FILE_HIDDEN,
            ISO9660_FILE_DIRECTORY,
            ISO9660_FILE_ASSOCIATED,
            ISO9660_FILE_RECORD,
            ISO9660_FILE_PROTECTION,
            ISO9660_FILE_MULTI_EXTENT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sector_size() {
        assert_eq!(ISO9660_SECTOR_SIZE, 2048);
        assert_eq!(ISO9660_SYSTEM_AREA_SIZE, 32768);
    }

    #[test]
    fn test_rr_signatures_distinct() {
        let sigs = [
            RR_SIG_PX, RR_SIG_PN, RR_SIG_SL, RR_SIG_NM, RR_SIG_CL, RR_SIG_TF,
        ];
        for i in 0..sigs.len() {
            assert_eq!(sigs[i].len(), 2);
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }
}
