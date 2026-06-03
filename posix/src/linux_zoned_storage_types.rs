//! `<linux/blkzoned.h>` — Zoned Block Device (ZBD) constants.
//!
//! Constants for the Linux uapi describing host-managed and
//! host-aware SMR HDDs and ZNS NVMe SSDs. Userspace tools
//! (`blkzone`, fio, libzbc) consume these.

// ---------------------------------------------------------------------------
// Zone-type codes (struct blk_zone.type)
// ---------------------------------------------------------------------------

/// Conventional zone — randomly writable, no zone pointer.
pub const BLK_ZONE_TYPE_CONVENTIONAL: u32 = 0x1;
/// Sequential-write-required zone — host-managed.
pub const BLK_ZONE_TYPE_SEQWRITE_REQ: u32 = 0x2;
/// Sequential-write-preferred zone — host-aware.
pub const BLK_ZONE_TYPE_SEQWRITE_PREF: u32 = 0x3;

// ---------------------------------------------------------------------------
// Zone-condition codes (struct blk_zone.cond)
// ---------------------------------------------------------------------------

/// Not yet written.
pub const BLK_ZONE_COND_NOT_WP: u32 = 0x0;
/// Empty — write pointer at the start.
pub const BLK_ZONE_COND_EMPTY: u32 = 0x1;
/// Implicitly opened by a write.
pub const BLK_ZONE_COND_IMP_OPEN: u32 = 0x2;
/// Explicitly opened by a zone-open op.
pub const BLK_ZONE_COND_EXP_OPEN: u32 = 0x3;
/// Closed — written but not yet full.
pub const BLK_ZONE_COND_CLOSED: u32 = 0x4;
/// Read-only.
pub const BLK_ZONE_COND_READONLY: u32 = 0xd;
/// Full — write pointer at the end.
pub const BLK_ZONE_COND_FULL: u32 = 0xe;
/// Offline — zone has been retired.
pub const BLK_ZONE_COND_OFFLINE: u32 = 0xf;

// ---------------------------------------------------------------------------
// Zone-operation codes (blk_zone_report / blk_zone_action ioctl args)
// ---------------------------------------------------------------------------

/// Close zone explicitly.
pub const BLK_ZONE_ACTION_CLOSE: u32 = 0x1;
/// Finish zone (write pointer to end).
pub const BLK_ZONE_ACTION_FINISH: u32 = 0x2;
/// Open zone explicitly.
pub const BLK_ZONE_ACTION_OPEN: u32 = 0x3;
/// Reset zone (reclaim space).
pub const BLK_ZONE_ACTION_RESET: u32 = 0x4;
/// Reset all zones.
pub const BLK_ZONE_ACTION_RESET_ALL: u32 = 0x5;

// ---------------------------------------------------------------------------
// Report-zone flag bits (blk_zone_report.flags)
// ---------------------------------------------------------------------------

/// Reset write pointer recommended (host-aware hint).
pub const BLK_ZONE_REP_F_RESET_WP_REC: u32 = 1 << 0;
/// Zone is non-sequential write resources allocated.
pub const BLK_ZONE_REP_F_NON_SEQ: u32 = 1 << 1;

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
        // All zone-type codes must fit a 4-bit field on the wire.
        for &t in &types {
            assert!(t <= 0xf);
        }
    }

    #[test]
    fn test_zone_conditions_distinct_and_fit_4_bits() {
        let conds = [
            BLK_ZONE_COND_NOT_WP,
            BLK_ZONE_COND_EMPTY,
            BLK_ZONE_COND_IMP_OPEN,
            BLK_ZONE_COND_EXP_OPEN,
            BLK_ZONE_COND_CLOSED,
            BLK_ZONE_COND_READONLY,
            BLK_ZONE_COND_FULL,
            BLK_ZONE_COND_OFFLINE,
        ];
        for &c in &conds {
            assert!(c <= 0xf);
        }
        for i in 0..conds.len() {
            for j in (i + 1)..conds.len() {
                assert_ne!(conds[i], conds[j]);
            }
        }
    }

    #[test]
    fn test_actions_distinct() {
        let acts = [
            BLK_ZONE_ACTION_CLOSE,
            BLK_ZONE_ACTION_FINISH,
            BLK_ZONE_ACTION_OPEN,
            BLK_ZONE_ACTION_RESET,
            BLK_ZONE_ACTION_RESET_ALL,
        ];
        for i in 0..acts.len() {
            for j in (i + 1)..acts.len() {
                assert_ne!(acts[i], acts[j]);
            }
        }
    }

    #[test]
    fn test_report_flag_bits() {
        assert!(BLK_ZONE_REP_F_RESET_WP_REC.is_power_of_two());
        assert!(BLK_ZONE_REP_F_NON_SEQ.is_power_of_two());
        assert_ne!(BLK_ZONE_REP_F_RESET_WP_REC, BLK_ZONE_REP_F_NON_SEQ);
    }
}
