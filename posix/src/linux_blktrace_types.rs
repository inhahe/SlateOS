//! `<linux/blktrace_api.h>` — Block layer tracing (blktrace) constants.
//!
//! blktrace provides fine-grained tracing of block I/O operations as
//! they flow through the I/O stack. Each trace event records what
//! happened (queue, merge, issue, complete, etc.) to a specific I/O
//! request. Used by `blktrace`/`blkparse` tools for I/O performance
//! analysis and debugging.

// ---------------------------------------------------------------------------
// Trace action categories (high bits of action field)
// ---------------------------------------------------------------------------

/// Request queued in scheduler.
pub const BLK_TC_READ: u32 = 1 << 0;
/// Write request.
pub const BLK_TC_WRITE: u32 = 1 << 1;
/// Flush request.
pub const BLK_TC_FLUSH: u32 = 1 << 2;
/// Synchronous request.
pub const BLK_TC_SYNC: u32 = 1 << 3;
/// Queue operation.
pub const BLK_TC_QUEUE: u32 = 1 << 4;
/// Requeue operation.
pub const BLK_TC_REQUEUE: u32 = 1 << 5;
/// Issue to device.
pub const BLK_TC_ISSUE: u32 = 1 << 6;
/// Completion from device.
pub const BLK_TC_COMPLETE: u32 = 1 << 7;
/// Filesystem operation.
pub const BLK_TC_FS: u32 = 1 << 8;
/// Page cache operation.
pub const BLK_TC_PC: u32 = 1 << 9;
/// Notify event (misc info).
pub const BLK_TC_NOTIFY: u32 = 1 << 10;
/// Ahead (readahead) operation.
pub const BLK_TC_AHEAD: u32 = 1 << 11;
/// Metadata operation.
pub const BLK_TC_META: u32 = 1 << 12;
/// Discard request.
pub const BLK_TC_DISCARD: u32 = 1 << 13;
/// DRV-specific data.
pub const BLK_TC_DRV_DATA: u32 = 1 << 14;
/// FUA (Force Unit Access) request.
pub const BLK_TC_FUA: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Trace actions (low 16 bits)
// ---------------------------------------------------------------------------

/// I/O was queued.
pub const BLK_TA_QUEUE: u32 = 1;
/// I/O back-merged with existing request.
pub const BLK_TA_BACKMERGE: u32 = 2;
/// I/O front-merged with existing request.
pub const BLK_TA_FRONTMERGE: u32 = 3;
/// Get request from free list.
pub const BLK_TA_GETRQ: u32 = 4;
/// Sleep waiting for request.
pub const BLK_TA_SLEEPRQ: u32 = 5;
/// Request requeued.
pub const BLK_TA_REQUEUE: u32 = 6;
/// Request issued to driver.
pub const BLK_TA_ISSUE: u32 = 7;
/// Request completed.
pub const BLK_TA_COMPLETE: u32 = 8;
/// Plug the queue.
pub const BLK_TA_PLUG: u32 = 9;
/// Unplug the queue (I/O count).
pub const BLK_TA_UNPLUG_IO: u32 = 10;
/// Unplug the queue (timer).
pub const BLK_TA_UNPLUG_TIMER: u32 = 11;
/// Request inserted into queue.
pub const BLK_TA_INSERT: u32 = 12;
/// Request split.
pub const BLK_TA_SPLIT: u32 = 13;
/// Bounce buffer allocated.
pub const BLK_TA_BOUNCE: u32 = 14;
/// Request remapped (DM/MD).
pub const BLK_TA_REMAP: u32 = 15;
/// Request aborted.
pub const BLK_TA_ABORT: u32 = 16;
/// DRV-specific data event.
pub const BLK_TA_DRV_DATA: u32 = 17;

// ---------------------------------------------------------------------------
// Trace setup ioctl
// ---------------------------------------------------------------------------

/// Set up blktrace for a device.
pub const BLKTRACESETUP: u32 = 0xC048_1200;
/// Start tracing.
pub const BLKTRACESTART: u32 = 0x1201;
/// Stop tracing.
pub const BLKTRACESTOP: u32 = 0x1202;
/// Tear down blktrace.
pub const BLKTRACETEARDOWN: u32 = 0x1203;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tc_categories_no_overlap() {
        let cats = [
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
        for i in 0..cats.len() {
            assert!(cats[i].is_power_of_two());
            for j in (i + 1)..cats.len() {
                assert_eq!(cats[i] & cats[j], 0);
            }
        }
    }

    #[test]
    fn test_ta_actions_distinct() {
        let actions = [
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
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [BLKTRACESETUP, BLKTRACESTART, BLKTRACESTOP, BLKTRACETEARDOWN];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ta_actions_sequential() {
        assert_eq!(BLK_TA_QUEUE, 1);
        assert_eq!(BLK_TA_DRV_DATA, 17);
    }
}
