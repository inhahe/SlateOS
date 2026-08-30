//! `<sched.h>` — Scheduler parameter and policy constants.
//!
//! These constants define scheduling policies and priority
//! ranges used by `sched_setscheduler()`, `sched_setparam()`,
//! and related interfaces.

// ---------------------------------------------------------------------------
// Scheduling policies (SCHED_*)
// ---------------------------------------------------------------------------

/// Normal (default) time-sharing scheduling.
pub const SCHED_OTHER: u32 = 0;
/// FIFO real-time scheduling.
pub const SCHED_FIFO: u32 = 1;
/// Round-robin real-time scheduling.
pub const SCHED_RR: u32 = 2;
/// Batch scheduling (non-interactive, CPU-bound).
pub const SCHED_BATCH: u32 = 3;
/// Idle scheduling (lower priority than nice 19).
pub const SCHED_IDLE: u32 = 5;
/// Deadline scheduling (earliest deadline first).
pub const SCHED_DEADLINE: u32 = 6;

// ---------------------------------------------------------------------------
// Scheduling policy flags (OR'd with policy)
// ---------------------------------------------------------------------------

/// Reset scheduling policy on fork.
pub const SCHED_RESET_ON_FORK: u32 = 0x40000000;

// ---------------------------------------------------------------------------
// Priority ranges
// ---------------------------------------------------------------------------

/// Minimum real-time priority.
pub const SCHED_PRIORITY_MIN: u32 = 1;
/// Maximum real-time priority.
pub const SCHED_PRIORITY_MAX: u32 = 99;
/// Priority for SCHED_OTHER (always 0).
pub const SCHED_OTHER_PRIORITY: u32 = 0;

// ---------------------------------------------------------------------------
// sched_setattr / sched_getattr flags
// ---------------------------------------------------------------------------

/// Flag size (sizeof struct sched_attr).
pub const SCHED_ATTR_SIZE_V0: u32 = 48;
/// Flag size version 1 (extended).
pub const SCHED_ATTR_SIZE_V1: u32 = 56;

// ---------------------------------------------------------------------------
// sched_getaffinity / sched_setaffinity
// ---------------------------------------------------------------------------

/// Default CPU set size in bytes (for 1024 CPUs).
pub const CPU_SETSIZE_BYTES: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_distinct() {
        let policies = [
            SCHED_OTHER,
            SCHED_FIFO,
            SCHED_RR,
            SCHED_BATCH,
            SCHED_IDLE,
            SCHED_DEADLINE,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_other_is_zero() {
        assert_eq!(SCHED_OTHER, 0);
    }

    #[test]
    fn test_fifo_is_one() {
        assert_eq!(SCHED_FIFO, 1);
    }

    #[test]
    fn test_reset_on_fork_high_bit() {
        assert_eq!(SCHED_RESET_ON_FORK, 0x40000000);
        // Should not collide with any policy value
        assert_eq!(SCHED_RESET_ON_FORK & 0xFF, 0);
    }

    #[test]
    fn test_priority_range() {
        assert!(SCHED_PRIORITY_MIN <= SCHED_PRIORITY_MAX);
        assert_eq!(SCHED_PRIORITY_MIN, 1);
        assert_eq!(SCHED_PRIORITY_MAX, 99);
    }

    #[test]
    fn test_other_priority_is_zero() {
        assert_eq!(SCHED_OTHER_PRIORITY, 0);
    }

    #[test]
    fn test_attr_sizes() {
        assert!(SCHED_ATTR_SIZE_V0 > 0);
        assert!(SCHED_ATTR_SIZE_V1 > SCHED_ATTR_SIZE_V0);
    }

    #[test]
    fn test_cpu_setsize() {
        assert_eq!(CPU_SETSIZE_BYTES, 128);
    }
}
