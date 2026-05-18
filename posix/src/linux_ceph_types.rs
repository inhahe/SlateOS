//! `<linux/ceph/ceph_fs.h>` — Ceph distributed filesystem constants.
//!
//! Ceph is a distributed storage system.  These constants define
//! Ceph RADOS object operations, OSD opcodes, CRUSH rule types,
//! and client session flags.

// ---------------------------------------------------------------------------
// Ceph OSD operation opcodes (CEPH_OSD_OP_*)
// ---------------------------------------------------------------------------

/// Read.
pub const CEPH_OSD_OP_READ: u16 = 1;
/// Stat (get attributes).
pub const CEPH_OSD_OP_STAT: u16 = 2;
/// Map extent.
pub const CEPH_OSD_OP_MAPEXT: u16 = 3;
/// Sparse read.
pub const CEPH_OSD_OP_SPARSE_READ: u16 = 4;
/// Notify.
pub const CEPH_OSD_OP_NOTIFY: u16 = 5;
/// Notify acknowledge.
pub const CEPH_OSD_OP_NOTIFY_ACK: u16 = 6;
/// Assert existence.
pub const CEPH_OSD_OP_ASSERT_EXISTS: u16 = 7;

/// Write.
pub const CEPH_OSD_OP_WRITE: u16 = 0x1001;
/// Write full object.
pub const CEPH_OSD_OP_WRITEFULL: u16 = 0x1002;
/// Truncate.
pub const CEPH_OSD_OP_TRUNCATE: u16 = 0x1003;
/// Zero fill.
pub const CEPH_OSD_OP_ZERO: u16 = 0x1004;
/// Delete.
pub const CEPH_OSD_OP_DELETE: u16 = 0x1005;
/// Append.
pub const CEPH_OSD_OP_APPEND: u16 = 0x1006;
/// Set truncate sequence.
pub const CEPH_OSD_OP_SETTRUNC: u16 = 0x1008;
/// Create object.
pub const CEPH_OSD_OP_CREATE: u16 = 0x1009;
/// Rollback.
pub const CEPH_OSD_OP_ROLLBACK: u16 = 0x100A;

// ---------------------------------------------------------------------------
// Ceph xattr operations
// ---------------------------------------------------------------------------

/// Get xattr.
pub const CEPH_OSD_OP_GETXATTR: u16 = 0x0301;
/// Get xattrs (all).
pub const CEPH_OSD_OP_GETXATTRS: u16 = 0x0302;
/// Compare xattr.
pub const CEPH_OSD_OP_CMPXATTR: u16 = 0x0303;
/// Set xattr.
pub const CEPH_OSD_OP_SETXATTR: u16 = 0x1304;
/// Set xattrs (all).
pub const CEPH_OSD_OP_SETXATTRS: u16 = 0x1305;
/// Remove xattr.
pub const CEPH_OSD_OP_RMXATTR: u16 = 0x1306;
/// Reset xattrs.
pub const CEPH_OSD_OP_RESETXATTRS: u16 = 0x1307;

// ---------------------------------------------------------------------------
// Ceph CRUSH rule types
// ---------------------------------------------------------------------------

/// No rule.
pub const CRUSH_RULE_NOOP: u32 = 0;
/// Take (root of tree).
pub const CRUSH_RULE_TAKE: u32 = 1;
/// Choose first N.
pub const CRUSH_RULE_CHOOSE_FIRSTN: u32 = 2;
/// Choose indep.
pub const CRUSH_RULE_CHOOSELEAF_FIRSTN: u32 = 3;
/// Emit (output).
pub const CRUSH_RULE_EMIT: u32 = 4;
/// Choose indep leaf.
pub const CRUSH_RULE_CHOOSE_INDEP: u32 = 5;
/// Choose leaf indep.
pub const CRUSH_RULE_CHOOSELEAF_INDEP: u32 = 6;
/// Set choose tries.
pub const CRUSH_RULE_SET_CHOOSE_TRIES: u32 = 8;
/// Set chooseleaf tries.
pub const CRUSH_RULE_SET_CHOOSELEAF_TRIES: u32 = 9;

// ---------------------------------------------------------------------------
// Ceph pool flags
// ---------------------------------------------------------------------------

/// No send OSD map.
pub const CEPH_POOL_FLAG_HASHPSPOOL: u32 = 1 << 0;
/// Full (no writes).
pub const CEPH_POOL_FLAG_FULL: u32 = 1 << 1;
/// Fake EC pool.
pub const CEPH_POOL_FLAG_EC_OVERWRITES: u32 = 1 << 2;
/// Incomplete clones.
pub const CEPH_POOL_FLAG_INCOMPLETE_CLONES: u32 = 1 << 3;
/// No delete.
pub const CEPH_POOL_FLAG_NODELETE: u32 = 1 << 4;
/// No size change.
pub const CEPH_POOL_FLAG_NOPGCHANGE: u32 = 1 << 5;
/// No scrub.
pub const CEPH_POOL_FLAG_NOSCRUB: u32 = 1 << 6;
/// No deep scrub.
pub const CEPH_POOL_FLAG_NODEEP_SCRUB: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Ceph entity types
// ---------------------------------------------------------------------------

/// Client entity.
pub const CEPH_ENTITY_TYPE_CLIENT: u32 = 0x08;
/// Monitor entity.
pub const CEPH_ENTITY_TYPE_MON: u32 = 0x01;
/// MDS entity.
pub const CEPH_ENTITY_TYPE_MDS: u32 = 0x02;
/// OSD entity.
pub const CEPH_ENTITY_TYPE_OSD: u32 = 0x04;
/// Any entity.
pub const CEPH_ENTITY_TYPE_ANY: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_ops_distinct() {
        let ops = [
            CEPH_OSD_OP_READ, CEPH_OSD_OP_STAT,
            CEPH_OSD_OP_MAPEXT, CEPH_OSD_OP_SPARSE_READ,
            CEPH_OSD_OP_NOTIFY, CEPH_OSD_OP_NOTIFY_ACK,
            CEPH_OSD_OP_ASSERT_EXISTS,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_write_ops_distinct() {
        let ops = [
            CEPH_OSD_OP_WRITE, CEPH_OSD_OP_WRITEFULL,
            CEPH_OSD_OP_TRUNCATE, CEPH_OSD_OP_ZERO,
            CEPH_OSD_OP_DELETE, CEPH_OSD_OP_APPEND,
            CEPH_OSD_OP_SETTRUNC, CEPH_OSD_OP_CREATE,
            CEPH_OSD_OP_ROLLBACK,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_xattr_ops_distinct() {
        let ops = [
            CEPH_OSD_OP_GETXATTR, CEPH_OSD_OP_GETXATTRS,
            CEPH_OSD_OP_CMPXATTR, CEPH_OSD_OP_SETXATTR,
            CEPH_OSD_OP_SETXATTRS, CEPH_OSD_OP_RMXATTR,
            CEPH_OSD_OP_RESETXATTRS,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_crush_rules_distinct() {
        let rules = [
            CRUSH_RULE_NOOP, CRUSH_RULE_TAKE,
            CRUSH_RULE_CHOOSE_FIRSTN, CRUSH_RULE_CHOOSELEAF_FIRSTN,
            CRUSH_RULE_EMIT, CRUSH_RULE_CHOOSE_INDEP,
            CRUSH_RULE_CHOOSELEAF_INDEP,
            CRUSH_RULE_SET_CHOOSE_TRIES,
            CRUSH_RULE_SET_CHOOSELEAF_TRIES,
        ];
        for i in 0..rules.len() {
            for j in (i + 1)..rules.len() {
                assert_ne!(rules[i], rules[j]);
            }
        }
    }

    #[test]
    fn test_pool_flags_powers_of_two() {
        let flags = [
            CEPH_POOL_FLAG_HASHPSPOOL, CEPH_POOL_FLAG_FULL,
            CEPH_POOL_FLAG_EC_OVERWRITES,
            CEPH_POOL_FLAG_INCOMPLETE_CLONES,
            CEPH_POOL_FLAG_NODELETE, CEPH_POOL_FLAG_NOPGCHANGE,
            CEPH_POOL_FLAG_NOSCRUB, CEPH_POOL_FLAG_NODEEP_SCRUB,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_pool_flags_no_overlap() {
        let flags = [
            CEPH_POOL_FLAG_HASHPSPOOL, CEPH_POOL_FLAG_FULL,
            CEPH_POOL_FLAG_EC_OVERWRITES,
            CEPH_POOL_FLAG_INCOMPLETE_CLONES,
            CEPH_POOL_FLAG_NODELETE, CEPH_POOL_FLAG_NOPGCHANGE,
            CEPH_POOL_FLAG_NOSCRUB, CEPH_POOL_FLAG_NODEEP_SCRUB,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_entity_types_distinct() {
        let types = [
            CEPH_ENTITY_TYPE_CLIENT, CEPH_ENTITY_TYPE_MON,
            CEPH_ENTITY_TYPE_MDS, CEPH_ENTITY_TYPE_OSD,
            CEPH_ENTITY_TYPE_ANY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_read_is_one() {
        assert_eq!(CEPH_OSD_OP_READ, 1);
    }

    #[test]
    fn test_noop_is_zero() {
        assert_eq!(CRUSH_RULE_NOOP, 0);
    }
}
