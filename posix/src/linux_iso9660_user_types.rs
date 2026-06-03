//! `<linux/iso_fs.h>` — ISO 9660 (CDFS) on-disk constants.
//!
//! ISO 9660 is still the only filesystem every install ISO, every
//! firmware boot disc, and every cdrtools-style tool agree on. The
//! constants below capture the Volume Descriptor layout, the Rock
//! Ridge and Joliet extensions, and the mount-option flags `mount
//! -t iso9660` reads.

// ---------------------------------------------------------------------------
// Filesystem identity
// ---------------------------------------------------------------------------

/// `statfs.f_type` for ISO 9660.
pub const ISOFS_SUPER_MAGIC: u32 = 0x9660;
/// On-disc sector size for CD-ROM (ECMA-130 mode 1).
pub const ISOFS_BLOCK_SIZE: u32 = 2048;
/// First sector used by the ISO 9660 volume area (after the 16-sector
/// system area at the start of the disc).
pub const ISO_VD_FIRST_SECTOR: u32 = 16;
/// Standard ID stored at offset 1 of every Volume Descriptor.
pub const ISO_STANDARD_ID: &[u8; 5] = b"CD001";

// ---------------------------------------------------------------------------
// Volume Descriptor types (ECMA-119 §8.1.1)
// ---------------------------------------------------------------------------

pub const ISO_VD_BOOT_RECORD: u8 = 0;
pub const ISO_VD_PRIMARY: u8 = 1;
pub const ISO_VD_SUPPLEMENTARY: u8 = 2;
pub const ISO_VD_PARTITION: u8 = 3;
pub const ISO_VD_SET_TERMINATOR: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Directory record flags (`isofs_directory_record.flags`)
// ---------------------------------------------------------------------------

pub const ISO_FILE_HIDDEN: u8 = 1 << 0;
pub const ISO_FILE_DIRECTORY: u8 = 1 << 1;
pub const ISO_FILE_ASSOCIATED: u8 = 1 << 2;
pub const ISO_FILE_RECORD: u8 = 1 << 3;
pub const ISO_FILE_PROTECTION: u8 = 1 << 4;
/// Bit 7: more directory records follow for this file (multi-extent).
pub const ISO_FILE_MULTIEXTENT: u8 = 1 << 7;

// ---------------------------------------------------------------------------
// Mount option flags (legacy `iso9660_opts.flags`)
// ---------------------------------------------------------------------------

pub const ISOFS_RR: u32 = 1 << 0;
pub const ISOFS_JOLIET: u32 = 1 << 1;
pub const ISOFS_NORR: u32 = 1 << 2;
pub const ISOFS_UNHIDE: u32 = 1 << 3;
pub const ISOFS_UTF8: u32 = 1 << 4;
pub const ISOFS_NOCOMPRESS: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Joliet escape sequences (UCS-2 levels in SVD)
// ---------------------------------------------------------------------------

pub const JOLIET_LEVEL1: &[u8; 3] = b"%/@";
pub const JOLIET_LEVEL2: &[u8; 3] = b"%/C";
pub const JOLIET_LEVEL3: &[u8; 3] = b"%/E";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_and_block_size() {
        // Linux statfs magic for iso9660 — matches /usr/include/linux/magic.h.
        assert_eq!(ISOFS_SUPER_MAGIC, 0x9660);
        assert_eq!(ISOFS_BLOCK_SIZE, 2048);
        // 16 reserved system-area sectors.
        assert_eq!(ISO_VD_FIRST_SECTOR, 16);
        assert_eq!(ISO_STANDARD_ID, b"CD001");
    }

    #[test]
    fn test_vd_types_distinct() {
        let v = [
            ISO_VD_BOOT_RECORD,
            ISO_VD_PRIMARY,
            ISO_VD_SUPPLEMENTARY,
            ISO_VD_PARTITION,
            ISO_VD_SET_TERMINATOR,
        ];
        for i in 0..v.len() {
            for j in (i + 1)..v.len() {
                assert_ne!(v[i], v[j]);
            }
        }
        // Set terminator is the sentinel 0xFF.
        assert_eq!(ISO_VD_SET_TERMINATOR, 0xFF);
    }

    #[test]
    fn test_record_flags_single_bit() {
        for &b in &[
            ISO_FILE_HIDDEN,
            ISO_FILE_DIRECTORY,
            ISO_FILE_ASSOCIATED,
            ISO_FILE_RECORD,
            ISO_FILE_PROTECTION,
            ISO_FILE_MULTIEXTENT,
        ] {
            assert!(b.is_power_of_two());
        }
        // MULTIEXTENT is bit 7 — the highest bit.
        assert_eq!(ISO_FILE_MULTIEXTENT, 0x80);
    }

    #[test]
    fn test_mount_flags_pow2() {
        for &b in &[
            ISOFS_RR,
            ISOFS_JOLIET,
            ISOFS_NORR,
            ISOFS_UNHIDE,
            ISOFS_UTF8,
            ISOFS_NOCOMPRESS,
        ] {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_joliet_escape_sequences() {
        // Three Joliet UCS-2 levels per ECMA-167 / Joliet spec.
        assert_eq!(JOLIET_LEVEL1, b"%/@");
        assert_eq!(JOLIET_LEVEL2, b"%/C");
        assert_eq!(JOLIET_LEVEL3, b"%/E");
        // All differ only in the level letter.
        assert_eq!(JOLIET_LEVEL1[0..2], JOLIET_LEVEL2[0..2]);
        assert_eq!(JOLIET_LEVEL2[0..2], JOLIET_LEVEL3[0..2]);
    }
}
