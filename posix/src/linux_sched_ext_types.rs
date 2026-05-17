//! `<linux/sched/ext.h>` — Extensible scheduler (sched_ext) constants.
//!
//! sched_ext (Linux 6.12+) allows loading custom BPF-based CPU
//! schedulers at runtime. A BPF program implements scheduling
//! callbacks (enqueue, dispatch, pick, etc.) and the kernel invokes
//! them for task placement decisions. Used for experimenting with
//! new scheduling policies without rebuilding the kernel.

// ---------------------------------------------------------------------------
// sched_ext operations (BPF callback IDs)
// ---------------------------------------------------------------------------

/// Task is enqueued (ready to run).
pub const SCX_OP_ENQUEUE: u32 = 0;
/// Task is dequeued (no longer runnable).
pub const SCX_OP_DEQUEUE: u32 = 1;
/// Dispatch tasks to a CPU (fill the local queue).
pub const SCX_OP_DISPATCH: u32 = 2;
/// Pick which task to run next.
pub const SCX_OP_SELECT_CPU: u32 = 3;
/// CPU going idle callback.
pub const SCX_OP_CPU_IDLE: u32 = 4;
/// Task started running.
pub const SCX_OP_RUNNING: u32 = 5;
/// Task stopped running.
pub const SCX_OP_STOPPING: u32 = 6;
/// Task is being created.
pub const SCX_OP_INIT_TASK: u32 = 7;
/// Task is being destroyed.
pub const SCX_OP_EXIT_TASK: u32 = 8;
/// Scheduler is being enabled.
pub const SCX_OP_INIT: u32 = 9;
/// Scheduler is being disabled.
pub const SCX_OP_EXIT: u32 = 10;

// ---------------------------------------------------------------------------
// sched_ext flags
// ---------------------------------------------------------------------------

/// Allow scheduler to preempt tasks.
pub const SCX_OPS_ALLOW_PREEMPT: u32 = 1 << 0;
/// Scheduler manages its own CPU selection.
pub const SCX_OPS_SWITCH_PARTIAL: u32 = 1 << 1;
/// Keep built-in idle tracking.
pub const SCX_OPS_KEEP_BUILTIN_IDLE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Dispatch queue IDs
// ---------------------------------------------------------------------------

/// Global shared dispatch queue.
pub const SCX_DSQ_GLOBAL: u64 = 0;
/// Per-CPU local dispatch queue.
pub const SCX_DSQ_LOCAL: u64 = u64::MAX;
/// Per-CPU local dispatch queue (on specific CPU).
pub const SCX_DSQ_LOCAL_ON: u64 = u64::MAX - 1;

// ---------------------------------------------------------------------------
// Exit reasons
// ---------------------------------------------------------------------------

/// Normal exit (scheduler unloaded cleanly).
pub const SCX_EXIT_NONE: u32 = 0;
/// Exit due to an error.
pub const SCX_EXIT_ERROR: u32 = 1024;
/// Exit requested by the BPF program.
pub const SCX_EXIT_UNREG_BPF: u32 = 1025;
/// Exit due to kernel request.
pub const SCX_EXIT_UNREG_KERN: u32 = 1026;
/// Exit by sysrq.
pub const SCX_EXIT_SYSRQ: u32 = 1027;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_distinct() {
        let ops = [
            SCX_OP_ENQUEUE, SCX_OP_DEQUEUE, SCX_OP_DISPATCH,
            SCX_OP_SELECT_CPU, SCX_OP_CPU_IDLE, SCX_OP_RUNNING,
            SCX_OP_STOPPING, SCX_OP_INIT_TASK, SCX_OP_EXIT_TASK,
            SCX_OP_INIT, SCX_OP_EXIT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            SCX_OPS_ALLOW_PREEMPT,
            SCX_OPS_SWITCH_PARTIAL,
            SCX_OPS_KEEP_BUILTIN_IDLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dsq_ids_distinct() {
        assert_ne!(SCX_DSQ_GLOBAL, SCX_DSQ_LOCAL);
        assert_ne!(SCX_DSQ_LOCAL, SCX_DSQ_LOCAL_ON);
        assert_ne!(SCX_DSQ_GLOBAL, SCX_DSQ_LOCAL_ON);
    }

    #[test]
    fn test_exit_reasons_distinct() {
        let exits = [
            SCX_EXIT_NONE, SCX_EXIT_ERROR,
            SCX_EXIT_UNREG_BPF, SCX_EXIT_UNREG_KERN,
            SCX_EXIT_SYSRQ,
        ];
        for i in 0..exits.len() {
            for j in (i + 1)..exits.len() {
                assert_ne!(exits[i], exits[j]);
            }
        }
    }
}
