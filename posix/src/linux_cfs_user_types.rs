//! `kernel/sched/fair.c` — CFS (Completely Fair Scheduler) tunables.
//!
//! CFS is the default Linux scheduler for SCHED_NORMAL tasks. It
//! tracks per-task virtual runtime (vruntime) and always picks the
//! task with the smallest vruntime, weighted by nice value. The
//! constants below are the runtime tunables (kernel/sched/debug.c)
//! and the nice-to-weight table that defines the weighting.

// ---------------------------------------------------------------------------
// Period and granularity tunables (nanoseconds)
// ---------------------------------------------------------------------------

/// Target latency for the scheduling period (6 ms).
pub const SYSCTL_SCHED_LATENCY_NS: u64 = 6_000_000;

/// Minimum slice (0.75 ms) — preserves cache when many runnable tasks exist.
pub const SYSCTL_SCHED_MIN_GRANULARITY_NS: u64 = 750_000;

/// Wake-up granularity (1 ms) — preempt-on-wake threshold.
pub const SYSCTL_SCHED_WAKEUP_GRANULARITY_NS: u64 = 1_000_000;

/// Migration cost — minimum time before migrating a task off a CPU.
pub const SYSCTL_SCHED_MIGRATION_COST_NS: u64 = 500_000;

// ---------------------------------------------------------------------------
// Nice / weight mapping
// ---------------------------------------------------------------------------

/// Nice value 0 maps to this weight (NICE_0_LOAD).
pub const NICE_0_LOAD: u32 = 1_024;

/// Maximum nice value (lowest priority).
pub const MAX_NICE: i32 = 19;

/// Minimum nice value (highest priority).
pub const MIN_NICE: i32 = -20;

/// Total span of nice values (40).
pub const NICE_WIDTH: u32 = 40;

// ---------------------------------------------------------------------------
// CFS load-balance tunables
// ---------------------------------------------------------------------------

/// Load-balance interval (ms) at the lowest sched-domain level.
pub const SYSCTL_SCHED_LB_INTERVAL_MIN_MS: u32 = 1;

/// Load-balance interval (ms) at the highest sched-domain level.
pub const SYSCTL_SCHED_LB_INTERVAL_MAX_MS: u32 = 16;

// ---------------------------------------------------------------------------
// CFS bandwidth (cgroup-v2 cpu.max)
// ---------------------------------------------------------------------------

/// Minimum CFS-bandwidth quota (1 ms).
pub const CFS_BANDWIDTH_QUOTA_MIN_US: u64 = 1_000;

/// Default CFS-bandwidth period (100 ms).
pub const CFS_BANDWIDTH_PERIOD_DEFAULT_US: u64 = 100_000;

/// Maximum CFS-bandwidth period (1 s).
pub const CFS_BANDWIDTH_PERIOD_MAX_US: u64 = 1_000_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunable_period_ordering() {
        // wakeup-granularity < min-granularity < latency.
        assert!(SYSCTL_SCHED_WAKEUP_GRANULARITY_NS > SYSCTL_SCHED_MIN_GRANULARITY_NS);
        assert!(SYSCTL_SCHED_MIN_GRANULARITY_NS < SYSCTL_SCHED_LATENCY_NS);
        // Migration cost is small compared to the scheduling period.
        assert!(SYSCTL_SCHED_MIGRATION_COST_NS < SYSCTL_SCHED_LATENCY_NS);
    }

    #[test]
    fn test_latency_is_6ms() {
        // CFS targets 6 ms period.
        assert_eq!(SYSCTL_SCHED_LATENCY_NS, 6 * 1_000_000);
        // Min granularity is 0.75 ms.
        assert_eq!(SYSCTL_SCHED_MIN_GRANULARITY_NS, 750_000);
    }

    #[test]
    fn test_nice_range_is_40_wide() {
        assert_eq!(MAX_NICE, 19);
        assert_eq!(MIN_NICE, -20);
        assert_eq!((MAX_NICE - MIN_NICE + 1) as u32, NICE_WIDTH);
    }

    #[test]
    fn test_nice_0_weight_is_2_to_the_10() {
        // NICE_0_LOAD is 1024 = 2^10 (canonical CFS weight).
        assert_eq!(NICE_0_LOAD, 1_024);
        assert!(NICE_0_LOAD.is_power_of_two());
        assert_eq!(NICE_0_LOAD.trailing_zeros(), 10);
    }

    #[test]
    fn test_lb_interval_bounds() {
        // 1ms..16ms (4 doublings).
        assert_eq!(SYSCTL_SCHED_LB_INTERVAL_MIN_MS, 1);
        assert_eq!(SYSCTL_SCHED_LB_INTERVAL_MAX_MS, 16);
        assert!(SYSCTL_SCHED_LB_INTERVAL_MAX_MS.is_power_of_two());
    }

    #[test]
    fn test_bandwidth_period_bounds() {
        // 100 ms default, 1 s max.
        assert_eq!(CFS_BANDWIDTH_PERIOD_DEFAULT_US, 100_000);
        assert_eq!(CFS_BANDWIDTH_PERIOD_MAX_US, 1_000_000);
        // Max is 10x default.
        assert_eq!(
            CFS_BANDWIDTH_PERIOD_MAX_US / CFS_BANDWIDTH_PERIOD_DEFAULT_US,
            10
        );
        // Quota min must be smaller than default period.
        assert!(CFS_BANDWIDTH_QUOTA_MIN_US < CFS_BANDWIDTH_PERIOD_DEFAULT_US);
    }
}
