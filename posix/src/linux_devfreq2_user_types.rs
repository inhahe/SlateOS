//! `<linux/devfreq.h>` — devfreq (device DVFS) extended governors and stats.
//!
//! Devfreq manages frequency scaling for non-CPU devices: GPUs, memory
//! controllers, interconnects. Each device has a list of operating
//! points (OPPs) and a governor that picks one based on load. This
//! module covers the second-half of the userspace ABI: per-governor
//! tunables and statistics.

// ---------------------------------------------------------------------------
// Sysfs root and per-device path templates
// ---------------------------------------------------------------------------

pub const DEVFREQ_SYSFS_ROOT: &str = "/sys/class/devfreq";

// ---------------------------------------------------------------------------
// Per-device sysfs attributes
// ---------------------------------------------------------------------------

pub const DEVFREQ_ATTR_GOVERNOR: &str = "governor";
pub const DEVFREQ_ATTR_AVAILABLE_GOVERNORS: &str = "available_governors";
pub const DEVFREQ_ATTR_AVAILABLE_FREQUENCIES: &str = "available_frequencies";
pub const DEVFREQ_ATTR_CUR_FREQ: &str = "cur_freq";
pub const DEVFREQ_ATTR_TARGET_FREQ: &str = "target_freq";
pub const DEVFREQ_ATTR_MIN_FREQ: &str = "min_freq";
pub const DEVFREQ_ATTR_MAX_FREQ: &str = "max_freq";
pub const DEVFREQ_ATTR_POLLING_INTERVAL: &str = "polling_interval";
pub const DEVFREQ_ATTR_TRANS_STAT: &str = "trans_stat";
pub const DEVFREQ_ATTR_TIME_IN_STATE: &str = "time_in_state";
pub const DEVFREQ_ATTR_TOTAL_TRANS: &str = "total_trans";

// ---------------------------------------------------------------------------
// Governor names (non-overlapping with linux_devfreq.rs)
// ---------------------------------------------------------------------------

pub const DEVFREQ_GOV_SIMPLE_ONDEMAND: &str = "simple_ondemand";
pub const DEVFREQ_GOV_PERFORMANCE: &str = "performance";
pub const DEVFREQ_GOV_POWERSAVE: &str = "powersave";
pub const DEVFREQ_GOV_USERSPACE: &str = "userspace";
pub const DEVFREQ_GOV_PASSIVE: &str = "passive";

// ---------------------------------------------------------------------------
// simple_ondemand governor tunables (upthreshold / downdifferential)
// ---------------------------------------------------------------------------

pub const DEVFREQ_SO_DEFAULT_UPTHRESHOLD: u32 = 90;
pub const DEVFREQ_SO_DEFAULT_DOWNDIFFERENTIAL: u32 = 5;
pub const DEVFREQ_SO_MAX_UPTHRESHOLD: u32 = 100;
pub const DEVFREQ_SO_MIN_UPTHRESHOLD: u32 = 0;

// ---------------------------------------------------------------------------
// Polling interval bounds (ms)
// ---------------------------------------------------------------------------

pub const DEVFREQ_MIN_POLLING_MS: u32 = 1;
pub const DEVFREQ_DEFAULT_POLLING_MS: u32 = 100;
pub const DEVFREQ_MAX_POLLING_MS: u32 = 60_000;

// ---------------------------------------------------------------------------
// PM_QOS-style update flags
// ---------------------------------------------------------------------------

pub const DEVFREQ_PM_QOS_REQ_ADD: u32 = 0;
pub const DEVFREQ_PM_QOS_REQ_UPDATE: u32 = 1;
pub const DEVFREQ_PM_QOS_REQ_REMOVE: u32 = 2;

// ---------------------------------------------------------------------------
// Transition statistics
// ---------------------------------------------------------------------------

/// Width of one row in `trans_stat`: 1 freq column + N freq columns + last col.
/// Each row has at least 2 entries when there's one frequency.
pub const DEVFREQ_TRANS_MIN_COLS: usize = 2;
/// Maximum frequency points the kernel reports in `available_frequencies`.
pub const DEVFREQ_MAX_FREQ_POINTS: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_root_is_devfreq_class() {
        assert_eq!(DEVFREQ_SYSFS_ROOT, "/sys/class/devfreq");
    }

    #[test]
    fn test_freq_attrs_lowercase_and_distinct() {
        let a = [
            DEVFREQ_ATTR_GOVERNOR,
            DEVFREQ_ATTR_AVAILABLE_GOVERNORS,
            DEVFREQ_ATTR_AVAILABLE_FREQUENCIES,
            DEVFREQ_ATTR_CUR_FREQ,
            DEVFREQ_ATTR_TARGET_FREQ,
            DEVFREQ_ATTR_MIN_FREQ,
            DEVFREQ_ATTR_MAX_FREQ,
            DEVFREQ_ATTR_POLLING_INTERVAL,
            DEVFREQ_ATTR_TRANS_STAT,
            DEVFREQ_ATTR_TIME_IN_STATE,
            DEVFREQ_ATTR_TOTAL_TRANS,
        ];
        for (i, &x) in a.iter().enumerate() {
            for c in x.chars() {
                assert!(c.is_ascii_lowercase() || c == '_');
            }
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_governors_distinct() {
        let g = [
            DEVFREQ_GOV_SIMPLE_ONDEMAND,
            DEVFREQ_GOV_PERFORMANCE,
            DEVFREQ_GOV_POWERSAVE,
            DEVFREQ_GOV_USERSPACE,
            DEVFREQ_GOV_PASSIVE,
        ];
        for (i, &x) in g.iter().enumerate() {
            for &y in &g[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_simple_ondemand_defaults_in_range() {
        assert!(DEVFREQ_SO_DEFAULT_UPTHRESHOLD <= DEVFREQ_SO_MAX_UPTHRESHOLD);
        assert!(DEVFREQ_SO_DEFAULT_UPTHRESHOLD >= DEVFREQ_SO_MIN_UPTHRESHOLD);
        assert_eq!(DEVFREQ_SO_MAX_UPTHRESHOLD, 100);
        assert_eq!(DEVFREQ_SO_DEFAULT_UPTHRESHOLD, 90);
    }

    #[test]
    fn test_downdifferential_smaller_than_upthreshold() {
        // Differential must be less than threshold or the governor never
        // crosses below — sanity check.
        assert!(DEVFREQ_SO_DEFAULT_DOWNDIFFERENTIAL < DEVFREQ_SO_DEFAULT_UPTHRESHOLD);
    }

    #[test]
    fn test_polling_bounds_ordered() {
        assert!(DEVFREQ_MIN_POLLING_MS < DEVFREQ_DEFAULT_POLLING_MS);
        assert!(DEVFREQ_DEFAULT_POLLING_MS < DEVFREQ_MAX_POLLING_MS);
        assert_eq!(DEVFREQ_DEFAULT_POLLING_MS, 100);
    }

    #[test]
    fn test_pm_qos_req_codes_dense_0_to_2() {
        assert_eq!(DEVFREQ_PM_QOS_REQ_ADD, 0);
        assert_eq!(DEVFREQ_PM_QOS_REQ_UPDATE, 1);
        assert_eq!(DEVFREQ_PM_QOS_REQ_REMOVE, 2);
    }

    #[test]
    fn test_trans_stat_min_cols_and_max_points() {
        assert_eq!(DEVFREQ_TRANS_MIN_COLS, 2);
        assert_eq!(DEVFREQ_MAX_FREQ_POINTS, 32);
        assert!(DEVFREQ_MAX_FREQ_POINTS.is_power_of_two());
    }
}
