//! `<linux/ceph/ceph_fs.h>` — Ceph distributed filesystem constants.
//!
//! Ceph is a distributed storage system providing object, block, and
//! file storage. CephFS is its POSIX-compliant distributed filesystem
//! built atop the RADOS object store, with dynamic subtree partitioning
//! across multiple metadata servers (MDS).

// ---------------------------------------------------------------------------
// Ceph MDS operation codes
// ---------------------------------------------------------------------------

/// Lookup (path resolution).
pub const CEPH_MDS_OP_LOOKUP: u32 = 0x00100;
/// Get attributes.
pub const CEPH_MDS_OP_GETATTR: u32 = 0x00101;
/// Lookup by inode number.
pub const CEPH_MDS_OP_LOOKUPHASH: u32 = 0x00102;
/// Lookup parent.
pub const CEPH_MDS_OP_LOOKUPPARENT: u32 = 0x00103;
/// Lookup by name.
pub const CEPH_MDS_OP_LOOKUPNAME: u32 = 0x00105;
/// Set attributes.
pub const CEPH_MDS_OP_SETATTR: u32 = 0x00201;
/// Set layout.
pub const CEPH_MDS_OP_SETLAYOUT: u32 = 0x00204;
/// Set directory layout.
pub const CEPH_MDS_OP_SETDIRLAYOUT: u32 = 0x00205;
/// Create file.
pub const CEPH_MDS_OP_CREATE: u32 = 0x00301;
/// Open file.
pub const CEPH_MDS_OP_OPEN: u32 = 0x00302;
/// Read directory.
pub const CEPH_MDS_OP_READDIR: u32 = 0x00305;
/// Make directory.
pub const CEPH_MDS_OP_MKDIR: u32 = 0x00400;
/// Remove file.
pub const CEPH_MDS_OP_UNLINK: u32 = 0x00403;
/// Remove directory.
pub const CEPH_MDS_OP_RMDIR: u32 = 0x00404;
/// Rename.
pub const CEPH_MDS_OP_RENAME: u32 = 0x00405;
/// Create symbolic link.
pub const CEPH_MDS_OP_SYMLINK: u32 = 0x00406;
/// Create hard link.
pub const CEPH_MDS_OP_LINK: u32 = 0x00407;
/// Make node.
pub const CEPH_MDS_OP_MKNOD: u32 = 0x00401;

// ---------------------------------------------------------------------------
// Ceph capability (cap) bits
// ---------------------------------------------------------------------------

/// Permit reading file data.
pub const CEPH_CAP_FILE_RD: u32 = 1 << 0;
/// Permit writing file data.
pub const CEPH_CAP_FILE_WR: u32 = 1 << 1;
/// Cache reads (file buffer).
pub const CEPH_CAP_FILE_BUFFER: u32 = 1 << 2;
/// File shared lock.
pub const CEPH_CAP_FILE_SHARED: u32 = 1 << 3;
/// File exclusive lock.
pub const CEPH_CAP_FILE_EXCL: u32 = 1 << 4;
/// Lazy I/O allowed.
pub const CEPH_CAP_FILE_LAZYIO: u32 = 1 << 5;
/// Pin inode in cache.
pub const CEPH_CAP_PIN: u32 = 1 << 8;
/// Auth (permission) capability.
pub const CEPH_CAP_AUTH_SHARED: u32 = 1 << 10;
/// Link (hardlink count) shared.
pub const CEPH_CAP_LINK_SHARED: u32 = 1 << 12;
/// Xattr shared.
pub const CEPH_CAP_XATTR_SHARED: u32 = 1 << 14;

// ---------------------------------------------------------------------------
// Ceph OSD operation codes (subset)
// ---------------------------------------------------------------------------

/// Read from object.
pub const CEPH_OSD_OP_READ: u16 = 1;
/// Write to object.
pub const CEPH_OSD_OP_WRITE: u16 = 2;
/// Write full object (replace).
pub const CEPH_OSD_OP_WRITEFULL: u16 = 3;
/// Truncate object.
pub const CEPH_OSD_OP_TRUNCATE: u16 = 4;
/// Delete object.
pub const CEPH_OSD_OP_DELETE: u16 = 5;
/// Stat object.
pub const CEPH_OSD_OP_STAT: u16 = 13;
/// Append to object.
pub const CEPH_OSD_OP_APPEND: u16 = 6;

// ---------------------------------------------------------------------------
// Ceph file layout stripe constants
// ---------------------------------------------------------------------------

/// Default object size (4 MiB).
pub const CEPH_DEFAULT_OBJECT_SIZE: u32 = 4 * 1024 * 1024;
/// Default stripe unit (4 MiB).
pub const CEPH_DEFAULT_STRIPE_UNIT: u32 = 4 * 1024 * 1024;
/// Default stripe count.
pub const CEPH_DEFAULT_STRIPE_COUNT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mds_ops_distinct() {
        let ops = [
            CEPH_MDS_OP_LOOKUP, CEPH_MDS_OP_GETATTR, CEPH_MDS_OP_LOOKUPHASH,
            CEPH_MDS_OP_LOOKUPPARENT, CEPH_MDS_OP_LOOKUPNAME,
            CEPH_MDS_OP_SETATTR, CEPH_MDS_OP_SETLAYOUT, CEPH_MDS_OP_SETDIRLAYOUT,
            CEPH_MDS_OP_CREATE, CEPH_MDS_OP_OPEN, CEPH_MDS_OP_READDIR,
            CEPH_MDS_OP_MKDIR, CEPH_MDS_OP_UNLINK, CEPH_MDS_OP_RMDIR,
            CEPH_MDS_OP_RENAME, CEPH_MDS_OP_SYMLINK, CEPH_MDS_OP_LINK,
            CEPH_MDS_OP_MKNOD,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_cap_bits_no_overlap() {
        let caps = [
            CEPH_CAP_FILE_RD, CEPH_CAP_FILE_WR, CEPH_CAP_FILE_BUFFER,
            CEPH_CAP_FILE_SHARED, CEPH_CAP_FILE_EXCL, CEPH_CAP_FILE_LAZYIO,
            CEPH_CAP_PIN, CEPH_CAP_AUTH_SHARED, CEPH_CAP_LINK_SHARED,
            CEPH_CAP_XATTR_SHARED,
        ];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_osd_ops_distinct() {
        let ops = [
            CEPH_OSD_OP_READ, CEPH_OSD_OP_WRITE, CEPH_OSD_OP_WRITEFULL,
            CEPH_OSD_OP_TRUNCATE, CEPH_OSD_OP_DELETE, CEPH_OSD_OP_STAT,
            CEPH_OSD_OP_APPEND,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_default_layout_values() {
        assert_eq!(CEPH_DEFAULT_OBJECT_SIZE, 4 * 1024 * 1024);
        assert_eq!(CEPH_DEFAULT_STRIPE_UNIT, 4 * 1024 * 1024);
        assert_eq!(CEPH_DEFAULT_STRIPE_COUNT, 1);
    }
}
