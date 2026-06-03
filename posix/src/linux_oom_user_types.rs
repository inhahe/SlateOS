//! `/proc/<pid>/oom_*` and OOM-killer ABI.
//!
//! Linux's out-of-memory killer scores each process and picks the
//! highest-scoring victim when the kernel runs out of memory.
//! `systemd-oomd`, container runtimes, and database tuning guides
//! all interact with the user-tunable knobs defined here.

// ---------------------------------------------------------------------------
// Per-process `/proc/<pid>/` knobs
// ---------------------------------------------------------------------------

/// New-style (since 2.6.36): -1000..=1000 adjustment added to the score.
/// Setting `-1000` disables the OOM killer for the process.
pub const PROC_PID_OOM_SCORE_ADJ: &str = "/proc/self/oom_score_adj";
/// Legacy (deprecated since 2.6.36): -16..=15 adjustment.
pub const PROC_PID_OOM_ADJ: &str = "/proc/self/oom_adj";
/// Read-only: the badness score the kernel computed for this process.
pub const PROC_PID_OOM_SCORE: &str = "/proc/self/oom_score";

// ---------------------------------------------------------------------------
// `oom_score_adj` bounds
// ---------------------------------------------------------------------------

pub const OOM_SCORE_ADJ_MIN: i32 = -1000;
pub const OOM_SCORE_ADJ_MAX: i32 = 1000;
/// Default for new processes.
pub const OOM_SCORE_ADJ_DEFAULT: i32 = 0;
/// Setting `oom_score_adj` to this value makes the process effectively
/// immune to the OOM killer.
pub const OOM_SCORE_ADJ_DISABLE: i32 = -1000;

// ---------------------------------------------------------------------------
// Legacy `oom_adj` bounds (kept for old tooling)
// ---------------------------------------------------------------------------

pub const OOM_ADJUST_MIN: i32 = -16;
pub const OOM_ADJUST_MAX: i32 = 15;
/// Setting `oom_adj` to this value disables the killer (legacy form).
pub const OOM_DISABLE: i32 = -17;

// ---------------------------------------------------------------------------
// Sysctl knobs
// ---------------------------------------------------------------------------

pub const SYSCTL_PANIC_ON_OOM: &str = "/proc/sys/vm/panic_on_oom";
pub const SYSCTL_OOM_KILL_ALLOCATING_TASK: &str =
    "/proc/sys/vm/oom_kill_allocating_task";
pub const SYSCTL_OOM_DUMP_TASKS: &str = "/proc/sys/vm/oom_dump_tasks";
pub const SYSCTL_OVERCOMMIT_MEMORY: &str = "/proc/sys/vm/overcommit_memory";
pub const SYSCTL_OVERCOMMIT_RATIO: &str = "/proc/sys/vm/overcommit_ratio";
pub const SYSCTL_OVERCOMMIT_KBYTES: &str = "/proc/sys/vm/overcommit_kbytes";

// ---------------------------------------------------------------------------
// `overcommit_memory` values
// ---------------------------------------------------------------------------

pub const OVERCOMMIT_GUESS: u32 = 0;
pub const OVERCOMMIT_ALWAYS: u32 = 1;
pub const OVERCOMMIT_NEVER: u32 = 2;

// ---------------------------------------------------------------------------
// `panic_on_oom` values
// ---------------------------------------------------------------------------

pub const PANIC_ON_OOM_OFF: u32 = 0;
pub const PANIC_ON_OOM_CONSTRAINED: u32 = 1;
pub const PANIC_ON_OOM_ALWAYS: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proc_paths_look_right() {
        assert!(PROC_PID_OOM_SCORE_ADJ.ends_with("/oom_score_adj"));
        assert!(PROC_PID_OOM_ADJ.ends_with("/oom_adj"));
        assert!(PROC_PID_OOM_SCORE.ends_with("/oom_score"));
    }

    #[test]
    fn test_score_adj_symmetric_around_zero() {
        // The new-style knob is a symmetric ±1000 range.
        assert_eq!(OOM_SCORE_ADJ_MIN, -1000);
        assert_eq!(OOM_SCORE_ADJ_MAX, 1000);
        assert_eq!(OOM_SCORE_ADJ_DEFAULT, 0);
        assert_eq!(OOM_SCORE_ADJ_DISABLE, OOM_SCORE_ADJ_MIN);
        // Range fits in i32 trivially.
        assert!(OOM_SCORE_ADJ_MIN < OOM_SCORE_ADJ_MAX);
    }

    #[test]
    fn test_legacy_oom_adj_bounds() {
        // Old knob was -16..=15; DISABLE was a special -17 sentinel.
        assert_eq!(OOM_ADJUST_MIN, -16);
        assert_eq!(OOM_ADJUST_MAX, 15);
        assert_eq!(OOM_DISABLE, -17);
        assert!(OOM_DISABLE < OOM_ADJUST_MIN);
    }

    #[test]
    fn test_sysctl_paths_under_vm() {
        let p = [
            SYSCTL_PANIC_ON_OOM,
            SYSCTL_OOM_KILL_ALLOCATING_TASK,
            SYSCTL_OOM_DUMP_TASKS,
            SYSCTL_OVERCOMMIT_MEMORY,
            SYSCTL_OVERCOMMIT_RATIO,
            SYSCTL_OVERCOMMIT_KBYTES,
        ];
        for path in p {
            assert!(path.starts_with("/proc/sys/vm/"));
        }
    }

    #[test]
    fn test_overcommit_modes_dense() {
        assert_eq!(OVERCOMMIT_GUESS, 0);
        assert_eq!(OVERCOMMIT_ALWAYS, 1);
        assert_eq!(OVERCOMMIT_NEVER, 2);
    }

    #[test]
    fn test_panic_on_oom_modes_dense() {
        assert_eq!(PANIC_ON_OOM_OFF, 0);
        assert_eq!(PANIC_ON_OOM_CONSTRAINED, 1);
        assert_eq!(PANIC_ON_OOM_ALWAYS, 2);
    }
}
