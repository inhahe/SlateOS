//! `<linux/workqueue.h>` — Workqueue framework constants.
//!
//! Workqueues provide a mechanism to defer work to process context.
//! Unlike softirqs/tasklets (which run in interrupt context), work
//! items run in kernel threads and can sleep. The framework manages
//! a pool of worker threads (kworker/u:*) that execute queued work
//! items. Workqueues can be bound to specific CPUs, unbound (for
//! long-running work), or ordered (serialized execution).

// ---------------------------------------------------------------------------
// Workqueue flags (for alloc_workqueue)
// ---------------------------------------------------------------------------

/// Unbound: work items can run on any CPU.
pub const WQ_UNBOUND: u32 = 0x0000_0002;
/// Freezable: work pauses during system suspend.
pub const WQ_FREEZABLE: u32 = 0x0000_0004;
/// Mem-reclaim: workqueue can be used during memory reclaim.
pub const WQ_MEM_RECLAIM: u32 = 0x0000_0008;
/// High priority: use high-priority worker pool.
pub const WQ_HIGHPRI: u32 = 0x0000_0010;
/// CPU-intensive: work items take a long time, use dedicated threads.
pub const WQ_CPU_INTENSIVE: u32 = 0x0000_0020;
/// Power-efficient: allow system to batch work for power saving.
pub const WQ_POWER_EFFICIENT: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// Work item flags (internal state)
// ---------------------------------------------------------------------------

/// Work item is pending (queued but not executing).
pub const WORK_STRUCT_PENDING: u32 = 0x01;
/// Work item is currently executing.
pub const WORK_STRUCT_EXECUTING: u32 = 0x02;
/// Work item has a linked list of dependent items.
pub const WORK_STRUCT_LINKED: u32 = 0x04;

// ---------------------------------------------------------------------------
// Workqueue concurrency limits
// ---------------------------------------------------------------------------

/// Default maximum active work items per CPU.
pub const WQ_MAX_ACTIVE_DEFAULT: u32 = 256;
/// Minimum active work items.
pub const WQ_MAX_ACTIVE_MIN: u32 = 1;
/// Maximum active work items (unbound).
pub const WQ_MAX_ACTIVE_MAX: u32 = 512;
/// Ordered workqueue (max_active = 1, serialized).
pub const WQ_MAX_ACTIVE_ORDERED: u32 = 1;

// ---------------------------------------------------------------------------
// Delayed work timer modes
// ---------------------------------------------------------------------------

/// Delay in jiffies.
pub const DELAY_JIFFIES: u32 = 0;
/// Delay in milliseconds.
pub const DELAY_MS: u32 = 1;
/// Delay in microseconds.
pub const DELAY_US: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wq_flags_no_overlap() {
        let flags = [
            WQ_UNBOUND, WQ_FREEZABLE, WQ_MEM_RECLAIM,
            WQ_HIGHPRI, WQ_CPU_INTENSIVE, WQ_POWER_EFFICIENT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_work_flags_no_overlap() {
        let flags = [WORK_STRUCT_PENDING, WORK_STRUCT_EXECUTING, WORK_STRUCT_LINKED];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_concurrency_limits() {
        assert!(WQ_MAX_ACTIVE_MIN <= WQ_MAX_ACTIVE_ORDERED);
        assert!(WQ_MAX_ACTIVE_ORDERED <= WQ_MAX_ACTIVE_DEFAULT);
        assert!(WQ_MAX_ACTIVE_DEFAULT <= WQ_MAX_ACTIVE_MAX);
    }

    #[test]
    fn test_delay_modes_distinct() {
        let modes = [DELAY_JIFFIES, DELAY_MS, DELAY_US];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
