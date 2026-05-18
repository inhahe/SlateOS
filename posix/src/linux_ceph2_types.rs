//! `<linux/ceph/ceph_fs.h>` — Additional Ceph filesystem constants.
//!
//! Supplementary Ceph constants covering MDS operations,
//! capability bits, file layout parameters, and snap operations.

// ---------------------------------------------------------------------------
// MDS operation types (CEPH_MDS_OP_*)
// ---------------------------------------------------------------------------

/// Lookup.
pub const CEPH_MDS_OP_LOOKUP: u32 = 0x00100;
/// Lookuphash.
pub const CEPH_MDS_OP_LOOKUPHASH: u32 = 0x00101;
/// Lookupparent.
pub const CEPH_MDS_OP_LOOKUPPARENT: u32 = 0x00102;
/// Lookupino.
pub const CEPH_MDS_OP_LOOKUPINO: u32 = 0x00103;
/// Getattr.
pub const CEPH_MDS_OP_GETATTR: u32 = 0x00200;
/// Setattr.
pub const CEPH_MDS_OP_SETATTR: u32 = 0x00201;
/// Readdir.
pub const CEPH_MDS_OP_READDIR: u32 = 0x00305;
/// Mknod.
pub const CEPH_MDS_OP_MKNOD: u32 = 0x01100;
/// Link.
pub const CEPH_MDS_OP_LINK: u32 = 0x01101;
/// Unlink.
pub const CEPH_MDS_OP_UNLINK: u32 = 0x01102;
/// Rename.
pub const CEPH_MDS_OP_RENAME: u32 = 0x01103;
/// Mkdir.
pub const CEPH_MDS_OP_MKDIR: u32 = 0x01104;
/// Rmdir.
pub const CEPH_MDS_OP_RMDIR: u32 = 0x01105;
/// Symlink.
pub const CEPH_MDS_OP_SYMLINK: u32 = 0x01106;
/// Create.
pub const CEPH_MDS_OP_CREATE: u32 = 0x01301;
/// Open.
pub const CEPH_MDS_OP_OPEN: u32 = 0x01302;
/// Setlayout.
pub const CEPH_MDS_OP_SETLAYOUT: u32 = 0x01500;

// ---------------------------------------------------------------------------
// Capability bits (CEPH_CAP_*)
// ---------------------------------------------------------------------------

/// Pin.
pub const CEPH_CAP_PIN: u32 = 1;
/// Auth shared.
pub const CEPH_CAP_AUTH_SHARED: u32 = 1 << 2;
/// Auth excl.
pub const CEPH_CAP_AUTH_EXCL: u32 = (1 << 2) | (1 << 3);
/// Link shared.
pub const CEPH_CAP_LINK_SHARED: u32 = 1 << 4;
/// Link excl.
pub const CEPH_CAP_LINK_EXCL: u32 = (1 << 4) | (1 << 5);
/// Xattr shared.
pub const CEPH_CAP_XATTR_SHARED: u32 = 1 << 6;
/// Xattr excl.
pub const CEPH_CAP_XATTR_EXCL: u32 = (1 << 6) | (1 << 7);
/// File shared.
pub const CEPH_CAP_FILE_SHARED: u32 = 1 << 8;
/// File excl.
pub const CEPH_CAP_FILE_EXCL: u32 = (1 << 8) | (1 << 9);
/// File cache.
pub const CEPH_CAP_FILE_CACHE: u32 = 1 << 10;
/// File rd.
pub const CEPH_CAP_FILE_RD: u32 = 1 << 11;
/// File wr.
pub const CEPH_CAP_FILE_WR: u32 = 1 << 12;
/// File buffer.
pub const CEPH_CAP_FILE_BUFFER: u32 = 1 << 13;
/// File lazy IO.
pub const CEPH_CAP_FILE_LAZYIO: u32 = 1 << 14;

// ---------------------------------------------------------------------------
// File layout defaults
// ---------------------------------------------------------------------------

/// Default stripe unit (4 MiB).
pub const CEPH_DEFAULT_STRIPE_UNIT: u32 = 4194304;
/// Default stripe count.
pub const CEPH_DEFAULT_STRIPE_COUNT: u32 = 1;
/// Default object size (4 MiB).
pub const CEPH_DEFAULT_OBJECT_SIZE: u32 = 4194304;

// ---------------------------------------------------------------------------
// Snap operations
// ---------------------------------------------------------------------------

/// Create snap.
pub const CEPH_SNAP_OP_CREATE: u32 = 1;
/// Destroy snap.
pub const CEPH_SNAP_OP_DESTROY: u32 = 2;
/// Rename snap.
pub const CEPH_SNAP_OP_RENAME: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mds_ops_distinct() {
        let ops = [
            CEPH_MDS_OP_LOOKUP, CEPH_MDS_OP_LOOKUPHASH,
            CEPH_MDS_OP_LOOKUPPARENT, CEPH_MDS_OP_LOOKUPINO,
            CEPH_MDS_OP_GETATTR, CEPH_MDS_OP_SETATTR,
            CEPH_MDS_OP_READDIR, CEPH_MDS_OP_MKNOD,
            CEPH_MDS_OP_LINK, CEPH_MDS_OP_UNLINK,
            CEPH_MDS_OP_RENAME, CEPH_MDS_OP_MKDIR,
            CEPH_MDS_OP_RMDIR, CEPH_MDS_OP_SYMLINK,
            CEPH_MDS_OP_CREATE, CEPH_MDS_OP_OPEN,
            CEPH_MDS_OP_SETLAYOUT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_cap_pin() {
        assert_eq!(CEPH_CAP_PIN, 1);
    }

    #[test]
    fn test_cap_file_bits_distinct() {
        let caps = [
            CEPH_CAP_FILE_SHARED, CEPH_CAP_FILE_CACHE,
            CEPH_CAP_FILE_RD, CEPH_CAP_FILE_WR,
            CEPH_CAP_FILE_BUFFER, CEPH_CAP_FILE_LAZYIO,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_layout_defaults() {
        assert_eq!(CEPH_DEFAULT_STRIPE_UNIT, 4 * 1024 * 1024);
        assert_eq!(CEPH_DEFAULT_OBJECT_SIZE, 4 * 1024 * 1024);
        assert_eq!(CEPH_DEFAULT_STRIPE_COUNT, 1);
    }

    #[test]
    fn test_snap_ops_sequential() {
        assert_eq!(CEPH_SNAP_OP_CREATE, 1);
        assert_eq!(CEPH_SNAP_OP_DESTROY, 2);
        assert_eq!(CEPH_SNAP_OP_RENAME, 3);
    }
}
