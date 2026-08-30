//! `<linux/sched/fair.h>` — CFS/EEVDF scheduler constants.
//!
//! The Completely Fair Scheduler (CFS) — evolved to EEVDF (Earliest
//! Eligible Virtual Deadline First) in 6.6 — is the default scheduler
//! for SCHED_NORMAL tasks. It uses a virtual runtime (vruntime) to
//! ensure each task gets its fair share of CPU proportional to its
//! weight (derived from nice value). EEVDF adds a virtual deadline
//! concept to improve latency for interactive tasks while maintaining
//! throughput fairness.

// ---------------------------------------------------------------------------
// CFS weight values (from nice levels)
// ---------------------------------------------------------------------------

/// Weight for nice 0 (baseline, ~1024 in older kernels, scaled).
pub const SCHED_WEIGHT_NICE_0: u32 = 1024;
/// Minimum weight (nice +19).
pub const SCHED_WEIGHT_MIN: u32 = 15;
/// Maximum weight (nice -20).
pub const SCHED_WEIGHT_MAX: u32 = 88761;

// ---------------------------------------------------------------------------
// CFS tunable defaults (in nanoseconds)
// ---------------------------------------------------------------------------

/// Minimum granularity: minimum time a task runs before preemption (3ms).
pub const SCHED_MIN_GRANULARITY_NS: u32 = 3_000_000;
/// Wakeup granularity: minimum advantage needed to preempt current (4ms).
pub const SCHED_WAKEUP_GRANULARITY_NS: u32 = 4_000_000;
/// Latency target: scheduling period for n tasks (24ms default).
pub const SCHED_LATENCY_NS: u32 = 24_000_000;

// ---------------------------------------------------------------------------
// EEVDF-specific constants (Linux 6.6+)
// ---------------------------------------------------------------------------

/// Base slice for EEVDF eligible computation (3ms).
pub const EEVDF_BASE_SLICE_NS: u32 = 3_000_000;
/// Minimum request length for lag computation.
pub const EEVDF_MIN_REQUEST_NS: u32 = 100_000;

// ---------------------------------------------------------------------------
// Load tracking constants
// ---------------------------------------------------------------------------

/// PELT (Per-Entity Load Tracking) period in ms.
pub const PELT_PERIOD_MS: u32 = 32;
/// Maximum PELT load value (represents 100% utilization).
pub const PELT_MAX_LOAD: u32 = 1024;
/// Number of PELT half-life periods to consider (decay window).
pub const PELT_DECAY_PERIODS: u32 = 32;

// ---------------------------------------------------------------------------
// Load balance flags
// ---------------------------------------------------------------------------

/// Balance by moving tasks to less loaded CPUs.
pub const LB_MOVE_TASKS: u32 = 0x01;
/// Balance by waking tasks on less loaded CPUs.
pub const LB_WAKE_AFFINE: u32 = 0x02;
/// Allow NUMA-aware balancing.
pub const LB_NUMA: u32 = 0x04;
/// Balance is urgent (CPU is about to go idle).
pub const LB_IDLE: u32 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weight_range() {
        assert!(SCHED_WEIGHT_MIN < SCHED_WEIGHT_NICE_0);
        assert!(SCHED_WEIGHT_NICE_0 < SCHED_WEIGHT_MAX);
    }

    #[test]
    fn test_granularity_values() {
        assert!(SCHED_MIN_GRANULARITY_NS > 0);
        assert!(SCHED_WAKEUP_GRANULARITY_NS > 0);
        assert!(SCHED_LATENCY_NS > SCHED_MIN_GRANULARITY_NS);
    }

    #[test]
    fn test_eevdf_values() {
        assert!(EEVDF_BASE_SLICE_NS > 0);
        assert!(EEVDF_MIN_REQUEST_NS > 0);
        assert!(EEVDF_BASE_SLICE_NS > EEVDF_MIN_REQUEST_NS);
    }

    #[test]
    fn test_pelt_values() {
        assert!(PELT_PERIOD_MS > 0);
        assert!(PELT_MAX_LOAD > 0);
        assert!(PELT_DECAY_PERIODS > 0);
    }

    #[test]
    fn test_lb_flags_no_overlap() {
        let flags = [LB_MOVE_TASKS, LB_WAKE_AFFINE, LB_NUMA, LB_IDLE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
