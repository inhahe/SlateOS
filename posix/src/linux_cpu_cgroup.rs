//! CPU cgroup controller constants.
//!
//! The CPU cgroup controller limits CPU time per cgroup. Cgroup v2
//! uses a bandwidth control model (quota/period) and weight-based
//! proportional sharing. SCHED_DEADLINE-based bandwidth control
//! is also available.

// ---------------------------------------------------------------------------
// CPU controller file names (cgroup v2)
// ---------------------------------------------------------------------------

/// CPU weight.
pub const CPU_WEIGHT: &str = "cpu.weight";
/// CPU weight (nice-style, 1-10000).
pub const CPU_WEIGHT_NICE: &str = "cpu.weight.nice";
/// CPU bandwidth max (quota period).
pub const CPU_MAX: &str = "cpu.max";
/// CPU burst.
pub const CPU_MAX_BURST: &str = "cpu.max.burst";
/// CPU statistics.
pub const CPU_STAT: &str = "cpu.stat";
/// CPU pressure.
pub const CPU_PRESSURE: &str = "cpu.pressure";
/// CPU idle flag.
pub const CPU_IDLE: &str = "cpu.idle";

// ---------------------------------------------------------------------------
// Weight range
// ---------------------------------------------------------------------------

/// Minimum CPU weight.
pub const CPU_WEIGHT_MIN: u32 = 1;
/// Default CPU weight.
pub const CPU_WEIGHT_DEFAULT: u32 = 100;
/// Maximum CPU weight.
pub const CPU_WEIGHT_MAX: u32 = 10000;

// ---------------------------------------------------------------------------
// Bandwidth defaults
// ---------------------------------------------------------------------------

/// Default bandwidth period (microseconds).
pub const CPU_PERIOD_DEFAULT_US: u64 = 100_000;
/// Minimum bandwidth period (microseconds).
pub const CPU_PERIOD_MIN_US: u64 = 1_000;
/// Maximum bandwidth period (microseconds).
pub const CPU_PERIOD_MAX_US: u64 = 1_000_000;
/// Bandwidth quota: unlimited.
pub const CPU_QUOTA_MAX: &str = "max";

// ---------------------------------------------------------------------------
// CPU stat fields
// ---------------------------------------------------------------------------

/// Total CPU usage (microseconds).
pub const CPU_STAT_USAGE_USEC: &str = "usage_usec";
/// User CPU time (microseconds).
pub const CPU_STAT_USER_USEC: &str = "user_usec";
/// System CPU time (microseconds).
pub const CPU_STAT_SYSTEM_USEC: &str = "system_usec";
/// Number of scheduling periods.
pub const CPU_STAT_NR_PERIODS: &str = "nr_periods";
/// Number of throttled periods.
pub const CPU_STAT_NR_THROTTLED: &str = "nr_throttled";
/// Total throttled time (microseconds).
pub const CPU_STAT_THROTTLED_USEC: &str = "throttled_usec";
/// Number of bursts.
pub const CPU_STAT_NR_BURSTS: &str = "nr_bursts";
/// Total burst time (microseconds).
pub const CPU_STAT_BURST_USEC: &str = "burst_usec";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_names_distinct() {
        let files = [
            CPU_WEIGHT, CPU_WEIGHT_NICE, CPU_MAX, CPU_MAX_BURST,
            CPU_STAT, CPU_PRESSURE, CPU_IDLE,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_weight_range() {
        assert!(CPU_WEIGHT_MIN < CPU_WEIGHT_DEFAULT);
        assert!(CPU_WEIGHT_DEFAULT < CPU_WEIGHT_MAX);
    }

    #[test]
    fn test_period_range() {
        assert!(CPU_PERIOD_MIN_US < CPU_PERIOD_DEFAULT_US);
        assert!(CPU_PERIOD_DEFAULT_US < CPU_PERIOD_MAX_US);
    }

    #[test]
    fn test_stat_fields_distinct() {
        let fields = [
            CPU_STAT_USAGE_USEC, CPU_STAT_USER_USEC,
            CPU_STAT_SYSTEM_USEC, CPU_STAT_NR_PERIODS,
            CPU_STAT_NR_THROTTLED, CPU_STAT_THROTTLED_USEC,
            CPU_STAT_NR_BURSTS, CPU_STAT_BURST_USEC,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }
}
