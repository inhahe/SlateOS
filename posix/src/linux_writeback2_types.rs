//! `<linux/writeback.h>` (extended) — Writeback subsystem constants.
//!
//! The writeback subsystem flushes dirty pages from the page cache to
//! backing storage. It balances write throughput (batching many pages
//! for sequential I/O) against latency (not keeping dirty data in RAM
//! too long, which risks data loss on crash). Background writeback
//! runs continuously; forced writeback happens under memory pressure
//! or sync() calls. The BDI (backing device info) tracks per-device
//! writeback state.

// ---------------------------------------------------------------------------
// Writeback reasons (why writeback was triggered)
// ---------------------------------------------------------------------------

/// Background writeback (dirty ratio exceeded).
pub const WB_REASON_BACKGROUND: u32 = 0;
/// Explicit sync (fsync, sync, syncfs).
pub const WB_REASON_SYNC: u32 = 1;
/// Memory pressure (reclaimer needs pages).
pub const WB_REASON_VMSCAN: u32 = 2;
/// Periodic writeback timer (dirty_writeback_interval).
pub const WB_REASON_PERIODIC: u32 = 3;
/// Laptop mode (aggregate writes on inactivity).
pub const WB_REASON_LAPTOP_TIMER: u32 = 4;
/// FS-internal writeback request.
pub const WB_REASON_FS_FREE_SPACE: u32 = 5;
/// Fork (parent flushes before fork for CoW).
pub const WB_REASON_FORKER_THREAD: u32 = 6;
/// Foreign page on wrong BDI.
pub const WB_REASON_FOREIGN_FLUSH: u32 = 7;

// ---------------------------------------------------------------------------
// Writeback work flags
// ---------------------------------------------------------------------------

/// Write all dirty pages (not just old ones).
pub const WB_WORK_SYNC_ALL: u32 = 0x01;
/// Write pages for a specific inode only.
pub const WB_WORK_FOR_INODE: u32 = 0x02;
/// Write pages for kupdate (periodic timer).
pub const WB_WORK_FOR_KUPDATE: u32 = 0x04;
/// Write for background threshold.
pub const WB_WORK_FOR_BACKGROUND: u32 = 0x08;
/// Don't start new writeback, just wait.
pub const WB_WORK_NO_START: u32 = 0x10;

// ---------------------------------------------------------------------------
// BDI (Backing Device Info) capabilities
// ---------------------------------------------------------------------------

/// BDI supports writeback.
pub const BDI_CAP_WRITEBACK: u32 = 0x01;
/// BDI supports read-ahead.
pub const BDI_CAP_READ_AHEAD: u32 = 0x02;
/// BDI is congested (slow device).
pub const BDI_CAP_CONGESTED: u32 = 0x04;
/// BDI supports stable pages (no modification during writeback).
pub const BDI_CAP_STABLE_WRITES: u32 = 0x08;
/// BDI has no backing store (e.g., tmpfs).
pub const BDI_CAP_NO_WRITEBACK: u32 = 0x10;

// ---------------------------------------------------------------------------
// Dirty page thresholds (percentages)
// ---------------------------------------------------------------------------

/// Default dirty background ratio (% of RAM).
pub const DEFAULT_DIRTY_BACKGROUND_RATIO: u32 = 10;
/// Default dirty ratio (% of RAM, throttle point).
pub const DEFAULT_DIRTY_RATIO: u32 = 20;
/// Default writeback interval (centiseconds).
pub const DEFAULT_DIRTY_WRITEBACK_INTERVAL: u32 = 500;
/// Default expire interval (centiseconds, age for "old" dirty pages).
pub const DEFAULT_DIRTY_EXPIRE_INTERVAL: u32 = 3000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reasons_distinct() {
        let reasons = [
            WB_REASON_BACKGROUND,
            WB_REASON_SYNC,
            WB_REASON_VMSCAN,
            WB_REASON_PERIODIC,
            WB_REASON_LAPTOP_TIMER,
            WB_REASON_FS_FREE_SPACE,
            WB_REASON_FORKER_THREAD,
            WB_REASON_FOREIGN_FLUSH,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_work_flags_no_overlap() {
        let flags = [
            WB_WORK_SYNC_ALL,
            WB_WORK_FOR_INODE,
            WB_WORK_FOR_KUPDATE,
            WB_WORK_FOR_BACKGROUND,
            WB_WORK_NO_START,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_bdi_caps_no_overlap() {
        let caps = [
            BDI_CAP_WRITEBACK,
            BDI_CAP_READ_AHEAD,
            BDI_CAP_CONGESTED,
            BDI_CAP_STABLE_WRITES,
            BDI_CAP_NO_WRITEBACK,
        ];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_dirty_thresholds() {
        assert!(DEFAULT_DIRTY_BACKGROUND_RATIO < DEFAULT_DIRTY_RATIO);
        assert!(DEFAULT_DIRTY_RATIO <= 100);
        assert!(DEFAULT_DIRTY_WRITEBACK_INTERVAL > 0);
        assert!(DEFAULT_DIRTY_EXPIRE_INTERVAL > DEFAULT_DIRTY_WRITEBACK_INTERVAL);
    }
}
