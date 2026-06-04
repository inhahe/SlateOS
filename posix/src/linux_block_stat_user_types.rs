//! `<linux/genhd.h>` — `/proc/diskstats` and `/sys/block/<dev>/stat`
//! field layout.
//!
//! Both surfaces emit a fixed-order space-separated list of
//! cumulative counters per block device. Userspace `iostat`, `sar`,
//! `iotop`, and Prometheus exporters all parse it. The field count
//! has grown over kernel versions — this module pins the modern
//! 17-column format and labels each column.

// ---------------------------------------------------------------------------
// Field counts per kernel-version layout
// ---------------------------------------------------------------------------

/// Original 2.6 layout (read{ios,merges,sectors,ticks},
/// write{ios,merges,sectors,ticks}, in_flight, io_ticks, time_in_queue).
pub const BLOCK_STAT_FIELDS_LEGACY: usize = 11;

/// 4.18+ added discard{ios,merges,sectors,ticks} — 15 fields.
pub const BLOCK_STAT_FIELDS_DISCARD: usize = 15;

/// 5.5+ added flush{ios,ticks} — 17 fields. Current format.
pub const BLOCK_STAT_FIELDS_FLUSH: usize = 17;

// ---------------------------------------------------------------------------
// Per-field column indices (in the 17-column format)
// ---------------------------------------------------------------------------

pub const BLOCK_STAT_COL_READ_IOS: usize = 0;
pub const BLOCK_STAT_COL_READ_MERGES: usize = 1;
pub const BLOCK_STAT_COL_READ_SECTORS: usize = 2;
pub const BLOCK_STAT_COL_READ_TICKS: usize = 3;
pub const BLOCK_STAT_COL_WRITE_IOS: usize = 4;
pub const BLOCK_STAT_COL_WRITE_MERGES: usize = 5;
pub const BLOCK_STAT_COL_WRITE_SECTORS: usize = 6;
pub const BLOCK_STAT_COL_WRITE_TICKS: usize = 7;
pub const BLOCK_STAT_COL_IN_FLIGHT: usize = 8;
pub const BLOCK_STAT_COL_IO_TICKS: usize = 9;
pub const BLOCK_STAT_COL_TIME_IN_QUEUE: usize = 10;
pub const BLOCK_STAT_COL_DISCARD_IOS: usize = 11;
pub const BLOCK_STAT_COL_DISCARD_MERGES: usize = 12;
pub const BLOCK_STAT_COL_DISCARD_SECTORS: usize = 13;
pub const BLOCK_STAT_COL_DISCARD_TICKS: usize = 14;
pub const BLOCK_STAT_COL_FLUSH_IOS: usize = 15;
pub const BLOCK_STAT_COL_FLUSH_TICKS: usize = 16;

// ---------------------------------------------------------------------------
// Per-field width in /proc/diskstats (only relevant for column-aligned reads)
// ---------------------------------------------------------------------------

/// /proc/diskstats prefix: major, minor, name (in addition to the stat columns).
pub const DISKSTATS_PREFIX_FIELDS: usize = 3;

/// Total /proc/diskstats column count = prefix + 17 stats.
pub const DISKSTATS_TOTAL_FIELDS: usize =
    DISKSTATS_PREFIX_FIELDS + BLOCK_STAT_FIELDS_FLUSH;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_count_progression() {
        assert_eq!(BLOCK_STAT_FIELDS_LEGACY, 11);
        assert_eq!(BLOCK_STAT_FIELDS_DISCARD, 15);
        assert_eq!(BLOCK_STAT_FIELDS_FLUSH, 17);
        // Each kernel-version step added exactly one stat family.
        assert_eq!(BLOCK_STAT_FIELDS_DISCARD - BLOCK_STAT_FIELDS_LEGACY, 4);
        assert_eq!(BLOCK_STAT_FIELDS_FLUSH - BLOCK_STAT_FIELDS_DISCARD, 2);
    }

    #[test]
    fn test_column_indices_dense_0_to_16() {
        let c = [
            BLOCK_STAT_COL_READ_IOS,
            BLOCK_STAT_COL_READ_MERGES,
            BLOCK_STAT_COL_READ_SECTORS,
            BLOCK_STAT_COL_READ_TICKS,
            BLOCK_STAT_COL_WRITE_IOS,
            BLOCK_STAT_COL_WRITE_MERGES,
            BLOCK_STAT_COL_WRITE_SECTORS,
            BLOCK_STAT_COL_WRITE_TICKS,
            BLOCK_STAT_COL_IN_FLIGHT,
            BLOCK_STAT_COL_IO_TICKS,
            BLOCK_STAT_COL_TIME_IN_QUEUE,
            BLOCK_STAT_COL_DISCARD_IOS,
            BLOCK_STAT_COL_DISCARD_MERGES,
            BLOCK_STAT_COL_DISCARD_SECTORS,
            BLOCK_STAT_COL_DISCARD_TICKS,
            BLOCK_STAT_COL_FLUSH_IOS,
            BLOCK_STAT_COL_FLUSH_TICKS,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v, i);
        }
        // 17 distinct columns total.
        assert_eq!(c.len(), BLOCK_STAT_FIELDS_FLUSH);
    }

    #[test]
    fn test_read_write_groups_aligned() {
        // Each {ios,merges,sectors,ticks} group is 4 contiguous fields.
        assert_eq!(
            BLOCK_STAT_COL_WRITE_IOS - BLOCK_STAT_COL_READ_IOS,
            4
        );
        assert_eq!(
            BLOCK_STAT_COL_WRITE_TICKS - BLOCK_STAT_COL_READ_TICKS,
            4
        );
        // Discard group also 4 contiguous (ios..ticks).
        assert_eq!(
            BLOCK_STAT_COL_DISCARD_TICKS - BLOCK_STAT_COL_DISCARD_IOS,
            3
        );
    }

    #[test]
    fn test_diskstats_total() {
        assert_eq!(DISKSTATS_PREFIX_FIELDS, 3);
        assert_eq!(DISKSTATS_TOTAL_FIELDS, 3 + 17);
        // 20 columns total in modern /proc/diskstats.
        assert_eq!(DISKSTATS_TOTAL_FIELDS, 20);
    }

    #[test]
    fn test_inflight_is_between_write_and_io_ticks() {
        // The in_flight gauge follows the write group and precedes
        // the io_ticks / time_in_queue pair.
        assert!(BLOCK_STAT_COL_WRITE_TICKS < BLOCK_STAT_COL_IN_FLIGHT);
        assert!(BLOCK_STAT_COL_IN_FLIGHT < BLOCK_STAT_COL_IO_TICKS);
        assert_eq!(
            BLOCK_STAT_COL_IO_TICKS - BLOCK_STAT_COL_IN_FLIGHT,
            1
        );
    }
}
