//! `<linux/writeback.h>` — Dirty page writeback constants.
//!
//! The writeback subsystem manages writing dirty pages (modified but
//! not yet on disk) back to persistent storage. It balances between
//! allowing writes to accumulate in memory (for batching efficiency)
//! and ensuring data reaches disk promptly (for durability). The
//! flusher threads (formerly pdflush/bdflush) perform background
//! writeback; direct reclaim triggers foreground writeback under
//! memory pressure.

// ---------------------------------------------------------------------------
// Writeback reasons (why writeback was triggered)
// ---------------------------------------------------------------------------

/// Background writeback (periodic flusher).
pub const WB_REASON_BACKGROUND: u32 = 0;
/// vmscan reclaim (memory pressure).
pub const WB_REASON_VMSCAN: u32 = 1;
/// sync() syscall (user requested).
pub const WB_REASON_SYNC: u32 = 2;
/// Periodic timer expired.
pub const WB_REASON_PERIODIC: u32 = 3;
/// Laptop mode (flush before disk spins down).
pub const WB_REASON_LAPTOP_TIMER: u32 = 4;
/// Free more memory for allocation.
pub const WB_REASON_FS_FREE_SPACE: u32 = 5;
/// Forced by the block layer.
pub const WB_REASON_FORKER_THREAD: u32 = 6;

// ---------------------------------------------------------------------------
// Dirty page thresholds (defaults as percentage of memory)
// ---------------------------------------------------------------------------

/// Background dirty threshold (% of memory before flusher starts).
pub const DIRTY_BACKGROUND_RATIO_DEFAULT: u32 = 10;
/// Dirty ratio (% of memory before writer is throttled).
pub const DIRTY_RATIO_DEFAULT: u32 = 20;
/// Dirty expire centiseconds (how long before page is "expired").
pub const DIRTY_EXPIRE_CENTISECS_DEFAULT: u32 = 3000;
/// Dirty writeback centiseconds (flusher wakeup interval).
pub const DIRTY_WRITEBACK_CENTISECS_DEFAULT: u32 = 500;

// ---------------------------------------------------------------------------
// Writeback flags
// ---------------------------------------------------------------------------

/// Write whole pages (not just dirty portions).
pub const WB_FLAG_WHOLE_PAGE: u32 = 0x01;
/// Sync writeback (wait for completion).
pub const WB_FLAG_SYNC: u32 = 0x02;
/// Allow task to be killed during writeback.
pub const WB_FLAG_KILLABLE: u32 = 0x04;
/// Write range only (not entire file).
pub const WB_FLAG_RANGE: u32 = 0x08;
/// For reclaim (triggered by memory pressure).
pub const WB_FLAG_FOR_RECLAIM: u32 = 0x10;

// ---------------------------------------------------------------------------
// Writeback states
// ---------------------------------------------------------------------------

/// Idle (no dirty pages to write).
pub const WB_STATE_IDLE: u32 = 0;
/// Running (actively writing pages).
pub const WB_STATE_RUNNING: u32 = 1;
/// Registered (flusher thread exists but idle).
pub const WB_STATE_REGISTERED: u32 = 2;

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
            WB_REASON_VMSCAN,
            WB_REASON_SYNC,
            WB_REASON_PERIODIC,
            WB_REASON_LAPTOP_TIMER,
            WB_REASON_FS_FREE_SPACE,
            WB_REASON_FORKER_THREAD,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_dirty_thresholds() {
        assert!(DIRTY_BACKGROUND_RATIO_DEFAULT < DIRTY_RATIO_DEFAULT);
        assert!(DIRTY_RATIO_DEFAULT <= 100);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            WB_FLAG_WHOLE_PAGE,
            WB_FLAG_SYNC,
            WB_FLAG_KILLABLE,
            WB_FLAG_RANGE,
            WB_FLAG_FOR_RECLAIM,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [WB_STATE_IDLE, WB_STATE_RUNNING, WB_STATE_REGISTERED];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
