//! `<linux/cpufreq.h>` — cpufreq governor constants.
//!
//! Each governor implements a policy for picking the operating
//! frequency given the current load. Userspace selects one by
//! writing its name to scaling_governor.

// ---------------------------------------------------------------------------
// Built-in governor names
// ---------------------------------------------------------------------------

pub const CPUFREQ_GOV_PERFORMANCE: &str = "performance";
pub const CPUFREQ_GOV_POWERSAVE: &str = "powersave";
pub const CPUFREQ_GOV_USERSPACE: &str = "userspace";
pub const CPUFREQ_GOV_ONDEMAND: &str = "ondemand";
pub const CPUFREQ_GOV_CONSERVATIVE: &str = "conservative";
pub const CPUFREQ_GOV_SCHEDUTIL: &str = "schedutil";

// ---------------------------------------------------------------------------
// Governor events (struct cpufreq_governor::governor)
// ---------------------------------------------------------------------------

pub const CPUFREQ_GOV_START: u32 = 1;
pub const CPUFREQ_GOV_STOP: u32 = 2;
pub const CPUFREQ_GOV_LIMITS: u32 = 3;
pub const CPUFREQ_GOV_POLICY_INIT: u32 = 4;
pub const CPUFREQ_GOV_POLICY_EXIT: u32 = 5;

// ---------------------------------------------------------------------------
// Per-policy sysfs files
// ---------------------------------------------------------------------------

pub const CPUFREQ_ATTR_SCALING_GOVERNOR: &str = "scaling_governor";
pub const CPUFREQ_ATTR_AVAILABLE_GOVERNORS: &str = "scaling_available_governors";
pub const CPUFREQ_ATTR_SCALING_MIN_FREQ: &str = "scaling_min_freq";
pub const CPUFREQ_ATTR_SCALING_MAX_FREQ: &str = "scaling_max_freq";
pub const CPUFREQ_ATTR_SCALING_CUR_FREQ: &str = "scaling_cur_freq";
pub const CPUFREQ_ATTR_CPUINFO_MIN_FREQ: &str = "cpuinfo_min_freq";
pub const CPUFREQ_ATTR_CPUINFO_MAX_FREQ: &str = "cpuinfo_max_freq";
pub const CPUFREQ_ATTR_CPUINFO_CUR_FREQ: &str = "cpuinfo_cur_freq";

// ---------------------------------------------------------------------------
// ondemand / conservative governor tunables
// ---------------------------------------------------------------------------

pub const CPUFREQ_GOV_ONDEMAND_UP_THRESHOLD: &str = "up_threshold";
pub const CPUFREQ_GOV_ONDEMAND_SAMPLING_RATE: &str = "sampling_rate";
pub const CPUFREQ_GOV_ONDEMAND_DOWN_DIFFERENTIAL: &str = "down_differential";
pub const CPUFREQ_GOV_ONDEMAND_IGNORE_NICE_LOAD: &str = "ignore_nice_load";
pub const CPUFREQ_GOV_ONDEMAND_POWERSAVE_BIAS: &str = "powersave_bias";

/// Default up_threshold (load percentage above which to scale up).
pub const ONDEMAND_DEFAULT_UP_THRESHOLD: u32 = 80;
/// Reasonable lower bound for up_threshold.
pub const ONDEMAND_MIN_UP_THRESHOLD: u32 = 1;
pub const ONDEMAND_MAX_UP_THRESHOLD: u32 = 99;
/// Default sampling rate (microseconds).
pub const ONDEMAND_DEFAULT_SAMPLING_RATE_US: u32 = 10_000;

// ---------------------------------------------------------------------------
// schedutil tunable
// ---------------------------------------------------------------------------

pub const CPUFREQ_GOV_SCHEDUTIL_RATE_LIMIT: &str = "rate_limit_us";
pub const SCHEDUTIL_DEFAULT_RATE_LIMIT_US: u32 = 1_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governor_names_distinct_lowercase() {
        let g = [
            CPUFREQ_GOV_PERFORMANCE,
            CPUFREQ_GOV_POWERSAVE,
            CPUFREQ_GOV_USERSPACE,
            CPUFREQ_GOV_ONDEMAND,
            CPUFREQ_GOV_CONSERVATIVE,
            CPUFREQ_GOV_SCHEDUTIL,
        ];
        for (i, &x) in g.iter().enumerate() {
            for &y in &g[i + 1..] {
                assert_ne!(x, y);
            }
            for c in x.chars() {
                assert!(c.is_ascii_lowercase());
            }
        }
    }

    #[test]
    fn test_governor_events_dense_1_to_5() {
        let e = [
            CPUFREQ_GOV_START,
            CPUFREQ_GOV_STOP,
            CPUFREQ_GOV_LIMITS,
            CPUFREQ_GOV_POLICY_INIT,
            CPUFREQ_GOV_POLICY_EXIT,
        ];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_attr_files_distinct() {
        let a = [
            CPUFREQ_ATTR_SCALING_GOVERNOR,
            CPUFREQ_ATTR_AVAILABLE_GOVERNORS,
            CPUFREQ_ATTR_SCALING_MIN_FREQ,
            CPUFREQ_ATTR_SCALING_MAX_FREQ,
            CPUFREQ_ATTR_SCALING_CUR_FREQ,
            CPUFREQ_ATTR_CPUINFO_MIN_FREQ,
            CPUFREQ_ATTR_CPUINFO_MAX_FREQ,
            CPUFREQ_ATTR_CPUINFO_CUR_FREQ,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_ondemand_tunable_files_distinct() {
        let t = [
            CPUFREQ_GOV_ONDEMAND_UP_THRESHOLD,
            CPUFREQ_GOV_ONDEMAND_SAMPLING_RATE,
            CPUFREQ_GOV_ONDEMAND_DOWN_DIFFERENTIAL,
            CPUFREQ_GOV_ONDEMAND_IGNORE_NICE_LOAD,
            CPUFREQ_GOV_ONDEMAND_POWERSAVE_BIAS,
        ];
        for (i, &x) in t.iter().enumerate() {
            for &y in &t[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_ondemand_threshold_bounds() {
        assert_eq!(ONDEMAND_DEFAULT_UP_THRESHOLD, 80);
        assert!(ONDEMAND_MIN_UP_THRESHOLD < ONDEMAND_DEFAULT_UP_THRESHOLD);
        assert!(ONDEMAND_DEFAULT_UP_THRESHOLD < ONDEMAND_MAX_UP_THRESHOLD);
        assert_eq!(ONDEMAND_MAX_UP_THRESHOLD, 99);
        assert_eq!(ONDEMAND_MIN_UP_THRESHOLD, 1);
    }

    #[test]
    fn test_sampling_rate_defaults() {
        assert_eq!(ONDEMAND_DEFAULT_SAMPLING_RATE_US, 10_000);
        assert_eq!(SCHEDUTIL_DEFAULT_RATE_LIMIT_US, 1_000);
        // schedutil is more aggressive (shorter rate limit).
        assert!(SCHEDUTIL_DEFAULT_RATE_LIMIT_US < ONDEMAND_DEFAULT_SAMPLING_RATE_US);
    }
}
