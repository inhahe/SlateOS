//! `<linux/gfs2_ondisk.h>` — GFS2 (Global File System 2) constants.
//!
//! GFS2 is a shared-disk cluster filesystem from Red Hat.
//! These constants define magic numbers, metadata types,
//! and DLM lock modes.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// GFS2 superblock magic.
pub const GFS2_MAGIC: u32 = 0x01161970;
/// Linux VFS super magic.
pub const GFS2_SUPER_MAGIC: u32 = 0x01161970;

// ---------------------------------------------------------------------------
// Metadata types (GFS2_METATYPE_*)
// ---------------------------------------------------------------------------

/// No type.
pub const GFS2_METATYPE_NONE: u32 = 0;
/// Superblock.
pub const GFS2_METATYPE_SB: u32 = 1;
/// Resource group.
pub const GFS2_METATYPE_RG: u32 = 2;
/// Resource group bitmap.
pub const GFS2_METATYPE_RB: u32 = 3;
/// Dinode.
pub const GFS2_METATYPE_DI: u32 = 4;
/// Indirect block.
pub const GFS2_METATYPE_IN: u32 = 5;
/// Leaf directory.
pub const GFS2_METATYPE_LF: u32 = 6;
/// Journal header.
pub const GFS2_METATYPE_JD: u32 = 7;
/// Log header.
pub const GFS2_METATYPE_LH: u32 = 8;
/// Log block.
pub const GFS2_METATYPE_LB: u32 = 9;
/// Extended attribute.
pub const GFS2_METATYPE_EA: u32 = 10;
/// Ext attr indirect.
pub const GFS2_METATYPE_ED: u32 = 11;
/// Quota change.
pub const GFS2_METATYPE_QC: u32 = 14;

// ---------------------------------------------------------------------------
// Format numbers
// ---------------------------------------------------------------------------

/// Superblock format.
pub const GFS2_FORMAT_SB: u32 = 100;
/// Resource group format.
pub const GFS2_FORMAT_RG: u32 = 200;
/// Resource group bitmap format.
pub const GFS2_FORMAT_RB: u32 = 300;
/// Dinode format.
pub const GFS2_FORMAT_DI: u32 = 400;
/// Indirect format.
pub const GFS2_FORMAT_IN: u32 = 500;
/// Leaf format.
pub const GFS2_FORMAT_LF: u32 = 600;
/// Journal data format.
pub const GFS2_FORMAT_JD: u32 = 700;
/// Log header format.
pub const GFS2_FORMAT_LH: u32 = 800;
/// Log block format.
pub const GFS2_FORMAT_LB: u32 = 900;

// ---------------------------------------------------------------------------
// DLM lock modes
// ---------------------------------------------------------------------------

/// No lock.
pub const GFS2_LKS_UNLOCKED: u32 = 0;
/// Null lock.
pub const GFS2_LKS_NULL: u32 = 1;
/// Concurrent read.
pub const GFS2_LKS_CR: u32 = 2;
/// Concurrent write.
pub const GFS2_LKS_CW: u32 = 3;
/// Protected read.
pub const GFS2_LKS_PR: u32 = 4;
/// Protected write.
pub const GFS2_LKS_PW: u32 = 5;
/// Exclusive.
pub const GFS2_LKS_EX: u32 = 6;

// ---------------------------------------------------------------------------
// Dinode flags
// ---------------------------------------------------------------------------

/// Journaled data.
pub const GFS2_DIF_JDATA: u32 = 0x00000001;
/// Exhash directory.
pub const GFS2_DIF_EXHASH: u32 = 0x00000002;
/// Inode is unused.
pub const GFS2_DIF_EA_INDIRECT: u32 = 0x00000004;
/// Immutable.
pub const GFS2_DIF_IMMUTABLE: u32 = 0x00000020;
/// Append only.
pub const GFS2_DIF_APPENDONLY: u32 = 0x00000040;
/// No atime.
pub const GFS2_DIF_NOATIME: u32 = 0x00000080;
/// System file.
pub const GFS2_DIF_SYSTEM: u32 = 0x00000100;
/// Trunc in progress.
pub const GFS2_DIF_TRUNC_IN_PROG: u32 = 0x20000000;
/// Inherit jdata.
pub const GFS2_DIF_INHERIT_JDATA: u32 = 0x40000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(GFS2_MAGIC, 0x01161970);
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
    fn test_formats_distinct() {
        let fmts = [
            GFS2_FORMAT_SB,
            GFS2_FORMAT_RG,
            GFS2_FORMAT_RB,
            GFS2_FORMAT_DI,
            GFS2_FORMAT_IN,
            GFS2_FORMAT_LF,
            GFS2_FORMAT_JD,
            GFS2_FORMAT_LH,
            GFS2_FORMAT_LB,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_lock_modes_sequential() {
        assert_eq!(GFS2_LKS_UNLOCKED, 0);
        assert_eq!(GFS2_LKS_NULL, 1);
        assert_eq!(GFS2_LKS_EX, 6);
    }

    #[test]
    fn test_dinode_flags_distinct() {
        let flags = [
            GFS2_DIF_JDATA,
            GFS2_DIF_EXHASH,
            GFS2_DIF_EA_INDIRECT,
            GFS2_DIF_IMMUTABLE,
            GFS2_DIF_APPENDONLY,
            GFS2_DIF_NOATIME,
            GFS2_DIF_SYSTEM,
            GFS2_DIF_TRUNC_IN_PROG,
            GFS2_DIF_INHERIT_JDATA,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
