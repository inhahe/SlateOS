//! `<linux/rbd_types.h>` — Ceph RBD (RADOS Block Device) constants.
//!
//! RBD constants covering feature flags, image flags,
//! operation types, and snap management.

// ---------------------------------------------------------------------------
// RBD feature flags
// ---------------------------------------------------------------------------

/// Layering (clones).
pub const RBD_FEATURE_LAYERING: u64 = 1 << 0;
/// Striping v2.
pub const RBD_FEATURE_STRIPINGV2: u64 = 1 << 1;
/// Exclusive lock.
pub const RBD_FEATURE_EXCLUSIVE_LOCK: u64 = 1 << 2;
/// Object map.
pub const RBD_FEATURE_OBJECT_MAP: u64 = 1 << 3;
/// Fast diff.
pub const RBD_FEATURE_FAST_DIFF: u64 = 1 << 4;
/// Deep flatten.
pub const RBD_FEATURE_DEEP_FLATTEN: u64 = 1 << 5;
/// Journaling.
pub const RBD_FEATURE_JOURNALING: u64 = 1 << 6;
/// Data pool.
pub const RBD_FEATURE_DATA_POOL: u64 = 1 << 7;
/// Operations feature.
pub const RBD_FEATURE_OPERATIONS: u64 = 1 << 8;
/// Migrating.
pub const RBD_FEATURE_MIGRATING: u64 = 1 << 9;
/// Non-primary.
pub const RBD_FEATURE_NON_PRIMARY: u64 = 1 << 10;

// ---------------------------------------------------------------------------
// RBD image flags
// ---------------------------------------------------------------------------

/// Image is flattening.
pub const RBD_FLAG_OBJECT_MAP_INVALID: u32 = 1 << 0;
/// Fast diff invalid.
pub const RBD_FLAG_FAST_DIFF_INVALID: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// RBD operation types
// ---------------------------------------------------------------------------

/// Flatten operation.
pub const RBD_OPERATION_FLATTEN: u32 = 0;
/// Resize operation.
pub const RBD_OPERATION_RESIZE: u32 = 1;
/// Snap create.
pub const RBD_OPERATION_SNAP_CREATE: u32 = 2;
/// Snap remove.
pub const RBD_OPERATION_SNAP_REMOVE: u32 = 3;
/// Snap rename.
pub const RBD_OPERATION_SNAP_RENAME: u32 = 4;
/// Snap rollback.
pub const RBD_OPERATION_SNAP_ROLLBACK: u32 = 5;
/// Snap protect.
pub const RBD_OPERATION_SNAP_PROTECT: u32 = 6;
/// Snap unprotect.
pub const RBD_OPERATION_SNAP_UNPROTECT: u32 = 7;
/// Clone operation.
pub const RBD_OPERATION_CLONE: u32 = 8;
/// Rename operation.
pub const RBD_OPERATION_RENAME: u32 = 9;
/// Mirror enable.
pub const RBD_OPERATION_MIRROR_ENABLE: u32 = 10;
/// Mirror disable.
pub const RBD_OPERATION_MIRROR_DISABLE: u32 = 11;

// ---------------------------------------------------------------------------
// RBD snap states
// ---------------------------------------------------------------------------

/// Normal snapshot.
pub const RBD_SNAP_STATE_NORMAL: u8 = 0;
/// Trash snapshot.
pub const RBD_SNAP_STATE_TRASH: u8 = 1;
/// Unprotected snapshot.
pub const RBD_SNAP_PROTECTION_UNPROTECTED: u8 = 0;
/// Protected snapshot.
pub const RBD_SNAP_PROTECTION_PROTECTED: u8 = 1;
/// Unprotecting snapshot.
pub const RBD_SNAP_PROTECTION_UNPROTECTING: u8 = 2;

// ---------------------------------------------------------------------------
// RBD mirror modes
// ---------------------------------------------------------------------------

/// Mirror disabled.
pub const RBD_MIRROR_MODE_DISABLED: u32 = 0;
/// Mirror per image.
pub const RBD_MIRROR_MODE_IMAGE: u32 = 1;
/// Mirror per pool.
pub const RBD_MIRROR_MODE_POOL: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_power_of_two() {
        let features = [
            RBD_FEATURE_LAYERING,
            RBD_FEATURE_STRIPINGV2,
            RBD_FEATURE_EXCLUSIVE_LOCK,
            RBD_FEATURE_OBJECT_MAP,
            RBD_FEATURE_FAST_DIFF,
            RBD_FEATURE_DEEP_FLATTEN,
            RBD_FEATURE_JOURNALING,
            RBD_FEATURE_DATA_POOL,
            RBD_FEATURE_OPERATIONS,
            RBD_FEATURE_MIGRATING,
            RBD_FEATURE_NON_PRIMARY,
        ];
        for f in &features {
            assert!(f.is_power_of_two(), "0x{:016x} not power of two", f);
        }
    }

    #[test]
    fn test_features_no_overlap() {
        let features = [
            RBD_FEATURE_LAYERING,
            RBD_FEATURE_STRIPINGV2,
            RBD_FEATURE_EXCLUSIVE_LOCK,
            RBD_FEATURE_OBJECT_MAP,
            RBD_FEATURE_FAST_DIFF,
            RBD_FEATURE_DEEP_FLATTEN,
            RBD_FEATURE_JOURNALING,
            RBD_FEATURE_DATA_POOL,
            RBD_FEATURE_OPERATIONS,
            RBD_FEATURE_MIGRATING,
            RBD_FEATURE_NON_PRIMARY,
        ];
        for i in 0..features.len() {
            for j in (i + 1)..features.len() {
                assert_eq!(features[i] & features[j], 0);
            }
        }
    }

    #[test]
    fn test_operations_distinct() {
        let ops = [
            RBD_OPERATION_FLATTEN,
            RBD_OPERATION_RESIZE,
            RBD_OPERATION_SNAP_CREATE,
            RBD_OPERATION_SNAP_REMOVE,
            RBD_OPERATION_SNAP_RENAME,
            RBD_OPERATION_SNAP_ROLLBACK,
            RBD_OPERATION_SNAP_PROTECT,
            RBD_OPERATION_SNAP_UNPROTECT,
            RBD_OPERATION_CLONE,
            RBD_OPERATION_RENAME,
            RBD_OPERATION_MIRROR_ENABLE,
            RBD_OPERATION_MIRROR_DISABLE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_snap_protection_distinct() {
        let prots: [u8; 3] = [
            RBD_SNAP_PROTECTION_UNPROTECTED,
            RBD_SNAP_PROTECTION_PROTECTED,
            RBD_SNAP_PROTECTION_UNPROTECTING,
        ];
        for i in 0..prots.len() {
            for j in (i + 1)..prots.len() {
                assert_ne!(prots[i], prots[j]);
            }
        }
    }

    #[test]
    fn test_mirror_modes_distinct() {
        let modes = [
            RBD_MIRROR_MODE_DISABLED,
            RBD_MIRROR_MODE_IMAGE,
            RBD_MIRROR_MODE_POOL,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
