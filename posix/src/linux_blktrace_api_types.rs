//! `<linux/blktrace_api.h>` — blktrace user-API constants.
//!
//! blktrace traces I/O at the block layer and feeds blkparse,
//! seekwatcher, btt, and similar tools. Userspace sets up tracing
//! via the `BLKTRACE*` ioctls on the block device, then reads
//! `struct blk_io_trace` events from a relay file. Constants below
//! cover the action codes, category bits, control ioctls, and the
//! magic header used to validate trace files.

// ---------------------------------------------------------------------------
// Action codes (low 16 bits of blk_io_trace.action)
// ---------------------------------------------------------------------------

/// Queue (I/O entered the queue).
pub const BLK_TA_QUEUE: u32 = 1;
/// Back-merge with existing request.
pub const BLK_TA_BACKMERGE: u32 = 2;
/// Front-merge with existing request.
pub const BLK_TA_FRONTMERGE: u32 = 3;
/// I/O reached the dispatch list.
pub const BLK_TA_GETRQ: u32 = 4;
/// Failed to get a request, sleeping.
pub const BLK_TA_SLEEPRQ: u32 = 5;
/// Request requeued after partial completion.
pub const BLK_TA_REQUEUE: u32 = 6;
/// I/O issued to the driver.
pub const BLK_TA_ISSUE: u32 = 7;
/// I/O completed.
pub const BLK_TA_COMPLETE: u32 = 8;
/// Plug inserted (queue plugged).
pub const BLK_TA_PLUG: u32 = 9;
/// Unplug triggered by I/O.
pub const BLK_TA_UNPLUG_IO: u32 = 10;
/// Unplug triggered by timer.
pub const BLK_TA_UNPLUG_TIMER: u32 = 11;
/// Insert into queue.
pub const BLK_TA_INSERT: u32 = 12;
/// I/O split.
pub const BLK_TA_SPLIT: u32 = 13;
/// I/O bounced (high-mem to low-mem copy).
pub const BLK_TA_BOUNCE: u32 = 14;
/// I/O remapped to another device.
pub const BLK_TA_REMAP: u32 = 15;
/// I/O aborted.
pub const BLK_TA_ABORT: u32 = 16;
/// User message inserted into the trace stream.
pub const BLK_TA_DRV_DATA: u32 = 17;

// ---------------------------------------------------------------------------
// Category bits (high 16 bits of blk_io_trace.action — OR with action)
// ---------------------------------------------------------------------------

/// Read I/O.
pub const BLK_TC_READ: u32 = 1 << 0;
/// Write I/O.
pub const BLK_TC_WRITE: u32 = 1 << 1;
/// Flush I/O.
pub const BLK_TC_FLUSH: u32 = 1 << 2;
/// Synchronous I/O.
pub const BLK_TC_SYNC: u32 = 1 << 3;
/// Queue mark (plug/unplug events).
pub const BLK_TC_QUEUE: u32 = 1 << 4;
/// Request mark.
pub const BLK_TC_REQUEUE: u32 = 1 << 5;
/// Issue mark.
pub const BLK_TC_ISSUE: u32 = 1 << 6;
/// Completion mark.
pub const BLK_TC_COMPLETE: u32 = 1 << 7;
/// FS-level mark.
pub const BLK_TC_FS: u32 = 1 << 8;
/// Page-cache mark.
pub const BLK_TC_PC: u32 = 1 << 9;
/// AHEAD (read-ahead) flag.
pub const BLK_TC_AHEAD: u32 = 1 << 11;
/// Metadata I/O.
pub const BLK_TC_META: u32 = 1 << 12;
/// Discard I/O.
pub const BLK_TC_DISCARD: u32 = 1 << 13;
/// Driver-level data.
pub const BLK_TC_DRV_DATA: u32 = 1 << 14;
/// Force-unit-access flag.
pub const BLK_TC_FUA: u32 = 1 << 15;

/// Category mask (all category bits OR'd together).
pub const BLK_TC_END: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Header / file magic
// ---------------------------------------------------------------------------

/// "blktrace" magic in the trace-file header (little-endian "blk").
pub const BLK_IO_TRACE_MAGIC: u32 = 0x65617400;
/// Current trace format version.
pub const BLK_IO_TRACE_VERSION: u32 = 0x07;

// ---------------------------------------------------------------------------
// Per-CPU buffer parameters
// ---------------------------------------------------------------------------

/// Maximum length of a trace name (mount-point in /sys/kernel/debug).
pub const BLKTRACE_BDEV_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// ioctl numbers
// ---------------------------------------------------------------------------

/// `BLKTRACESETUP` — start a new trace.
pub const BLKTRACESETUP: u32 = 0xc0481273;
/// `BLKTRACESTART` — begin recording.
pub const BLKTRACESTART: u32 = 0x1274;
/// `BLKTRACESTOP` — stop recording.
pub const BLKTRACESTOP: u32 = 0x1275;
/// `BLKTRACETEARDOWN` — release trace resources.
pub const BLKTRACETEARDOWN: u32 = 0x1276;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let a = [
            BLK_TA_QUEUE,
            BLK_TA_BACKMERGE,
            BLK_TA_FRONTMERGE,
            BLK_TA_GETRQ,
            BLK_TA_SLEEPRQ,
            BLK_TA_REQUEUE,
            BLK_TA_ISSUE,
            BLK_TA_COMPLETE,
            BLK_TA_PLUG,
            BLK_TA_UNPLUG_IO,
            BLK_TA_UNPLUG_TIMER,
            BLK_TA_INSERT,
            BLK_TA_SPLIT,
            BLK_TA_BOUNCE,
            BLK_TA_REMAP,
            BLK_TA_ABORT,
            BLK_TA_DRV_DATA,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
            // All actions fit in the low 16 bits.
            assert!(a[i] <= 0xffff);
        }
    }

    #[test]
    fn test_categories_are_distinct_bits() {
        let c = [
            BLK_TC_READ,
            BLK_TC_WRITE,
            BLK_TC_FLUSH,
            BLK_TC_SYNC,
            BLK_TC_QUEUE,
            BLK_TC_REQUEUE,
            BLK_TC_ISSUE,
            BLK_TC_COMPLETE,
            BLK_TC_FS,
            BLK_TC_PC,
            BLK_TC_AHEAD,
            BLK_TC_META,
            BLK_TC_DISCARD,
            BLK_TC_DRV_DATA,
            BLK_TC_FUA,
        ];
        for &bit in &c {
            assert!(bit.is_power_of_two());
        }
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // BLK_TC_END marks the last valid bit, == BLK_TC_FUA.
        assert_eq!(BLK_TC_END, BLK_TC_FUA);
    }

    #[test]
    fn test_magic_and_version() {
        // Magic is a four-byte tag with the high byte holding 'b'
        // (0x65 = 'e' in the historical layout).
        assert_ne!(BLK_IO_TRACE_MAGIC, 0);
        assert!(BLK_IO_TRACE_VERSION >= 7);
    }

    #[test]
    fn test_ioctls_distinct() {
        let i = [
            BLKTRACESETUP,
            BLKTRACESTART,
            BLKTRACESTOP,
            BLKTRACETEARDOWN,
        ];
        for x in 0..i.len() {
            for y in (x + 1)..i.len() {
                assert_ne!(i[x], i[y]);
            }
        }
    }
}
