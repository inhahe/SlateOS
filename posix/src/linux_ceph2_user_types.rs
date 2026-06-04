//! `<linux/ceph/rados.h>` (part 2) — RADOS object operation opcodes.
//!
//! RADOS is the underlying object store that Ceph builds on. The
//! kernel client speaks the RADOS wire protocol directly. This module
//! covers the well-known opcode values used by `librados`-style
//! requests issued from the Ceph filesystem and RBD drivers.

// ---------------------------------------------------------------------------
// Operation flag bits (msg.ops[i].flags)
// ---------------------------------------------------------------------------

pub const CEPH_OSD_FLAG_ACK: u32 = 1 << 0;
pub const CEPH_OSD_FLAG_ONNVRAM: u32 = 1 << 1;
pub const CEPH_OSD_FLAG_ONDISK: u32 = 1 << 2;
pub const CEPH_OSD_FLAG_RETRY: u32 = 1 << 3;
pub const CEPH_OSD_FLAG_READ: u32 = 1 << 4;
pub const CEPH_OSD_FLAG_WRITE: u32 = 1 << 5;
pub const CEPH_OSD_FLAG_ORDERSNAP: u32 = 1 << 6;
pub const CEPH_OSD_FLAG_PEERSTAT_OLD: u32 = 1 << 7;
pub const CEPH_OSD_FLAG_BALANCE_READS: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Major RADOS opcodes (categorized — high nibble is class)
// ---------------------------------------------------------------------------

/// Read object payload.
pub const CEPH_OSD_OP_READ: u32 = 0x0001;

/// Stat (existence + size + mtime) check.
pub const CEPH_OSD_OP_STAT: u32 = 0x0002;

/// Map an object extent to objects (RBD use).
pub const CEPH_OSD_OP_MAPEXT: u32 = 0x0003;

/// Sparse read.
pub const CEPH_OSD_OP_SPARSE_READ: u32 = 0x0005;

/// Write object payload.
pub const CEPH_OSD_OP_WRITE: u32 = 0x0201;

/// Write full object (replaces contents).
pub const CEPH_OSD_OP_WRITEFULL: u32 = 0x0202;

/// Truncate object.
pub const CEPH_OSD_OP_TRUNCATE: u32 = 0x0203;

/// Zero a range within the object.
pub const CEPH_OSD_OP_ZERO: u32 = 0x0204;

/// Delete object.
pub const CEPH_OSD_OP_DELETE: u32 = 0x0205;

/// Append data to object tail.
pub const CEPH_OSD_OP_APPEND: u32 = 0x0206;

// ---------------------------------------------------------------------------
// Attribute (xattr) ops
// ---------------------------------------------------------------------------

pub const CEPH_OSD_OP_GETXATTR: u32 = 0x000B;
pub const CEPH_OSD_OP_SETXATTR: u32 = 0x020B;
pub const CEPH_OSD_OP_RMXATTR: u32 = 0x020D;

// ---------------------------------------------------------------------------
// Watch / notify (used by RBD for cooperative locking)
// ---------------------------------------------------------------------------

pub const CEPH_OSD_OP_WATCH: u32 = 0x020F;
pub const CEPH_OSD_OP_NOTIFY: u32 = 0x0210;

// ---------------------------------------------------------------------------
// High-nibble class masks
// ---------------------------------------------------------------------------

/// Mask that selects the op-class nibble in a RADOS opcode.
pub const CEPH_OSD_OP_CLASS_MASK: u32 = 0xFF00;

/// Read class.
pub const CEPH_OSD_OP_CLASS_RD: u32 = 0x0000;

/// Write class.
pub const CEPH_OSD_OP_CLASS_WR: u32 = 0x0200;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_osd_flags_distinct_single_bits() {
        let f = [
            CEPH_OSD_FLAG_ACK,
            CEPH_OSD_FLAG_ONNVRAM,
            CEPH_OSD_FLAG_ONDISK,
            CEPH_OSD_FLAG_RETRY,
            CEPH_OSD_FLAG_READ,
            CEPH_OSD_FLAG_WRITE,
            CEPH_OSD_FLAG_ORDERSNAP,
            CEPH_OSD_FLAG_PEERSTAT_OLD,
            CEPH_OSD_FLAG_BALANCE_READS,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
    }

    #[test]
    fn test_read_ops_in_read_class() {
        for op in [
            CEPH_OSD_OP_READ,
            CEPH_OSD_OP_STAT,
            CEPH_OSD_OP_MAPEXT,
            CEPH_OSD_OP_SPARSE_READ,
            CEPH_OSD_OP_GETXATTR,
        ] {
            assert_eq!(op & CEPH_OSD_OP_CLASS_MASK, CEPH_OSD_OP_CLASS_RD);
        }
    }

    #[test]
    fn test_write_ops_in_write_class() {
        for op in [
            CEPH_OSD_OP_WRITE,
            CEPH_OSD_OP_WRITEFULL,
            CEPH_OSD_OP_TRUNCATE,
            CEPH_OSD_OP_ZERO,
            CEPH_OSD_OP_DELETE,
            CEPH_OSD_OP_APPEND,
            CEPH_OSD_OP_SETXATTR,
            CEPH_OSD_OP_RMXATTR,
            CEPH_OSD_OP_WATCH,
            CEPH_OSD_OP_NOTIFY,
        ] {
            assert_eq!(op & CEPH_OSD_OP_CLASS_MASK, CEPH_OSD_OP_CLASS_WR);
        }
    }

    #[test]
    fn test_class_constants() {
        assert_eq!(CEPH_OSD_OP_CLASS_RD, 0x0000);
        assert_eq!(CEPH_OSD_OP_CLASS_WR, 0x0200);
        // Mask covers only the class nibble (bits 8..15).
        assert_eq!(CEPH_OSD_OP_CLASS_MASK, 0xFF00);
    }

    #[test]
    fn test_write_ops_dense_in_low_nibble() {
        // Within the write class, basic data ops are 0x201..0x206.
        let w = [
            CEPH_OSD_OP_WRITE,
            CEPH_OSD_OP_WRITEFULL,
            CEPH_OSD_OP_TRUNCATE,
            CEPH_OSD_OP_ZERO,
            CEPH_OSD_OP_DELETE,
            CEPH_OSD_OP_APPEND,
        ];
        for (i, &v) in w.iter().enumerate() {
            assert_eq!(v & 0xFF, (i + 1) as u32);
        }
    }

    #[test]
    fn test_read_and_write_attr_use_same_low_nibble() {
        // GET/SET/RM xattr share the low nibble of their class.
        assert_eq!(CEPH_OSD_OP_GETXATTR & 0xFF, 0x0B);
        assert_eq!(CEPH_OSD_OP_SETXATTR & 0xFF, 0x0B);
        // RM is one above SET.
        assert_eq!(CEPH_OSD_OP_RMXATTR - CEPH_OSD_OP_SETXATTR, 2);
    }
}
