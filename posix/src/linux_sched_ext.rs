//! `<linux/sched/ext.h>` — Extensible scheduler (sched_ext) constants.
//!
//! sched_ext allows BPF programs to implement scheduling policies
//! in userspace. A BPF scheduler registers via struct_ops and the
//! kernel calls its hooks for task placement, preemption, and
//! migration decisions. Added in Linux 6.12.

// ---------------------------------------------------------------------------
// sched_ext operation flags
// ---------------------------------------------------------------------------

/// The scheduler wants to handle CPU idle balancing.
pub const SCX_OPS_KEEP_BUILTIN_IDLE: u64 = 1 << 0;
/// Enable per-CPU dispatch queues.
pub const SCX_OPS_ENQ_LAST: u64 = 1 << 1;
/// Enable exiting on error.
pub const SCX_OPS_ENQ_EXITING: u64 = 1 << 2;
/// Don't switch all tasks, only cgroup-targeted ones.
pub const SCX_OPS_SWITCH_PARTIAL: u64 = 1 << 3;

// ---------------------------------------------------------------------------
// Task dispatch flags
// ---------------------------------------------------------------------------

/// Dispatch to local CPU's DSQ (dispatch queue).
pub const SCX_DSQ_LOCAL: u64 = u64::MAX;
/// Dispatch to global DSQ.
pub const SCX_DSQ_GLOBAL: u64 = u64::MAX - 1;
/// Invalid DSQ.
pub const SCX_DSQ_INVALID: u64 = u64::MAX - 2;

// ---------------------------------------------------------------------------
// Task state flags
// ---------------------------------------------------------------------------

/// Task is being enqueued.
pub const SCX_TASK_QUEUED: u32 = 1 << 0;
/// Task is running.
pub const SCX_TASK_RUNNING: u32 = 1 << 1;
/// Task can be dispatched to any CPU.
pub const SCX_TASK_CURSOR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Exit kinds
// ---------------------------------------------------------------------------

/// Scheduler exited normally.
pub const SCX_EXIT_NONE: u32 = 0;
/// Scheduler exited due to error.
pub const SCX_EXIT_ERROR: u32 = 1;
/// Scheduler was unregistered (another loaded).
pub const SCX_EXIT_UNREGISTER: u32 = 2;
/// Scheduler unreg by sysrq.
pub const SCX_EXIT_SYSRQ: u32 = 3;

// ---------------------------------------------------------------------------
// Kick flags
// ---------------------------------------------------------------------------

/// Kick CPU (resched IPI).
pub const SCX_KICK_IDLE: u32 = 1 << 0;
/// Preempt current task on target CPU.
pub const SCX_KICK_PREEMPT: u32 = 1 << 1;
/// Wait for kicked CPU to reschedule.
pub const SCX_KICK_WAIT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Sysfs interface
// ---------------------------------------------------------------------------

/// sched_ext state file.
pub const SCHED_EXT_STATE: &str = "/sys/kernel/sched_ext/state";
/// Current scheduler name.
pub const SCHED_EXT_NAME: &str = "/sys/kernel/sched_ext/root/ops";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_flags_powers_of_two() {
        let flags = [
            SCX_OPS_KEEP_BUILTIN_IDLE, SCX_OPS_ENQ_LAST,
            SCX_OPS_ENQ_EXITING, SCX_OPS_SWITCH_PARTIAL,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_ops_flags_no_overlap() {
        let flags = [
            SCX_OPS_KEEP_BUILTIN_IDLE, SCX_OPS_ENQ_LAST,
            SCX_OPS_ENQ_EXITING, SCX_OPS_SWITCH_PARTIAL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dsq_sentinels_distinct() {
        let dsqs = [SCX_DSQ_LOCAL, SCX_DSQ_GLOBAL, SCX_DSQ_INVALID];
        for i in 0..dsqs.len() {
            for j in (i + 1)..dsqs.len() {
                assert_ne!(dsqs[i], dsqs[j]);
            }
        }
    }

    #[test]
    fn test_task_flags_powers_of_two() {
        let flags = [SCX_TASK_QUEUED, SCX_TASK_RUNNING, SCX_TASK_CURSOR];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_exit_kinds_distinct() {
        let exits = [SCX_EXIT_NONE, SCX_EXIT_ERROR, SCX_EXIT_UNREGISTER, SCX_EXIT_SYSRQ];
        for i in 0..exits.len() {
            for j in (i + 1)..exits.len() {
                assert_ne!(exits[i], exits[j]);
            }
        }
    }

    #[test]
    fn test_kick_flags_powers_of_two() {
        let flags = [SCX_KICK_IDLE, SCX_KICK_PREEMPT, SCX_KICK_WAIT];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_sysfs_paths_distinct() {
        assert_ne!(SCHED_EXT_STATE, SCHED_EXT_NAME);
    }
}
