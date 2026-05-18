//! `<linux/blkzoned.h>` — block layer zoned device constants.
//!
//! Zoned block devices (ZBD) have regions (zones) that must be
//! written sequentially — you cannot overwrite data in place, only
//! append. Host-managed SMR (Shingled Magnetic Recording) drives
//! and ZNS (Zoned Namespace) NVMe SSDs use this model. The kernel
//! exposes zone information via ioctls and enforces write ordering.

// ---------------------------------------------------------------------------
// Zone types (blk_zone_type)
// ---------------------------------------------------------------------------

/// Conventional zone: random read/write (like normal disk).
pub const BLK_ZONE_TYPE_CONVENTIONAL: u32 = 1;
/// Sequential write required: must write from write pointer.
pub const BLK_ZONE_TYPE_SEQWRITE_REQ: u32 = 2;
/// Sequential write preferred: sequential recommended but not required.
pub const BLK_ZONE_TYPE_SEQWRITE_PREF: u32 = 3;

// ---------------------------------------------------------------------------
// Zone conditions (blk_zone_cond)
// ---------------------------------------------------------------------------

/// Zone not write-pointer managed.
pub const BLK_ZONE_COND_NOT_WP: u32 = 0x0;
/// Zone is empty (write pointer at start).
pub const BLK_ZONE_COND_EMPTY: u32 = 0x1;
/// Zone is implicitly open (write started).
pub const BLK_ZONE_COND_IMP_OPEN: u32 = 0x2;
/// Zone is explicitly open (opened by command).
pub const BLK_ZONE_COND_EXP_OPEN: u32 = 0x3;
/// Zone is closed (was open, now closed).
pub const BLK_ZONE_COND_CLOSED: u32 = 0x4;
/// Zone is read-only.
pub const BLK_ZONE_COND_READONLY: u32 = 0xD;
/// Zone is full (write pointer at end).
pub const BLK_ZONE_COND_FULL: u32 = 0xE;
/// Zone is offline (unusable).
pub const BLK_ZONE_COND_OFFLINE: u32 = 0xF;

// ---------------------------------------------------------------------------
// Zone management actions (REQ_OP_ZONE_*)
// ---------------------------------------------------------------------------

/// Open a zone.
pub const REQ_OP_ZONE_OPEN: u32 = 13;
/// Close a zone.
pub const REQ_OP_ZONE_CLOSE: u32 = 14;
/// Finish a zone (move write pointer to end).
pub const REQ_OP_ZONE_FINISH: u32 = 15;
/// Reset a zone (move write pointer to start).
pub const REQ_OP_ZONE_RESET: u32 = 17;
/// Append data to a zone.
pub const REQ_OP_ZONE_APPEND: u32 = 19;
/// Reset all zones.
pub const REQ_OP_ZONE_RESET_ALL: u32 = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zone_types_distinct() {
        let types = [
            BLK_ZONE_TYPE_CONVENTIONAL,
            BLK_ZONE_TYPE_SEQWRITE_REQ,
            BLK_ZONE_TYPE_SEQWRITE_PREF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_zone_conditions_distinct() {
        let conds = [
            BLK_ZONE_COND_NOT_WP, BLK_ZONE_COND_EMPTY,
            BLK_ZONE_COND_IMP_OPEN, BLK_ZONE_COND_EXP_OPEN,
            BLK_ZONE_COND_CLOSED, BLK_ZONE_COND_READONLY,
            BLK_ZONE_COND_FULL, BLK_ZONE_COND_OFFLINE,
        ];
        for i in 0..conds.len() {
            for j in (i + 1)..conds.len() {
                assert_ne!(conds[i], conds[j]);
            }
        }
    }

    #[test]
    fn test_zone_ops_distinct() {
        let ops = [
            REQ_OP_ZONE_OPEN, REQ_OP_ZONE_CLOSE,
            REQ_OP_ZONE_FINISH, REQ_OP_ZONE_RESET,
            REQ_OP_ZONE_APPEND, REQ_OP_ZONE_RESET_ALL,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_zone_lifecycle() {
        // Logical state progression: empty → open → closed → full
        assert!(BLK_ZONE_COND_EMPTY < BLK_ZONE_COND_IMP_OPEN);
        assert!(BLK_ZONE_COND_IMP_OPEN < BLK_ZONE_COND_EXP_OPEN);
        assert!(BLK_ZONE_COND_EXP_OPEN < BLK_ZONE_COND_CLOSED);
    }
}
