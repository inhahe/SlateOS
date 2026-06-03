//! `<xfs/xfs_fs.h>` — XFS-specific ioctls and on-disk constants.
//!
//! XFS is the default filesystem on RHEL and a top choice for large
//! filesystems on Debian/SUSE. Userspace tools (`xfs_io`, `mkfs.xfs`,
//! `xfs_growfs`, `xfs_quota`) use these ioctls for preallocation,
//! reflink, quotas, and bulk inode iteration.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// `XFSB` — superblock magic (`xfs_sb_t.sb_magicnum`).
pub const XFS_SB_MAGIC: u32 =
    (b'X' as u32) << 24 | (b'F' as u32) << 16 | (b'S' as u32) << 8 | (b'B' as u32);

/// Magic value seen on the FUSE-passthrough variant.
pub const XFS_AGF_MAGIC: u32 =
    (b'X' as u32) << 24 | (b'A' as u32) << 16 | (b'G' as u32) << 8 | (b'F' as u32);

// ---------------------------------------------------------------------------
// Block-size limits
// ---------------------------------------------------------------------------

pub const XFS_MIN_BLOCKSIZE: usize = 512;
pub const XFS_MAX_BLOCKSIZE: usize = 65_536;
pub const XFS_DEFAULT_BLOCKSIZE: usize = 4096;

/// XFS inodes are always 512 B (the legacy minimum) or larger; default is 512.
pub const XFS_DEFAULT_INODE_SIZE: usize = 512;
pub const XFS_MAX_INODE_SIZE: usize = 2048;

// ---------------------------------------------------------------------------
// `XFS_IOC_*` ioctl numbers (subset of the most common ones)
// ---------------------------------------------------------------------------

pub const XFS_IOC_ALLOCSP: u32 = 0x4020_5810;
pub const XFS_IOC_FREESP: u32 = 0x4020_5811;
pub const XFS_IOC_DIOINFO: u32 = 0x800C_5820;
pub const XFS_IOC_FSGEOMETRY_V1: u32 = 0x803C_5803;
pub const XFS_IOC_FSGROWFSDATA: u32 = 0x4020_5828;
pub const XFS_IOC_FSGROWFSLOG: u32 = 0x4008_5829;
pub const XFS_IOC_FSGROWFSRT: u32 = 0x4018_582A;
pub const XFS_IOC_FSCOUNTS: u32 = 0x8018_5825;
pub const XFS_IOC_SET_RESBLKS: u32 = 0xC010_5827;
pub const XFS_IOC_GET_RESBLKS: u32 = 0x8010_5826;
pub const XFS_IOC_PATH_TO_FSHANDLE: u32 = 0xC010_5807;
pub const XFS_IOC_PATH_TO_HANDLE: u32 = 0xC010_5808;
pub const XFS_IOC_GETBMAP: u32 = 0xC020_580D;

// ---------------------------------------------------------------------------
// `XFS_XFLAG_*` extended attributes (`struct fsxattr.fsx_xflags`)
// ---------------------------------------------------------------------------

pub const XFS_XFLAG_REALTIME: u32 = 0x0000_0001;
pub const XFS_XFLAG_PREALLOC: u32 = 0x0000_0002;
pub const XFS_XFLAG_IMMUTABLE: u32 = 0x0000_0008;
pub const XFS_XFLAG_APPEND: u32 = 0x0000_0010;
pub const XFS_XFLAG_SYNC: u32 = 0x0000_0020;
pub const XFS_XFLAG_NOATIME: u32 = 0x0000_0040;
pub const XFS_XFLAG_NODUMP: u32 = 0x0000_0080;
pub const XFS_XFLAG_RTINHERIT: u32 = 0x0000_0100;
pub const XFS_XFLAG_PROJINHERIT: u32 = 0x0000_0200;
pub const XFS_XFLAG_NOSYMLINKS: u32 = 0x0000_0400;
pub const XFS_XFLAG_EXTSIZE: u32 = 0x0000_0800;
pub const XFS_XFLAG_EXTSZINHERIT: u32 = 0x0000_1000;
pub const XFS_XFLAG_NODEFRAG: u32 = 0x0000_2000;
pub const XFS_XFLAG_FILESTREAM: u32 = 0x0000_4000;
pub const XFS_XFLAG_DAX: u32 = 0x0000_8000;
pub const XFS_XFLAG_HASATTR: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Quota types passed to Q_XQUOTAON/OFF/GETQUOTA
// ---------------------------------------------------------------------------

pub const XFS_USER_QUOTA: u32 = 0x01;
pub const XFS_PROJ_QUOTA: u32 = 0x02;
pub const XFS_GROUP_QUOTA: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sb_magic_spells_xfsb() {
        // 'X' 'F' 'S' 'B' big-endian.
        assert_eq!(XFS_SB_MAGIC.to_be_bytes(), *b"XFSB");
        assert_eq!(XFS_AGF_MAGIC.to_be_bytes(), *b"XAGF");
    }

    #[test]
    fn test_blocksize_window() {
        // 512 B .. 64 KiB, default 4 KiB.
        assert_eq!(XFS_MIN_BLOCKSIZE, 512);
        assert_eq!(XFS_MAX_BLOCKSIZE, 65_536);
        assert_eq!(XFS_DEFAULT_BLOCKSIZE, 4096);
        for v in [XFS_MIN_BLOCKSIZE, XFS_MAX_BLOCKSIZE, XFS_DEFAULT_BLOCKSIZE] {
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_inode_size_bounds() {
        assert_eq!(XFS_DEFAULT_INODE_SIZE, 512);
        assert_eq!(XFS_MAX_INODE_SIZE, 2048);
        assert!(XFS_DEFAULT_INODE_SIZE < XFS_MAX_INODE_SIZE);
    }

    #[test]
    fn test_ioc_numbers_in_xfs_namespace() {
        // All XFS ioctls share the 'X' (0x58) magic in the low byte of
        // the type field (bits 8..15).
        let i = [
            XFS_IOC_ALLOCSP,
            XFS_IOC_FREESP,
            XFS_IOC_DIOINFO,
            XFS_IOC_FSGEOMETRY_V1,
            XFS_IOC_FSGROWFSDATA,
            XFS_IOC_FSGROWFSLOG,
            XFS_IOC_FSGROWFSRT,
            XFS_IOC_FSCOUNTS,
            XFS_IOC_SET_RESBLKS,
            XFS_IOC_GET_RESBLKS,
            XFS_IOC_PATH_TO_FSHANDLE,
            XFS_IOC_PATH_TO_HANDLE,
            XFS_IOC_GETBMAP,
        ];
        for v in i {
            assert_eq!((v >> 8) & 0xFF, 0x58);
        }
    }

    #[test]
    fn test_xflag_bits_single_or_top() {
        let f = [
            XFS_XFLAG_REALTIME,
            XFS_XFLAG_PREALLOC,
            XFS_XFLAG_IMMUTABLE,
            XFS_XFLAG_APPEND,
            XFS_XFLAG_SYNC,
            XFS_XFLAG_NOATIME,
            XFS_XFLAG_NODUMP,
            XFS_XFLAG_RTINHERIT,
            XFS_XFLAG_PROJINHERIT,
            XFS_XFLAG_NOSYMLINKS,
            XFS_XFLAG_EXTSIZE,
            XFS_XFLAG_EXTSZINHERIT,
            XFS_XFLAG_NODEFRAG,
            XFS_XFLAG_FILESTREAM,
            XFS_XFLAG_DAX,
            XFS_XFLAG_HASATTR,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // HASATTR sits at the top bit because it's a derived/output
        // flag, not a writable user-settable bit.
        assert_eq!(XFS_XFLAG_HASATTR, 1 << 31);
    }

    #[test]
    fn test_quota_types_low_3_bits() {
        let q = [XFS_USER_QUOTA, XFS_PROJ_QUOTA, XFS_GROUP_QUOTA];
        let mut or = 0u32;
        for &v in &q {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x07);
    }
}
