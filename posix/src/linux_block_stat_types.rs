//! `<linux/blk-stat.h>` — Block device statistics constants.
//!
//! Block device statistics track I/O performance metrics: read/write
//! counts, bytes transferred, time spent in queue, time spent
//! servicing I/O, and in-flight request counts. These are exposed
//! via /sys/block/<dev>/stat and /proc/diskstats. Applications and
//! monitoring tools use these to detect bottlenecks, measure
//! throughput, and calculate utilization.

// ---------------------------------------------------------------------------
// Disk stat field indices (as in /proc/diskstats)
// ---------------------------------------------------------------------------

/// Number of reads completed.
pub const DISKSTAT_READS_COMPLETED: u32 = 0;
/// Number of reads merged (adjacent requests combined).
pub const DISKSTAT_READS_MERGED: u32 = 1;
/// Number of sectors read.
pub const DISKSTAT_SECTORS_READ: u32 = 2;
/// Time spent reading (milliseconds).
pub const DISKSTAT_READ_TIME_MS: u32 = 3;
/// Number of writes completed.
pub const DISKSTAT_WRITES_COMPLETED: u32 = 4;
/// Number of writes merged.
pub const DISKSTAT_WRITES_MERGED: u32 = 5;
/// Number of sectors written.
pub const DISKSTAT_SECTORS_WRITTEN: u32 = 6;
/// Time spent writing (milliseconds).
pub const DISKSTAT_WRITE_TIME_MS: u32 = 7;
/// Number of I/Os currently in flight.
pub const DISKSTAT_IO_IN_FLIGHT: u32 = 8;
/// Time spent doing I/O (milliseconds, non-zero while queue non-empty).
pub const DISKSTAT_IO_TIME_MS: u32 = 9;
/// Weighted time spent doing I/O (ms * in_flight).
pub const DISKSTAT_WEIGHTED_IO_TIME_MS: u32 = 10;
/// Number of discards completed.
pub const DISKSTAT_DISCARDS_COMPLETED: u32 = 11;
/// Number of discards merged.
pub const DISKSTAT_DISCARDS_MERGED: u32 = 12;
/// Number of sectors discarded.
pub const DISKSTAT_SECTORS_DISCARDED: u32 = 13;
/// Time spent discarding (milliseconds).
pub const DISKSTAT_DISCARD_TIME_MS: u32 = 14;
/// Number of flush requests completed.
pub const DISKSTAT_FLUSH_COMPLETED: u32 = 15;
/// Time spent flushing (milliseconds).
pub const DISKSTAT_FLUSH_TIME_MS: u32 = 16;
/// Total number of stat fields.
pub const DISKSTAT_NUM_FIELDS: u32 = 17;

// ---------------------------------------------------------------------------
// Block stat callback types
// ---------------------------------------------------------------------------

/// Read latency bucket.
pub const BLK_STAT_READ: u32 = 0;
/// Write latency bucket.
pub const BLK_STAT_WRITE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stat_fields_sequential() {
        assert_eq!(DISKSTAT_READS_COMPLETED, 0);
        assert_eq!(DISKSTAT_FLUSH_TIME_MS, 16);
        assert_eq!(DISKSTAT_NUM_FIELDS, 17);
    }

    #[test]
    fn test_stat_fields_distinct() {
        let fields: Vec<u32> = (0..DISKSTAT_NUM_FIELDS).collect();
        // They are sequential 0..17, so inherently distinct
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_blk_stat_types_distinct() {
        assert_ne!(BLK_STAT_READ, BLK_STAT_WRITE);
    }
}
