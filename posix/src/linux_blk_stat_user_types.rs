//! `<linux/blk-stat.h>` — block-layer per-device I/O statistics.
//!
//! The block layer maintains rolling latency histograms per request
//! type so the I/O scheduler (wbt, BFQ) and userspace tools (`iostat`,
//! `bpftrace`) can read smoothed throughput / queue-depth numbers
//! without touching the request fast path.

// ---------------------------------------------------------------------------
// Request-type bins (`enum blk_stat_rw`)
// ---------------------------------------------------------------------------

pub const BLK_STAT_READ: u32 = 0;
pub const BLK_STAT_WRITE: u32 = 1;
pub const BLK_STAT_DISCARD: u32 = 2;
pub const BLK_STAT_FLUSH: u32 = 3;

/// Number of request-type bins (one past the last valid index).
pub const BLK_STAT_NR_RW: u32 = 4;

// ---------------------------------------------------------------------------
// Rolling-window timing (nanoseconds)
// ---------------------------------------------------------------------------

/// Default callback granularity — 1 ms windows.
pub const BLK_STAT_DEFAULT_CB_NSEC: u64 = 1_000_000;

/// Minimum callback granularity — 1 us (any smaller is lost in jitter).
pub const BLK_STAT_MIN_CB_NSEC: u64 = 1_000;

/// Maximum callback granularity — 1 s (longer windows hide spikes).
pub const BLK_STAT_MAX_CB_NSEC: u64 = 1_000_000_000;

/// Number of buckets in each rolling histogram (powers-of-two latency bands).
pub const BLK_STAT_NR_BUCKETS: u32 = 16;

// ---------------------------------------------------------------------------
// Writeback-throttling (`wbt`) defaults
// ---------------------------------------------------------------------------

/// Default WBT minimum latency (ms) before throttling kicks in.
pub const WBT_DEFAULT_MIN_LAT_MS: u32 = 75;

/// WBT scales back queue depth in 4 steps.
pub const WBT_NR_SCALE_STEPS: u32 = 4;

/// WBT "rwb_step" minimum depth.
pub const WBT_MIN_DEPTH: u32 = 1;

// ---------------------------------------------------------------------------
// sysfs attribute names (`/sys/block/<dev>/stat`, `/sys/block/<dev>/queue/iostats`)
// ---------------------------------------------------------------------------

pub const SYSFS_BLOCK_STAT: &str = "stat";
pub const SYSFS_BLOCK_INFLIGHT: &str = "inflight";
pub const SYSFS_QUEUE_IOSTATS: &str = "iostats";
pub const SYSFS_QUEUE_WBT_LAT_USEC: &str = "wbt_lat_usec";
pub const SYSFS_QUEUE_IO_POLL: &str = "io_poll";
pub const SYSFS_QUEUE_IO_POLL_DELAY: &str = "io_poll_delay";

/// Number of u64 fields in the `/sys/block/<dev>/stat` line.
///
/// The Linux kernel writes 17 cumulative counters on each read of
/// `stat`: ios/sectors/ticks for read, write, discard, plus flush
/// counts and the in-flight + io_ticks + time_in_queue trio.
pub const SYSFS_BLOCK_STAT_FIELDS: usize = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rw_bins_dense_0_to_3_plus_nr() {
        let r = [
            BLK_STAT_READ,
            BLK_STAT_WRITE,
            BLK_STAT_DISCARD,
            BLK_STAT_FLUSH,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // _NR_RW is one past the last bin.
        assert_eq!(BLK_STAT_NR_RW, r.len() as u32);
    }

    #[test]
    fn test_window_timing_bounds() {
        assert_eq!(BLK_STAT_DEFAULT_CB_NSEC, 1_000_000);
        assert_eq!(BLK_STAT_MIN_CB_NSEC, 1_000);
        assert_eq!(BLK_STAT_MAX_CB_NSEC, 1_000_000_000);
        // Default sits between min and max.
        assert!(BLK_STAT_MIN_CB_NSEC < BLK_STAT_DEFAULT_CB_NSEC);
        assert!(BLK_STAT_DEFAULT_CB_NSEC < BLK_STAT_MAX_CB_NSEC);
        // Min/max span six orders of magnitude (1 us .. 1 s).
        assert_eq!(BLK_STAT_MAX_CB_NSEC / BLK_STAT_MIN_CB_NSEC, 1_000_000);
        // Default = 1000x min, max = 1000x default — equal ratios.
        assert_eq!(BLK_STAT_DEFAULT_CB_NSEC / BLK_STAT_MIN_CB_NSEC, 1_000);
        assert_eq!(BLK_STAT_MAX_CB_NSEC / BLK_STAT_DEFAULT_CB_NSEC, 1_000);
    }

    #[test]
    fn test_bucket_count_is_power_of_two() {
        assert_eq!(BLK_STAT_NR_BUCKETS, 16);
        assert!(BLK_STAT_NR_BUCKETS.is_power_of_two());
    }

    #[test]
    fn test_wbt_defaults() {
        assert_eq!(WBT_DEFAULT_MIN_LAT_MS, 75);
        assert_eq!(WBT_NR_SCALE_STEPS, 4);
        assert_eq!(WBT_MIN_DEPTH, 1);
        // Scale-step count is a power of two (bit-mask backoff).
        assert!(WBT_NR_SCALE_STEPS.is_power_of_two());
    }

    #[test]
    fn test_sysfs_attribute_names_distinct() {
        let a = [
            SYSFS_BLOCK_STAT,
            SYSFS_BLOCK_INFLIGHT,
            SYSFS_QUEUE_IOSTATS,
            SYSFS_QUEUE_WBT_LAT_USEC,
            SYSFS_QUEUE_IO_POLL,
            SYSFS_QUEUE_IO_POLL_DELAY,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
            assert!(!x.contains('/'));
        }
        // io_poll_delay extends io_poll with a suffix.
        assert!(SYSFS_QUEUE_IO_POLL_DELAY.starts_with(SYSFS_QUEUE_IO_POLL));
    }

    #[test]
    fn test_block_stat_field_count() {
        // /sys/block/<dev>/stat exposes 17 cumulative counters.
        assert_eq!(SYSFS_BLOCK_STAT_FIELDS, 17);
        // Three triples (ios/sectors/ticks for r/w/d) + flush count
        // + inflight + io_ticks + time_in_queue = 3*3 + 1 + 1 + 1 + 1 + 1 + 1 + 2 = 17.
        // (The 17-field layout is the stable v5+ format.)
        assert!(SYSFS_BLOCK_STAT_FIELDS > BLK_STAT_NR_RW as usize);
    }
}
