//! `<linux/gfs2_ondisk.h>` — GFS2 on-disk format constants.
//!
//! Constants describing the on-disk layout of the GFS2 cluster
//! filesystem — magic numbers, format/metatype tags, dinode flags,
//! and the rgrp resource-group descriptor bits.

// ---------------------------------------------------------------------------
// Magic numbers and format versions
// ---------------------------------------------------------------------------

/// Identifier in superblock and metaheader.
pub const GFS2_MAGIC: u32 = 0x0161_1970;
/// Mount-protocol version (kernel ABI checked by mount.gfs2).
pub const GFS2_MOUNT_VERSION: u32 = 4;
/// Superblock version.
pub const GFS2_FORMAT_FS: u32 = 1801;
/// Lock-protocol multi-host version.
pub const GFS2_FORMAT_MULTI: u32 = 1900;
/// Resource-group descriptor format.
pub const GFS2_FORMAT_RG: u32 = 1002;
/// Dinode format.
pub const GFS2_FORMAT_DI: u32 = 1201;
/// Indirect block format.
pub const GFS2_FORMAT_IN: u32 = 1300;
/// Leaf-page format (directory hash leaf).
pub const GFS2_FORMAT_LF: u32 = 1400;

// ---------------------------------------------------------------------------
// Metatype codes (struct gfs2_meta_header.mh_type)
// ---------------------------------------------------------------------------

/// None / placeholder.
pub const GFS2_METATYPE_NONE: u32 = 0;
/// Superblock.
pub const GFS2_METATYPE_SB: u32 = 1;
/// Resource group descriptor.
pub const GFS2_METATYPE_RG: u32 = 2;
/// Resource group bitmap.
pub const GFS2_METATYPE_RB: u32 = 3;
/// Disk inode.
pub const GFS2_METATYPE_DI: u32 = 4;
/// Indirect block.
pub const GFS2_METATYPE_IN: u32 = 5;
/// Directory hash leaf.
pub const GFS2_METATYPE_LF: u32 = 6;
/// Journal data block.
pub const GFS2_METATYPE_JD: u32 = 7;
/// Journal log descriptor.
pub const GFS2_METATYPE_LH: u32 = 8;
/// Journal log block.
pub const GFS2_METATYPE_LB: u32 = 9;
/// Extended attribute header.
pub const GFS2_METATYPE_EA: u32 = 10;
/// Extended attribute data.
pub const GFS2_METATYPE_ED: u32 = 11;
/// Quota change.
pub const GFS2_METATYPE_QC: u32 = 12;

// ---------------------------------------------------------------------------
// Dinode flags (struct gfs2_dinode.di_flags)
// ---------------------------------------------------------------------------

/// Journaled data.
pub const GFS2_DIF_JDATA: u32 = 0x0000_0001;
/// File data is exclusively cached.
pub const GFS2_DIF_EXHASH: u32 = 0x0000_0002;
/// Unused.
pub const GFS2_DIF_UNUSED: u32 = 0x0000_0004;
/// End-of-leaf marker.
pub const GFS2_DIF_EA_INDIRECT: u32 = 0x0000_0008;
/// Directory entries are case-folded.
pub const GFS2_DIF_DIRECTIO: u32 = 0x0000_0010;
/// Immutable.
pub const GFS2_DIF_IMMUTABLE: u32 = 0x0000_0020;
/// Append-only.
pub const GFS2_DIF_APPENDONLY: u32 = 0x0000_0040;
/// No atime updates.
pub const GFS2_DIF_NOATIME: u32 = 0x0000_0080;
/// Sync writes only.
pub const GFS2_DIF_SYNC: u32 = 0x0000_0100;
/// Has system-attribute extension.
pub const GFS2_DIF_SYSTEM: u32 = 0x0000_0200;
/// Trunc in progress.
pub const GFS2_DIF_TRUNC_IN_PROG: u32 = 0x2000_0000;
/// Has inherited journaled-data hint.
pub const GFS2_DIF_INHERIT_JDATA: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// Resource-group flag bits (struct gfs2_rgrp.rg_flags)
// ---------------------------------------------------------------------------

/// Resource group is in journaled space.
pub const GFS2_RGF_JOURNAL: u32 = 0x0000_0001;
/// Resource group on its own metadata.
pub const GFS2_RGF_METAONLY: u32 = 0x0000_0002;
/// Resource group on data only.
pub const GFS2_RGF_DATAONLY: u32 = 0x0000_0004;
/// Trim in progress.
pub const GFS2_RGF_NOALLOC: u32 = 0x0000_0008;
/// Trim history flag.
pub const GFS2_RGF_TRIMMED: u32 = 0x0000_0010;

// ---------------------------------------------------------------------------
// Filesystem block-size limits
// ---------------------------------------------------------------------------

/// Minimum block size.
pub const GFS2_MIN_BSIZE: u32 = 512;
/// Maximum block size.
pub const GFS2_MAX_BSIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_constant() {
        assert_eq!(GFS2_MAGIC, 0x0161_1970);
    }

    #[test]
    fn test_format_versions_sane() {
        assert!(GFS2_FORMAT_FS >= 1801);
        assert!(GFS2_FORMAT_MULTI >= 1900);
        assert!(GFS2_FORMAT_RG >= 1002);
        assert!(GFS2_FORMAT_DI >= 1201);
        assert!(GFS2_FORMAT_IN >= 1300);
        assert!(GFS2_FORMAT_LF >= 1400);
    }

    #[test]
    fn test_metatypes_distinct() {
        let types = [
            GFS2_METATYPE_NONE,
            GFS2_METATYPE_SB,
            GFS2_METATYPE_RG,
            GFS2_METATYPE_RB,
            GFS2_METATYPE_DI,
            GFS2_METATYPE_IN,
            GFS2_METATYPE_LF,
            GFS2_METATYPE_JD,
            GFS2_METATYPE_LH,
            GFS2_METATYPE_LB,
            GFS2_METATYPE_EA,
            GFS2_METATYPE_ED,
            GFS2_METATYPE_QC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dif_flags_distinct_bits() {
        let flags = [
            GFS2_DIF_JDATA,
            GFS2_DIF_EXHASH,
            GFS2_DIF_UNUSED,
            GFS2_DIF_EA_INDIRECT,
            GFS2_DIF_DIRECTIO,
            GFS2_DIF_IMMUTABLE,
            GFS2_DIF_APPENDONLY,
            GFS2_DIF_NOATIME,
            GFS2_DIF_SYNC,
            GFS2_DIF_SYSTEM,
            GFS2_DIF_TRUNC_IN_PROG,
            GFS2_DIF_INHERIT_JDATA,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two(), "{f:#x} not single-bit");
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_rgf_flags_distinct_bits() {
        let flags = [
            GFS2_RGF_JOURNAL,
            GFS2_RGF_METAONLY,
            GFS2_RGF_DATAONLY,
            GFS2_RGF_NOALLOC,
            GFS2_RGF_TRIMMED,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_bsize_bounds_sane() {
        assert!(GFS2_MIN_BSIZE.is_power_of_two());
        assert!(GFS2_MAX_BSIZE.is_power_of_two());
        assert!(GFS2_MIN_BSIZE <= GFS2_MAX_BSIZE);
    }
}
