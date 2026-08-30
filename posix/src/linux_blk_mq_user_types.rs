//! `<linux/blk-mq.h>` — multi-queue block layer user-facing constants.
//!
//! The `blk-mq` infrastructure is mostly kernel-private, but it does
//! export a handful of sysfs files (`/sys/block/<dev>/queue/`) and
//! tag/queue-depth limits that userspace tooling (`fio`, `iostat`,
//! `nvme-cli`) reads.

// ---------------------------------------------------------------------------
// Queue-depth and tag limits
// ---------------------------------------------------------------------------

pub const BLK_MQ_MIN_DEPTH: u32 = 1;
pub const BLK_MQ_MAX_DEPTH: u32 = 10_240;
pub const BLK_MQ_NO_TAG: u32 = u32::MAX;

/// Default reserved-tags count (for fairness / high-priority requests).
pub const BLK_MQ_DEFAULT_RESERVED: u32 = 0;

// ---------------------------------------------------------------------------
// I/O priority classes (`enum bio_prio`)
// ---------------------------------------------------------------------------

pub const IOPRIO_CLASS_NONE: u32 = 0;
pub const IOPRIO_CLASS_RT: u32 = 1;
pub const IOPRIO_CLASS_BE: u32 = 2;
pub const IOPRIO_CLASS_IDLE: u32 = 3;
pub const IOPRIO_CLASS_INVALID: u32 = 7;

/// 8 priority levels (0..7) within RT/BE classes.
pub const IOPRIO_NR_LEVELS: u32 = 8;

/// Default best-effort priority level (matches Linux task default).
pub const IOPRIO_BE_NORM: u32 = 4;

// ---------------------------------------------------------------------------
// Queue sysfs attribute names
// ---------------------------------------------------------------------------

pub const SYSFS_QUEUE_SCHEDULER: &str = "scheduler";
pub const SYSFS_QUEUE_NR_REQUESTS: &str = "nr_requests";
pub const SYSFS_QUEUE_READ_AHEAD_KB: &str = "read_ahead_kb";
pub const SYSFS_QUEUE_MAX_SECTORS_KB: &str = "max_sectors_kb";
pub const SYSFS_QUEUE_NOMERGES: &str = "nomerges";
pub const SYSFS_QUEUE_RQ_AFFINITY: &str = "rq_affinity";
pub const SYSFS_QUEUE_ROTATIONAL: &str = "rotational";
pub const SYSFS_QUEUE_HW_SECTOR_SIZE: &str = "hw_sector_size";
pub const SYSFS_QUEUE_LOGICAL_BLOCK_SIZE: &str = "logical_block_size";
pub const SYSFS_QUEUE_PHYSICAL_BLOCK_SIZE: &str = "physical_block_size";

// ---------------------------------------------------------------------------
// nomerges (request-merging policy)
// ---------------------------------------------------------------------------

pub const QUEUE_NOMERGES_DEFAULT: u32 = 0;
pub const QUEUE_NOMERGES_SIMPLE: u32 = 1;
pub const QUEUE_NOMERGES_FULL: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_depth_bounds() {
        assert_eq!(BLK_MQ_MIN_DEPTH, 1);
        assert_eq!(BLK_MQ_MAX_DEPTH, 10_240);
        // Sentinel tag value (used for "no tag yet allocated").
        assert_eq!(BLK_MQ_NO_TAG, u32::MAX);
        // Default 0 reserved tags — drivers opt-in.
        assert_eq!(BLK_MQ_DEFAULT_RESERVED, 0);
        assert!(BLK_MQ_MIN_DEPTH < BLK_MQ_MAX_DEPTH);
    }

    #[test]
    fn test_ioprio_classes_dense_0_to_3_plus_invalid() {
        let c = [
            IOPRIO_CLASS_NONE,
            IOPRIO_CLASS_RT,
            IOPRIO_CLASS_BE,
            IOPRIO_CLASS_IDLE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // INVALID is a sentinel above the valid range — the ioprio_t
        // encoding uses 3 bits, so 7 is the maximum representable.
        assert_eq!(IOPRIO_CLASS_INVALID, 7);
        for &v in &c {
            assert!(v < IOPRIO_CLASS_INVALID);
        }
    }

    #[test]
    fn test_ioprio_levels_and_default() {
        // 8 levels in each class (0..=7).
        assert_eq!(IOPRIO_NR_LEVELS, 8);
        assert!(IOPRIO_NR_LEVELS.is_power_of_two());
        // Default best-effort sits squarely in the middle.
        assert_eq!(IOPRIO_BE_NORM, 4);
        assert!(IOPRIO_BE_NORM < IOPRIO_NR_LEVELS);
    }

    #[test]
    fn test_sysfs_queue_attr_names_distinct() {
        let a = [
            SYSFS_QUEUE_SCHEDULER,
            SYSFS_QUEUE_NR_REQUESTS,
            SYSFS_QUEUE_READ_AHEAD_KB,
            SYSFS_QUEUE_MAX_SECTORS_KB,
            SYSFS_QUEUE_NOMERGES,
            SYSFS_QUEUE_RQ_AFFINITY,
            SYSFS_QUEUE_ROTATIONAL,
            SYSFS_QUEUE_HW_SECTOR_SIZE,
            SYSFS_QUEUE_LOGICAL_BLOCK_SIZE,
            SYSFS_QUEUE_PHYSICAL_BLOCK_SIZE,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
            assert!(!x.contains('/'));
        }
        // The *_block_size trio shares a suffix.
        for &v in &[
            SYSFS_QUEUE_LOGICAL_BLOCK_SIZE,
            SYSFS_QUEUE_PHYSICAL_BLOCK_SIZE,
        ] {
            assert!(v.ends_with("_block_size"));
        }
    }

    #[test]
    fn test_nomerges_modes_dense_0_to_2() {
        let n = [
            QUEUE_NOMERGES_DEFAULT,
            QUEUE_NOMERGES_SIMPLE,
            QUEUE_NOMERGES_FULL,
        ];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // DEFAULT enables full merging (legacy convention).
        assert_eq!(QUEUE_NOMERGES_DEFAULT, 0);
    }
}
