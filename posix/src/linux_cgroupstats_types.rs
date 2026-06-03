//! `<linux/cgroupstats.h>` — Cgroup statistics netlink constants.
//!
//! The cgroupstats netlink interface provides aggregate task state
//! statistics for a cgroup: number of tasks sleeping, running,
//! stopped, and in uninterruptible sleep. This is a lightweight
//! alternative to iterating over all tasks in a cgroup to count
//! states. Used by monitoring tools and container runtimes to
//! quickly assess cgroup health without expensive per-task queries.

// ---------------------------------------------------------------------------
// Cgroupstats commands
// ---------------------------------------------------------------------------

/// Get cgroup statistics.
pub const CGROUPSTATS_CMD_GET: u32 = 1;
/// New statistics available (notification).
pub const CGROUPSTATS_CMD_NEW: u32 = 2;

// ---------------------------------------------------------------------------
// Cgroupstats attributes (CGROUPSTATS_TYPE_*)
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const CGROUPSTATS_TYPE_UNSPEC: u32 = 0;
/// Cgroup statistics structure.
pub const CGROUPSTATS_TYPE_CGROUP_STATS: u32 = 1;

// ---------------------------------------------------------------------------
// Cgroupstats command attributes (CGROUPSTATS_CMD_ATTR_*)
// ---------------------------------------------------------------------------

/// File descriptor of cgroup directory (for query).
pub const CGROUPSTATS_CMD_ATTR_FD: u32 = 1;

// ---------------------------------------------------------------------------
// Task states for cgroupstats counting
// ---------------------------------------------------------------------------

/// Task is running.
pub const CGROUP_TASK_RUNNING: u32 = 0;
/// Task is in interruptible sleep.
pub const CGROUP_TASK_SLEEPING: u32 = 1;
/// Task is in uninterruptible sleep (disk I/O).
pub const CGROUP_TASK_UNINTERRUPTIBLE: u32 = 2;
/// Task is stopped (SIGSTOP, ptrace).
pub const CGROUP_TASK_STOPPED: u32 = 3;

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// Cgroupstats version.
pub const CGROUPSTATS_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        assert_ne!(CGROUPSTATS_CMD_GET, CGROUPSTATS_CMD_NEW);
    }

    #[test]
    fn test_type_attrs_distinct() {
        assert_ne!(CGROUPSTATS_TYPE_UNSPEC, CGROUPSTATS_TYPE_CGROUP_STATS);
    }

    #[test]
    fn test_task_states_distinct() {
        let states = [
            CGROUP_TASK_RUNNING,
            CGROUP_TASK_SLEEPING,
            CGROUP_TASK_UNINTERRUPTIBLE,
            CGROUP_TASK_STOPPED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_version() {
        assert_eq!(CGROUPSTATS_VERSION, 1);
    }
}
