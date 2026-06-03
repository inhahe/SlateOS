//! `<linux/sched.h>` — Scheduler affinity and policy constants.
//!
//! Thread affinity controls which CPUs a thread is allowed to run on.
//! Combined with scheduling policies (FIFO, RR, DEADLINE, etc.), this
//! gives fine-grained control over CPU-bound workload placement.
//! Real-time policies guarantee bounded latency; normal policies
//! provide fairness.

// ---------------------------------------------------------------------------
// Scheduling policies (sched_setscheduler / sched_setattr)
// ---------------------------------------------------------------------------

/// Normal time-sharing (EEVDF/CFS).
pub const SCHED_NORMAL: u32 = 0;
/// FIFO real-time (run until yield/block/preempted by higher prio).
pub const SCHED_FIFO: u32 = 1;
/// Round-robin real-time (time-sliced within same priority).
pub const SCHED_RR: u32 = 2;
/// Batch (non-interactive, throughput-oriented).
pub const SCHED_BATCH: u32 = 3;
/// Idle (run only when no other work).
pub const SCHED_IDLE: u32 = 5;
/// Deadline (EDF-based, guaranteed CPU bandwidth).
pub const SCHED_DEADLINE: u32 = 6;

// ---------------------------------------------------------------------------
// Scheduling policy flags (sched_setattr flags field)
// ---------------------------------------------------------------------------

/// Reset on fork (child gets SCHED_NORMAL).
pub const SCHED_FLAG_RESET_ON_FORK: u32 = 0x01;
/// Allow DEADLINE task to reclaim unused bandwidth.
pub const SCHED_FLAG_RECLAIM: u32 = 0x02;
/// Deadline overrun generates SIGXCPU.
pub const SCHED_FLAG_DL_OVERRUN: u32 = 0x04;
/// Keep all scheduling parameters (partial update).
pub const SCHED_FLAG_KEEP_ALL: u32 = 0x08;
/// Keep scheduling parameters.
pub const SCHED_FLAG_KEEP_PARAMS: u32 = 0x10;
/// Use the utilization clamp values.
pub const SCHED_FLAG_UTIL_CLAMP_MIN: u32 = 0x20;
/// Use the utilization clamp values.
pub const SCHED_FLAG_UTIL_CLAMP_MAX: u32 = 0x40;

// ---------------------------------------------------------------------------
// Priority ranges
// ---------------------------------------------------------------------------

/// Minimum real-time priority (lowest).
pub const SCHED_PRIORITY_MIN: u32 = 1;
/// Maximum real-time priority (highest, for FIFO/RR).
pub const SCHED_PRIORITY_MAX: u32 = 99;
/// Nice value range: minimum (highest priority).
pub const NICE_MIN: i32 = -20;
/// Nice value range: maximum (lowest priority).
pub const NICE_MAX: i32 = 19;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_distinct() {
        let pols = [
            SCHED_NORMAL,
            SCHED_FIFO,
            SCHED_RR,
            SCHED_BATCH,
            SCHED_IDLE,
            SCHED_DEADLINE,
        ];
        for i in 0..pols.len() {
            for j in (i + 1)..pols.len() {
                assert_ne!(pols[i], pols[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            SCHED_FLAG_RESET_ON_FORK,
            SCHED_FLAG_RECLAIM,
            SCHED_FLAG_DL_OVERRUN,
            SCHED_FLAG_KEEP_ALL,
            SCHED_FLAG_KEEP_PARAMS,
            SCHED_FLAG_UTIL_CLAMP_MIN,
            SCHED_FLAG_UTIL_CLAMP_MAX,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_priority_range() {
        assert!(SCHED_PRIORITY_MIN < SCHED_PRIORITY_MAX);
        assert_eq!(SCHED_PRIORITY_MIN, 1);
        assert_eq!(SCHED_PRIORITY_MAX, 99);
    }

    #[test]
    fn test_nice_range() {
        assert!(NICE_MIN < NICE_MAX);
        assert_eq!(NICE_MIN, -20);
        assert_eq!(NICE_MAX, 19);
    }

    #[test]
    fn test_normal_is_zero() {
        assert_eq!(SCHED_NORMAL, 0);
    }
}
