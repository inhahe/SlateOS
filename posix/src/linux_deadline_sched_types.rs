//! `<linux/sched/deadline.h>` — SCHED_DEADLINE scheduler constants.
//!
//! SCHED_DEADLINE implements Earliest Deadline First (EDF) scheduling
//! with Constant Bandwidth Server (CBS) admission control. Each
//! deadline task specifies: runtime (worst-case execution time per
//! period), deadline (relative deadline from activation), and period
//! (how often the task activates). The scheduler guarantees the task
//! gets its runtime within each deadline, preventing deadline tasks
//! from stealing more than their declared share of CPU.

// ---------------------------------------------------------------------------
// SCHED_DEADLINE parameter limits
// ---------------------------------------------------------------------------

/// Minimum runtime in nanoseconds (1 microsecond).
pub const DL_RUNTIME_MIN_NS: u64 = 1_000;
/// Minimum period/deadline in nanoseconds (1 microsecond).
pub const DL_PERIOD_MIN_NS: u64 = 1_000;
/// Maximum period/deadline in nanoseconds (~4 seconds).
pub const DL_PERIOD_MAX_NS: u64 = 4_000_000_000;

// ---------------------------------------------------------------------------
// SCHED_DEADLINE task states
// ---------------------------------------------------------------------------

/// Task is within its current period (has budget remaining).
pub const DL_STATE_ACTIVE: u32 = 0;
/// Task exhausted its runtime (throttled until next period).
pub const DL_STATE_THROTTLED: u32 = 1;
/// Task's period has not started yet (new activation pending).
pub const DL_STATE_INACTIVE: u32 = 2;

// ---------------------------------------------------------------------------
// SCHED_DEADLINE flags
// ---------------------------------------------------------------------------

/// Task can reclaim unused bandwidth from other deadline tasks.
pub const DL_FLAG_RECLAIM: u32 = 0x01;
/// Generate SIGXCPU on runtime overrun.
pub const DL_FLAG_OVERRUN: u32 = 0x02;
/// Runtime is measured in CPU time (not wall time).
pub const DL_FLAG_CPUTIME: u32 = 0x04;

// ---------------------------------------------------------------------------
// Bandwidth management
// ---------------------------------------------------------------------------

/// Maximum total bandwidth (sum of runtime/period for all DL tasks).
/// 95% to leave 5% for system tasks.
pub const DL_BW_MAX_PERCENT: u32 = 95;
/// Bandwidth check: admission would exceed capacity.
pub const DL_BW_OVERFLOW: u32 = 1;
/// Bandwidth check: admission is accepted.
pub const DL_BW_OK: u32 = 0;

// ---------------------------------------------------------------------------
// CBS (Constant Bandwidth Server) parameters
// ---------------------------------------------------------------------------

/// CBS replenishment: full replenish (runtime restored to maximum).
pub const CBS_REPLENISH_FULL: u32 = 0;
/// CBS replenishment: partial (carry over unused time).
pub const CBS_REPLENISH_PARTIAL: u32 = 1;
/// CBS rule: hard (never exceed declared runtime per period).
pub const CBS_RULE_HARD: u32 = 0;
/// CBS rule: soft (may exceed if bandwidth available).
pub const CBS_RULE_SOFT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_period_limits() {
        assert!(DL_PERIOD_MIN_NS < DL_PERIOD_MAX_NS);
        assert!(DL_RUNTIME_MIN_NS <= DL_PERIOD_MIN_NS);
    }

    #[test]
    fn test_states_distinct() {
        let states = [DL_STATE_ACTIVE, DL_STATE_THROTTLED, DL_STATE_INACTIVE];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [DL_FLAG_RECLAIM, DL_FLAG_OVERRUN, DL_FLAG_CPUTIME];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_bandwidth_values() {
        assert!(DL_BW_MAX_PERCENT <= 100);
        assert_ne!(DL_BW_OVERFLOW, DL_BW_OK);
    }

    #[test]
    fn test_cbs_values_distinct() {
        assert_ne!(CBS_REPLENISH_FULL, CBS_REPLENISH_PARTIAL);
        assert_ne!(CBS_RULE_HARD, CBS_RULE_SOFT);
    }
}
