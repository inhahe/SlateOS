//! `<linux/fs.h>` / `<xfs/xfs_fs.h>` — Additional XFS filesystem constants.
//!
//! Supplementary XFS constants covering geometry flags,
//! quota types, extent flags, and allocation hints.

// ---------------------------------------------------------------------------
// XFS geometry flags (XFS_FSOP_GEOM_FLAGS_*)
// ---------------------------------------------------------------------------

/// Has attribute fork.
pub const XFS_FSOP_GEOM_FLAGS_ATTR: u32 = 0x0001;
/// Has 32-bit nlink.
pub const XFS_FSOP_GEOM_FLAGS_NLINK: u32 = 0x0002;
/// Has quota support.
pub const XFS_FSOP_GEOM_FLAGS_QUOTA: u32 = 0x0004;
/// Aligned inodes.
pub const XFS_FSOP_GEOM_FLAGS_IALIGN: u32 = 0x0008;
/// Uses dalign.
pub const XFS_FSOP_GEOM_FLAGS_DALIGN: u32 = 0x0010;
/// Shared (obsolete).
pub const XFS_FSOP_GEOM_FLAGS_SHARED: u32 = 0x0020;
/// Has extended flags.
pub const XFS_FSOP_GEOM_FLAGS_EXTFLG: u32 = 0x0040;
/// Directory v2 format.
pub const XFS_FSOP_GEOM_FLAGS_DIRV2: u32 = 0x0080;
/// Log stripe unit.
pub const XFS_FSOP_GEOM_FLAGS_LOGV2: u32 = 0x0100;
/// Sector size.
pub const XFS_FSOP_GEOM_FLAGS_SECTOR: u32 = 0x0200;
/// Supports attrs v2.
pub const XFS_FSOP_GEOM_FLAGS_ATTR2: u32 = 0x0400;
/// Projid32bit.
pub const XFS_FSOP_GEOM_FLAGS_PROJID32: u32 = 0x0800;
/// CRC enabled.
pub const XFS_FSOP_GEOM_FLAGS_V5SB: u32 = 0x1000;
/// Finobt enabled.
pub const XFS_FSOP_GEOM_FLAGS_FTYPE: u32 = 0x2000;
/// Finobt.
pub const XFS_FSOP_GEOM_FLAGS_FINOBT: u32 = 0x4000;
/// Sparse inodes.
pub const XFS_FSOP_GEOM_FLAGS_SPINODES: u32 = 0x8000;
/// Rmapbt enabled.
pub const XFS_FSOP_GEOM_FLAGS_RMAPBT: u32 = 0x10000;
/// Reflink enabled.
pub const XFS_FSOP_GEOM_FLAGS_REFLINK: u32 = 0x20000;
/// Big timestamps.
pub const XFS_FSOP_GEOM_FLAGS_BIGTIME: u32 = 0x40000;
/// Inobtcount.
pub const XFS_FSOP_GEOM_FLAGS_INOBTCNT: u32 = 0x80000;
/// Needsrepair.
pub const XFS_FSOP_GEOM_FLAGS_NREXT64: u32 = 0x100000;

// ---------------------------------------------------------------------------
// XFS quota types (XFS_DQ_*)
// ---------------------------------------------------------------------------

/// User quota.
pub const XFS_DQ_USER: u32 = 0x0001;
/// Project quota.
pub const XFS_DQ_PROJ: u32 = 0x0002;
/// Group quota.
pub const XFS_DQ_GROUP: u32 = 0x0004;

// ---------------------------------------------------------------------------
// XFS extent flags
// ---------------------------------------------------------------------------

/// Normal written extent.
pub const XFS_EXT_NORM: u32 = 0;
/// Unwritten (preallocated) extent.
pub const XFS_EXT_UNWRITTEN: u32 = 1;

// ---------------------------------------------------------------------------
// XFS allocation hints (XFS_XFLAG_*)
// ---------------------------------------------------------------------------

/// Realtime device.
pub const XFS_XFLAG_REALTIME: u32 = 0x00000001;
/// Prealloc extent (immutable).
pub const XFS_XFLAG_PREALLOC: u32 = 0x00000002;
/// Immutable file.
pub const XFS_XFLAG_IMMUTABLE: u32 = 0x00000008;
/// Append-only file.
pub const XFS_XFLAG_APPEND: u32 = 0x00000010;
/// Sync updates.
pub const XFS_XFLAG_SYNC: u32 = 0x00000020;
/// Noatime.
pub const XFS_XFLAG_NOATIME: u32 = 0x00000040;
/// Nodump.
pub const XFS_XFLAG_NODUMP: u32 = 0x00000080;
/// Realtime extent size hint.
pub const XFS_XFLAG_RTINHERIT: u32 = 0x00000100;
/// Projid inherit.
pub const XFS_XFLAG_PROJINHERIT: u32 = 0x00000200;
/// No symlink.
pub const XFS_XFLAG_NOSYMLINKS: u32 = 0x00000400;
/// Extent size hint.
pub const XFS_XFLAG_EXTSIZE: u32 = 0x00000800;
/// Extent size inherit.
pub const XFS_XFLAG_EXTSZINHERIT: u32 = 0x00001000;
/// No defrag.
pub const XFS_XFLAG_NODEFRAG: u32 = 0x00002000;
/// File stream.
pub const XFS_XFLAG_FILESTREAM: u32 = 0x00004000;
/// DAX (direct access).
pub const XFS_XFLAG_DAX: u32 = 0x00008000;
/// CoW extent size hint.
pub const XFS_XFLAG_COWEXTSIZE: u32 = 0x00010000;
/// Has attribute.
pub const XFS_XFLAG_HASATTR: u32 = 0x80000000;

// ---------------------------------------------------------------------------
// XFS max values
// ---------------------------------------------------------------------------

/// Maximum AG (allocation group) count.
pub const XFS_MAX_AGNUMBER: u32 = 0xFFFFFFFE;
/// Maximum extent length (in blocks).
pub const XFS_MAX_EXTLEN: u32 = 0x001FFFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geom_flags_power_of_two() {
        let flags = [
            XFS_FSOP_GEOM_FLAGS_ATTR, XFS_FSOP_GEOM_FLAGS_NLINK,
            XFS_FSOP_GEOM_FLAGS_QUOTA, XFS_FSOP_GEOM_FLAGS_IALIGN,
            XFS_FSOP_GEOM_FLAGS_DALIGN, XFS_FSOP_GEOM_FLAGS_SHARED,
            XFS_FSOP_GEOM_FLAGS_EXTFLG, XFS_FSOP_GEOM_FLAGS_DIRV2,
            XFS_FSOP_GEOM_FLAGS_LOGV2, XFS_FSOP_GEOM_FLAGS_SECTOR,
            XFS_FSOP_GEOM_FLAGS_ATTR2, XFS_FSOP_GEOM_FLAGS_PROJID32,
            XFS_FSOP_GEOM_FLAGS_V5SB, XFS_FSOP_GEOM_FLAGS_FTYPE,
            XFS_FSOP_GEOM_FLAGS_FINOBT, XFS_FSOP_GEOM_FLAGS_SPINODES,
            XFS_FSOP_GEOM_FLAGS_RMAPBT, XFS_FSOP_GEOM_FLAGS_REFLINK,
            XFS_FSOP_GEOM_FLAGS_BIGTIME, XFS_FSOP_GEOM_FLAGS_INOBTCNT,
            XFS_FSOP_GEOM_FLAGS_NREXT64,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:x} not power of two", f);
        }
    }

    #[test]
    fn test_geom_flags_distinct() {
        let flags = [
            XFS_FSOP_GEOM_FLAGS_ATTR, XFS_FSOP_GEOM_FLAGS_NLINK,
            XFS_FSOP_GEOM_FLAGS_QUOTA, XFS_FSOP_GEOM_FLAGS_IALIGN,
            XFS_FSOP_GEOM_FLAGS_DALIGN, XFS_FSOP_GEOM_FLAGS_SHARED,
            XFS_FSOP_GEOM_FLAGS_EXTFLG, XFS_FSOP_GEOM_FLAGS_DIRV2,
            XFS_FSOP_GEOM_FLAGS_LOGV2, XFS_FSOP_GEOM_FLAGS_SECTOR,
            XFS_FSOP_GEOM_FLAGS_ATTR2, XFS_FSOP_GEOM_FLAGS_PROJID32,
            XFS_FSOP_GEOM_FLAGS_V5SB, XFS_FSOP_GEOM_FLAGS_FTYPE,
            XFS_FSOP_GEOM_FLAGS_FINOBT, XFS_FSOP_GEOM_FLAGS_SPINODES,
            XFS_FSOP_GEOM_FLAGS_RMAPBT, XFS_FSOP_GEOM_FLAGS_REFLINK,
            XFS_FSOP_GEOM_FLAGS_BIGTIME, XFS_FSOP_GEOM_FLAGS_INOBTCNT,
            XFS_FSOP_GEOM_FLAGS_NREXT64,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_quota_types_power_of_two() {
        let types = [XFS_DQ_USER, XFS_DQ_PROJ, XFS_DQ_GROUP];
        for t in &types {
            assert!(t.is_power_of_two(), "{} not power of two", t);
        }
    }

    #[test]
    fn test_quota_no_overlap() {
        assert_eq!(XFS_DQ_USER & XFS_DQ_PROJ, 0);
        assert_eq!(XFS_DQ_USER & XFS_DQ_GROUP, 0);
        assert_eq!(XFS_DQ_PROJ & XFS_DQ_GROUP, 0);
    }

    #[test]
    fn test_extent_flags() {
        assert_eq!(XFS_EXT_NORM, 0);
        assert_eq!(XFS_EXT_UNWRITTEN, 1);
    }

    #[test]
    fn test_xflags_distinct() {
        let flags = [
            XFS_XFLAG_REALTIME, XFS_XFLAG_PREALLOC,
            XFS_XFLAG_IMMUTABLE, XFS_XFLAG_APPEND,
            XFS_XFLAG_SYNC, XFS_XFLAG_NOATIME,
            XFS_XFLAG_NODUMP, XFS_XFLAG_RTINHERIT,
            XFS_XFLAG_PROJINHERIT, XFS_XFLAG_NOSYMLINKS,
            XFS_XFLAG_EXTSIZE, XFS_XFLAG_EXTSZINHERIT,
            XFS_XFLAG_NODEFRAG, XFS_XFLAG_FILESTREAM,
            XFS_XFLAG_DAX, XFS_XFLAG_COWEXTSIZE,
            XFS_XFLAG_HASATTR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_max_values() {
        assert_eq!(XFS_MAX_AGNUMBER, 0xFFFFFFFE);
        assert_eq!(XFS_MAX_EXTLEN, 0x001FFFFF);
    }
}
