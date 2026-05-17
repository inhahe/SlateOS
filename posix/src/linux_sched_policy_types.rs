//! `<linux/sched.h>` — Scheduling policy constants.
//!
//! Linux supports multiple scheduling policies: normal (CFS/EEVDF)
//! for regular tasks, real-time (FIFO/RR) for latency-sensitive
//! tasks, and deadline for tasks with explicit timing requirements.
//! Policies are set per-task via sched_setscheduler() or
//! sched_setattr(). Within each policy, tasks are further
//! differentiated by nice value (normal) or priority (RT).

// ---------------------------------------------------------------------------
// Scheduling policies
// ---------------------------------------------------------------------------

/// Normal scheduling (CFS/EEVDF, fair share).
pub const SCHED_NORMAL: u32 = 0;
/// FIFO real-time (highest priority runs until it yields/blocks).
pub const SCHED_FIFO: u32 = 1;
/// Round-robin real-time (like FIFO but with time quantum).
pub const SCHED_RR: u32 = 2;
/// Batch scheduling (like normal but favors throughput).
pub const SCHED_BATCH: u32 = 3;
/// Idle scheduling (only runs when no other tasks want CPU).
pub const SCHED_IDLE: u32 = 5;
/// Deadline scheduling (earliest deadline first).
pub const SCHED_DEADLINE: u32 = 6;

// ---------------------------------------------------------------------------
// Scheduling policy flags (OR'd with policy in sched_setattr)
// ---------------------------------------------------------------------------

/// Reset on fork (child doesn't inherit elevated policy).
pub const SCHED_FLAG_RESET_ON_FORK: u32 = 0x01;
/// Reclaim: allow deadline task to use unused bandwidth.
pub const SCHED_FLAG_RECLAIM: u32 = 0x02;
/// Use DL_OVERRUN signal (notify when deadline overrun).
pub const SCHED_FLAG_DL_OVERRUN: u32 = 0x04;
/// Keep all scheduling parameters (don't clear on setattr).
pub const SCHED_FLAG_KEEP_ALL: u32 = 0x08;
/// Keep scheduling policy (only change params).
pub const SCHED_FLAG_KEEP_POLICY: u32 = 0x10;
/// Keep scheduling parameters (only change policy).
pub const SCHED_FLAG_KEEP_PARAMS: u32 = 0x20;
/// Utilization clamping: set minimum utilization.
pub const SCHED_FLAG_UTIL_CLAMP_MIN: u32 = 0x40;
/// Utilization clamping: set maximum utilization.
pub const SCHED_FLAG_UTIL_CLAMP_MAX: u32 = 0x80;

// ---------------------------------------------------------------------------
// Nice value limits
// ---------------------------------------------------------------------------

/// Minimum nice value (highest priority for normal tasks).
pub const NICE_MIN: i32 = -20;
/// Maximum nice value (lowest priority for normal tasks).
pub const NICE_MAX: i32 = 19;
/// Default nice value.
pub const NICE_DEFAULT: i32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_distinct() {
        let policies = [
            SCHED_NORMAL, SCHED_FIFO, SCHED_RR,
            SCHED_BATCH, SCHED_IDLE, SCHED_DEADLINE,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            SCHED_FLAG_RESET_ON_FORK, SCHED_FLAG_RECLAIM,
            SCHED_FLAG_DL_OVERRUN, SCHED_FLAG_KEEP_ALL,
            SCHED_FLAG_KEEP_POLICY, SCHED_FLAG_KEEP_PARAMS,
            SCHED_FLAG_UTIL_CLAMP_MIN, SCHED_FLAG_UTIL_CLAMP_MAX,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_nice_range() {
        assert!(NICE_MIN < NICE_DEFAULT);
        assert!(NICE_DEFAULT < NICE_MAX);
    }
}
