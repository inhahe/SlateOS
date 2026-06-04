//! `<linux/blktrace_api.h>` — `blktrace` userspace control surface.
//!
//! `blktrace` is the kernel's per-bio event stream. Userspace
//! (`blktrace`, `btrace`, `iowatcher`) sets up a per-CPU relay
//! through these ioctls and parses the binary `struct blk_io_trace`
//! records written to the relay buffers.

// ---------------------------------------------------------------------------
// Magic / version
// ---------------------------------------------------------------------------

/// Trace-file magic ("\x65\x10" in low half + version in high half).
pub const BLK_IO_TRACE_MAGIC: u32 = 0x6510_0000;

/// Current trace-record version.
pub const BLK_IO_TRACE_VERSION: u32 = 0x07;

/// Mask isolating the version field in the magic word.
pub const BLK_IO_TRACE_VERSION_MASK: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Trace-class (event-type) bits (`BLK_TC_*`)
// ---------------------------------------------------------------------------

pub const BLK_TC_READ: u32 = 1 << 0;
pub const BLK_TC_WRITE: u32 = 1 << 1;
pub const BLK_TC_FLUSH: u32 = 1 << 2;
pub const BLK_TC_SYNC: u32 = 1 << 3;
pub const BLK_TC_QUEUE: u32 = 1 << 4;
pub const BLK_TC_REQUEUE: u32 = 1 << 5;
pub const BLK_TC_ISSUE: u32 = 1 << 6;
pub const BLK_TC_COMPLETE: u32 = 1 << 7;
pub const BLK_TC_FS: u32 = 1 << 8;
pub const BLK_TC_PC: u32 = 1 << 9;
pub const BLK_TC_NOTIFY: u32 = 1 << 10;
pub const BLK_TC_AHEAD: u32 = 1 << 11;
pub const BLK_TC_META: u32 = 1 << 12;
pub const BLK_TC_DISCARD: u32 = 1 << 13;
pub const BLK_TC_DRV_DATA: u32 = 1 << 14;
pub const BLK_TC_FUA: u32 = 1 << 15;

/// "All event classes" mask.
pub const BLK_TC_END: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Action codes (`BLK_TA_*` — low byte of action word)
// ---------------------------------------------------------------------------

pub const BLK_TA_QUEUE: u32 = 1;
pub const BLK_TA_BACKMERGE: u32 = 2;
pub const BLK_TA_FRONTMERGE: u32 = 3;
pub const BLK_TA_GETRQ: u32 = 4;
pub const BLK_TA_SLEEPRQ: u32 = 5;
pub const BLK_TA_REQUEUE: u32 = 6;
pub const BLK_TA_ISSUE: u32 = 7;
pub const BLK_TA_COMPLETE: u32 = 8;
pub const BLK_TA_PLUG: u32 = 9;
pub const BLK_TA_UNPLUG_IO: u32 = 10;
pub const BLK_TA_UNPLUG_TIMER: u32 = 11;
pub const BLK_TA_INSERT: u32 = 12;
pub const BLK_TA_SPLIT: u32 = 13;
pub const BLK_TA_BOUNCE: u32 = 14;
pub const BLK_TA_REMAP: u32 = 15;
pub const BLK_TA_ABORT: u32 = 16;
pub const BLK_TA_DRV_DATA: u32 = 17;

// ---------------------------------------------------------------------------
// blktrace setup ioctls
// ---------------------------------------------------------------------------

pub const BLKTRACESETUP: u32 = 0x1276;
pub const BLKTRACESTART: u32 = 0x1274;
pub const BLKTRACESTOP: u32 = 0x1273;
pub const BLKTRACETEARDOWN: u32 = 0x1275;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_layout() {
        // Top 16 bits hold the "\x65\x10" marker; low bits are version.
        assert_eq!(BLK_IO_TRACE_MAGIC, 0x6510_0000);
        assert_eq!(BLK_IO_TRACE_VERSION, 7);
        assert_eq!(BLK_IO_TRACE_VERSION_MASK, 0xFF);
        // Magic does not overlap the version field.
        assert_eq!(BLK_IO_TRACE_MAGIC & BLK_IO_TRACE_VERSION_MASK, 0);
    }

    #[test]
    fn test_tc_bits_each_single_bit() {
        let tc = [
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
            BLK_TC_NOTIFY,
            BLK_TC_AHEAD,
            BLK_TC_META,
            BLK_TC_DISCARD,
            BLK_TC_DRV_DATA,
            BLK_TC_FUA,
        ];
        let mut or = 0u32;
        for (i, &v) in tc.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1u32 << i);
            or |= v;
        }
        // Low 16 bits.
        assert_eq!(or, 0xFFFF);
        // _END aliases the top bit (FUA).
        assert_eq!(BLK_TC_END, BLK_TC_FUA);
    }

    #[test]
    fn test_ta_codes_dense_1_to_17() {
        let ta = [
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
        for (i, &v) in ta.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        // Pairwise distinct (covered by density check above) but no zero.
        assert!(!ta.contains(&0));
    }

    #[test]
    fn test_setup_ioctls_in_blk_namespace() {
        for v in [
            BLKTRACESETUP,
            BLKTRACESTART,
            BLKTRACESTOP,
            BLKTRACETEARDOWN,
        ] {
            // _IO(0x12, ...) family.
            assert_eq!(v >> 8, 0x12);
        }
        // Setup / teardown form a pair (0x1276 / 0x1275).
        assert_eq!(BLKTRACESETUP - BLKTRACETEARDOWN, 1);
        // Start / stop form a pair (0x1274 / 0x1273).
        assert_eq!(BLKTRACESTART - BLKTRACESTOP, 1);
    }
}
