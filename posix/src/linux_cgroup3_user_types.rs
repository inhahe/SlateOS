//! `<linux/cgroup.h>` (part 3) — cgroup-v2 cpu controller files.
//!
//! The cpu controller in cgroup-v2 exposes weight, max bandwidth,
//! idle accounting, and pressure-stall information. This module
//! covers the file names, the well-known weight bounds, and the
//! `cpu.max` field layout.

// ---------------------------------------------------------------------------
// File names under a cgroup directory (cpu controller)
// ---------------------------------------------------------------------------

pub const CGROUP_CPU_WEIGHT: &str = "cpu.weight";
pub const CGROUP_CPU_WEIGHT_NICE: &str = "cpu.weight.nice";
pub const CGROUP_CPU_MAX: &str = "cpu.max";
pub const CGROUP_CPU_MAX_BURST: &str = "cpu.max.burst";
pub const CGROUP_CPU_IDLE: &str = "cpu.idle";
pub const CGROUP_CPU_PRESSURE: &str = "cpu.pressure";
pub const CGROUP_CPU_STAT: &str = "cpu.stat";
pub const CGROUP_CPU_UCLAMP_MIN: &str = "cpu.uclamp.min";
pub const CGROUP_CPU_UCLAMP_MAX: &str = "cpu.uclamp.max";

// ---------------------------------------------------------------------------
// cpu.weight bounds and default
// ---------------------------------------------------------------------------

/// Minimum cpu.weight value.
pub const CGROUP_CPU_WEIGHT_MIN: u32 = 1;

/// Default cpu.weight (matches nice 0).
pub const CGROUP_CPU_WEIGHT_DEFAULT: u32 = 100;

/// Maximum cpu.weight value.
pub const CGROUP_CPU_WEIGHT_MAX: u32 = 10_000;

// ---------------------------------------------------------------------------
// cpu.weight.nice bounds (-20..19, same as nice(2))
// ---------------------------------------------------------------------------

pub const CGROUP_CPU_WEIGHT_NICE_MIN: i32 = -20;
pub const CGROUP_CPU_WEIGHT_NICE_MAX: i32 = 19;

// ---------------------------------------------------------------------------
// cpu.max — quota/period defaults (microseconds)
// ---------------------------------------------------------------------------

/// Default cpu.max period (100 ms).
pub const CGROUP_CPU_MAX_PERIOD_DEFAULT_US: u64 = 100_000;

/// Default cpu.max quota — "max" (no bandwidth cap).
pub const CGROUP_CPU_MAX_QUOTA_UNLIMITED: u64 = u64::MAX;

/// Minimum quota the kernel will accept (1 ms).
pub const CGROUP_CPU_MAX_QUOTA_MIN_US: u64 = 1_000;

// ---------------------------------------------------------------------------
// uclamp bounds (0..1024)
// ---------------------------------------------------------------------------

pub const CGROUP_CPU_UCLAMP_VAL_MIN: u32 = 0;
pub const CGROUP_CPU_UCLAMP_VAL_MAX: u32 = 1_024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_files_have_cpu_prefix() {
        for f in [
            CGROUP_CPU_WEIGHT,
            CGROUP_CPU_WEIGHT_NICE,
            CGROUP_CPU_MAX,
            CGROUP_CPU_MAX_BURST,
            CGROUP_CPU_IDLE,
            CGROUP_CPU_PRESSURE,
            CGROUP_CPU_STAT,
            CGROUP_CPU_UCLAMP_MIN,
            CGROUP_CPU_UCLAMP_MAX,
        ] {
            assert!(f.starts_with("cpu."));
        }
    }

    #[test]
    fn test_weight_bounds_and_default() {
        assert_eq!(CGROUP_CPU_WEIGHT_MIN, 1);
        assert_eq!(CGROUP_CPU_WEIGHT_DEFAULT, 100);
        assert_eq!(CGROUP_CPU_WEIGHT_MAX, 10_000);
        // Default sits at the geometric midpoint.
        assert!(CGROUP_CPU_WEIGHT_DEFAULT > CGROUP_CPU_WEIGHT_MIN);
        assert!(CGROUP_CPU_WEIGHT_DEFAULT < CGROUP_CPU_WEIGHT_MAX);
        // Max is 100x the default — generous headroom.
        assert_eq!(CGROUP_CPU_WEIGHT_MAX / CGROUP_CPU_WEIGHT_DEFAULT, 100);
    }

    #[test]
    fn test_nice_bounds_match_nice_2() {
        // cpu.weight.nice mirrors the nice(2) range.
        assert_eq!(CGROUP_CPU_WEIGHT_NICE_MIN, -20);
        assert_eq!(CGROUP_CPU_WEIGHT_NICE_MAX, 19);
        assert_eq!(
            (CGROUP_CPU_WEIGHT_NICE_MAX - CGROUP_CPU_WEIGHT_NICE_MIN + 1) as u32,
            40
        );
    }

    #[test]
    fn test_max_period_and_quota_defaults() {
        // 100 ms period.
        assert_eq!(CGROUP_CPU_MAX_PERIOD_DEFAULT_US, 100_000);
        // "max" quota is sentinel u64::MAX.
        assert_eq!(CGROUP_CPU_MAX_QUOTA_UNLIMITED, u64::MAX);
        // 1 ms minimum quota.
        assert_eq!(CGROUP_CPU_MAX_QUOTA_MIN_US, 1_000);
        assert!(CGROUP_CPU_MAX_QUOTA_MIN_US < CGROUP_CPU_MAX_PERIOD_DEFAULT_US);
    }

    #[test]
    fn test_uclamp_range_is_0_to_1024() {
        assert_eq!(CGROUP_CPU_UCLAMP_VAL_MIN, 0);
        assert_eq!(CGROUP_CPU_UCLAMP_VAL_MAX, 1_024);
        assert!(CGROUP_CPU_UCLAMP_VAL_MAX.is_power_of_two());
    }

    #[test]
    fn test_uclamp_files_paired() {
        // UCLAMP_MIN and UCLAMP_MAX form the lower/upper utilization bound.
        assert!(CGROUP_CPU_UCLAMP_MIN.ends_with(".min"));
        assert!(CGROUP_CPU_UCLAMP_MAX.ends_with(".max"));
    }
}
