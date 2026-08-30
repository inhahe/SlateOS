//! `<linux/pids_cgroup.h>` — PIDs cgroup controller constants.
//!
//! The PIDs cgroup controller limits the number of processes (tasks)
//! that can be created within a cgroup hierarchy. This prevents
//! fork bombs from consuming all available PIDs and affecting other
//! cgroups or the host system. The limit applies to all tasks (threads
//! + processes) in the cgroup and all its descendants. When the limit
//! is reached, fork()/clone() fails with EAGAIN.

// ---------------------------------------------------------------------------
// PIDs cgroup limits
// ---------------------------------------------------------------------------

/// Maximum PIDs limit value (effectively unlimited).
pub const PIDS_MAX_UNLIMITED: u64 = u64::MAX;
/// Default PIDs limit (no limit set).
pub const PIDS_MAX_DEFAULT: u64 = u64::MAX;
/// Minimum allowable PIDs limit.
pub const PIDS_MIN_LIMIT: u32 = 1;

// ---------------------------------------------------------------------------
// PIDs cgroup events
// ---------------------------------------------------------------------------

/// Number of times fork was denied due to PID limit.
pub const PIDS_EVENT_MAX: u32 = 0;

// ---------------------------------------------------------------------------
// PIDs cgroup states
// ---------------------------------------------------------------------------

/// PIDs cgroup is within limit.
pub const PIDS_STATE_OK: u32 = 0;
/// PIDs cgroup has reached its limit.
pub const PIDS_STATE_MAXED: u32 = 1;

// ---------------------------------------------------------------------------
// Task counting types
// ---------------------------------------------------------------------------

/// Count only processes (thread group leaders).
pub const PIDS_COUNT_PROCESSES: u32 = 0;
/// Count all tasks (processes + threads).
pub const PIDS_COUNT_ALL_TASKS: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unlimited_is_max() {
        assert_eq!(PIDS_MAX_UNLIMITED, u64::MAX);
        assert_eq!(PIDS_MAX_DEFAULT, u64::MAX);
    }

    #[test]
    fn test_min_limit_positive() {
        assert!(PIDS_MIN_LIMIT > 0);
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(PIDS_STATE_OK, PIDS_STATE_MAXED);
    }

    #[test]
    fn test_count_types_distinct() {
        assert_ne!(PIDS_COUNT_PROCESSES, PIDS_COUNT_ALL_TASKS);
    }
}
