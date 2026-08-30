//! `<linux/cpufreq.h>` — extended cpufreq subsystem constants.
//!
//! Companion to the base cpufreq module. Covers EPP (energy/performance
//! preference), boost-control sysfs files, transition notifier reason
//! codes, and the global stats interface.

// ---------------------------------------------------------------------------
// Transition notifier reason codes
// ---------------------------------------------------------------------------

pub const CPUFREQ_PRECHANGE: u32 = 0;
pub const CPUFREQ_POSTCHANGE: u32 = 1;
pub const CPUFREQ_CREATE_POLICY: u32 = 2;
pub const CPUFREQ_REMOVE_POLICY: u32 = 3;

// ---------------------------------------------------------------------------
// Boost-control sysfs (intel_pstate / acpi_cpufreq)
// ---------------------------------------------------------------------------

pub const CPUFREQ_BOOST_PATH: &str = "/sys/devices/system/cpu/cpufreq/boost";
pub const CPUFREQ_BOOST_ENABLED: u32 = 1;
pub const CPUFREQ_BOOST_DISABLED: u32 = 0;

// ---------------------------------------------------------------------------
// intel_pstate-specific files
// ---------------------------------------------------------------------------

pub const INTEL_PSTATE_NO_TURBO: &str = "/sys/devices/system/cpu/intel_pstate/no_turbo";
pub const INTEL_PSTATE_HWP_DYNAMIC_BOOST: &str =
    "/sys/devices/system/cpu/intel_pstate/hwp_dynamic_boost";
pub const INTEL_PSTATE_STATUS: &str = "/sys/devices/system/cpu/intel_pstate/status";
pub const INTEL_PSTATE_MAX_PERF_PCT: &str = "/sys/devices/system/cpu/intel_pstate/max_perf_pct";
pub const INTEL_PSTATE_MIN_PERF_PCT: &str = "/sys/devices/system/cpu/intel_pstate/min_perf_pct";

/// intel_pstate "status" values.
pub const INTEL_PSTATE_STATUS_ACTIVE: &str = "active";
pub const INTEL_PSTATE_STATUS_PASSIVE: &str = "passive";
pub const INTEL_PSTATE_STATUS_OFF: &str = "off";

// ---------------------------------------------------------------------------
// EPP (Energy/Performance Preference) string values
// ---------------------------------------------------------------------------

pub const EPP_PERFORMANCE: &str = "performance";
pub const EPP_BALANCE_PERFORMANCE: &str = "balance_performance";
pub const EPP_BALANCE_POWER: &str = "balance_power";
pub const EPP_POWER: &str = "power";
pub const EPP_DEFAULT: &str = "default";

// ---------------------------------------------------------------------------
// EPP numeric encoding (HWP request register field 0..255)
// ---------------------------------------------------------------------------

pub const EPP_PERFORMANCE_VAL: u8 = 0;
pub const EPP_BALANCE_PERFORMANCE_VAL: u8 = 128;
pub const EPP_BALANCE_POWER_VAL: u8 = 192;
pub const EPP_POWER_VAL: u8 = 255;

// ---------------------------------------------------------------------------
// Per-policy "energy_performance_available_preferences" path suffix
// ---------------------------------------------------------------------------

pub const CPUFREQ_ATTR_EPP_AVAIL: &str = "energy_performance_available_preferences";
pub const CPUFREQ_ATTR_EPP_CURRENT: &str = "energy_performance_preference";

// ---------------------------------------------------------------------------
// Stats sysfs directory
// ---------------------------------------------------------------------------

pub const CPUFREQ_STATS_DIR: &str = "stats";
pub const CPUFREQ_STATS_TIME_IN_STATE: &str = "stats/time_in_state";
pub const CPUFREQ_STATS_TOTAL_TRANS: &str = "stats/total_trans";
pub const CPUFREQ_STATS_TRANS_TABLE: &str = "stats/trans_table";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notifier_codes_dense_0_to_3() {
        let r = [
            CPUFREQ_PRECHANGE,
            CPUFREQ_POSTCHANGE,
            CPUFREQ_CREATE_POLICY,
            CPUFREQ_REMOVE_POLICY,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_boost_binary() {
        assert_eq!(CPUFREQ_BOOST_DISABLED, 0);
        assert_eq!(CPUFREQ_BOOST_ENABLED, 1);
        assert!(CPUFREQ_BOOST_PATH.starts_with("/sys/devices/system/cpu/"));
    }

    #[test]
    fn test_intel_pstate_paths_under_intel_pstate_dir() {
        for p in [
            INTEL_PSTATE_NO_TURBO,
            INTEL_PSTATE_HWP_DYNAMIC_BOOST,
            INTEL_PSTATE_STATUS,
            INTEL_PSTATE_MAX_PERF_PCT,
            INTEL_PSTATE_MIN_PERF_PCT,
        ] {
            assert!(p.starts_with("/sys/devices/system/cpu/intel_pstate/"));
        }
    }

    #[test]
    fn test_intel_pstate_status_values_distinct() {
        assert_ne!(INTEL_PSTATE_STATUS_ACTIVE, INTEL_PSTATE_STATUS_PASSIVE);
        assert_ne!(INTEL_PSTATE_STATUS_PASSIVE, INTEL_PSTATE_STATUS_OFF);
        assert_ne!(INTEL_PSTATE_STATUS_OFF, INTEL_PSTATE_STATUS_ACTIVE);
    }

    #[test]
    fn test_epp_string_values_distinct() {
        let s = [
            EPP_PERFORMANCE,
            EPP_BALANCE_PERFORMANCE,
            EPP_BALANCE_POWER,
            EPP_POWER,
            EPP_DEFAULT,
        ];
        for (i, &x) in s.iter().enumerate() {
            for &y in &s[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_epp_numeric_monotonic_perf_to_power() {
        // 0 = max perf, 255 = max power saving — monotone increasing.
        assert!(EPP_PERFORMANCE_VAL < EPP_BALANCE_PERFORMANCE_VAL);
        assert!(EPP_BALANCE_PERFORMANCE_VAL < EPP_BALANCE_POWER_VAL);
        assert!(EPP_BALANCE_POWER_VAL < EPP_POWER_VAL);
        assert_eq!(EPP_PERFORMANCE_VAL, 0);
        assert_eq!(EPP_POWER_VAL, 255);
    }

    #[test]
    fn test_epp_attr_files_distinct() {
        assert_ne!(CPUFREQ_ATTR_EPP_AVAIL, CPUFREQ_ATTR_EPP_CURRENT);
        // Both attribute files live in the energy_performance_* namespace.
        assert!(CPUFREQ_ATTR_EPP_AVAIL.starts_with("energy_performance_"));
        assert!(CPUFREQ_ATTR_EPP_CURRENT.starts_with("energy_performance_"));
    }

    #[test]
    fn test_stats_files_under_stats_dir() {
        for p in [
            CPUFREQ_STATS_TIME_IN_STATE,
            CPUFREQ_STATS_TOTAL_TRANS,
            CPUFREQ_STATS_TRANS_TABLE,
        ] {
            assert!(p.starts_with(CPUFREQ_STATS_DIR));
            assert!(p.contains('/'));
        }
    }
}
