//! `<linux/sched/rt.h>` — Real-time scheduling constants.
//!
//! Real-time (RT) scheduling policies (SCHED_FIFO, SCHED_RR) provide
//! deterministic scheduling guarantees: an RT task always preempts
//! normal tasks, and among RT tasks, higher priority (1-99) always
//! wins. SCHED_FIFO tasks run until they block or yield; SCHED_RR
//! tasks get a time quantum. RT bandwidth throttling prevents RT
//! tasks from starving the system entirely.

// ---------------------------------------------------------------------------
// RT priority limits
// ---------------------------------------------------------------------------

/// Minimum RT priority (lowest among RT tasks).
pub const RT_PRIO_MIN: u32 = 1;
/// Maximum RT priority (highest, preempts all other tasks).
pub const RT_PRIO_MAX: u32 = 99;
/// Number of RT priority levels.
pub const RT_PRIO_LEVELS: u32 = 99;
/// Internal: MAX_RT_PRIO (boundary between RT and normal).
pub const MAX_RT_PRIO: u32 = 100;

// ---------------------------------------------------------------------------
// SCHED_RR time quantum
// ---------------------------------------------------------------------------

/// Default SCHED_RR time quantum in milliseconds.
pub const RR_TIMESLICE_MS: u32 = 100;
/// Minimum SCHED_RR time quantum in milliseconds.
pub const RR_TIMESLICE_MIN_MS: u32 = 1;

// ---------------------------------------------------------------------------
// RT bandwidth throttling (rt_runtime / rt_period)
// ---------------------------------------------------------------------------

/// Default RT period in microseconds (1 second).
pub const RT_PERIOD_US: u32 = 1_000_000;
/// Default RT runtime in microseconds (950ms of each 1s period).
pub const RT_RUNTIME_US: u32 = 950_000;
/// RT runtime disabled (unlimited, no throttling).
pub const RT_RUNTIME_UNLIMITED: i32 = -1;

// ---------------------------------------------------------------------------
// RT task states (for throttling)
// ---------------------------------------------------------------------------

/// RT task is running normally.
pub const RT_STATE_RUNNING: u32 = 0;
/// RT task is throttled (exceeded bandwidth).
pub const RT_STATE_THROTTLED: u32 = 1;
/// RT task group is empty (no runnable RT tasks).
pub const RT_STATE_IDLE: u32 = 2;

// ---------------------------------------------------------------------------
// RT group scheduling flags
// ---------------------------------------------------------------------------

/// RT group has runnable tasks.
pub const RT_GROUP_RUNNABLE: u32 = 0x01;
/// RT group is boosted (priority inheritance).
pub const RT_GROUP_BOOSTED: u32 = 0x02;
/// RT group bandwidth is being replenished.
pub const RT_GROUP_REPLENISH: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_range() {
        assert_eq!(RT_PRIO_MIN, 1);
        assert_eq!(RT_PRIO_MAX, 99);
        assert_eq!(RT_PRIO_LEVELS, RT_PRIO_MAX - RT_PRIO_MIN + 1);
        assert_eq!(MAX_RT_PRIO, RT_PRIO_MAX + 1);
    }

    #[test]
    fn test_timeslice_range() {
        assert!(RR_TIMESLICE_MIN_MS < RR_TIMESLICE_MS);
    }

    #[test]
    fn test_bandwidth_defaults() {
        assert!(RT_RUNTIME_US < RT_PERIOD_US);
        // 95% of the period is allowed for RT
        assert_eq!(RT_RUNTIME_US * 100 / RT_PERIOD_US, 95);
    }

    #[test]
    fn test_states_distinct() {
        let states = [RT_STATE_RUNNING, RT_STATE_THROTTLED, RT_STATE_IDLE];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_group_flags_no_overlap() {
        let flags = [RT_GROUP_RUNNABLE, RT_GROUP_BOOSTED, RT_GROUP_REPLENISH];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
