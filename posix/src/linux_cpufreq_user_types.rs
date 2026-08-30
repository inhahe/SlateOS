//! `<linux/cpufreq.h>` — cpufreq userspace-interface constants.
//!
//! Constants describing the per-CPU sysfs files under
//! `/sys/devices/system/cpu/cpufreq/policyN/`. `cpupower`, tuned and
//! intel_pstate userspace tools consume these governor and policy
//! names.

// ---------------------------------------------------------------------------
// Governor names (writable to scaling_governor)
// ---------------------------------------------------------------------------

/// Performance — always lock to max frequency.
pub const CPUFREQ_GOV_PERFORMANCE: &str = "performance";
/// Powersave — always lock to min frequency.
pub const CPUFREQ_GOV_POWERSAVE: &str = "powersave";
/// Userspace — frequency set explicitly by writes to scaling_setspeed.
pub const CPUFREQ_GOV_USERSPACE: &str = "userspace";
/// Ondemand — sampled scaling.
pub const CPUFREQ_GOV_ONDEMAND: &str = "ondemand";
/// Conservative — gradual scaling.
pub const CPUFREQ_GOV_CONSERVATIVE: &str = "conservative";
/// Schedutil — uses the kernel scheduler util signal.
pub const CPUFREQ_GOV_SCHEDUTIL: &str = "schedutil";

// ---------------------------------------------------------------------------
// Energy-performance-preference (writable to energy_performance_preference)
// ---------------------------------------------------------------------------

/// Bias maximum performance.
pub const CPUFREQ_EPP_PERFORMANCE: &str = "performance";
/// Balance, biased toward performance.
pub const CPUFREQ_EPP_BALANCE_PERFORMANCE: &str = "balance_performance";
/// Balance, biased toward power.
pub const CPUFREQ_EPP_BALANCE_POWER: &str = "balance_power";
/// Bias minimum power.
pub const CPUFREQ_EPP_POWER: &str = "power";
/// Use the platform default.
pub const CPUFREQ_EPP_DEFAULT: &str = "default";

// ---------------------------------------------------------------------------
// Policy notification flag bits (cpufreq_policy.flags)
// ---------------------------------------------------------------------------

/// Driver wants only one CPU per policy.
pub const CPUFREQ_CONST_LOOPS: u32 = 1 << 0;
/// Policy active during system suspend.
pub const CPUFREQ_PM_NO_WARN: u32 = 1 << 1;
/// Per-policy boost (turbo) enabled.
pub const CPUFREQ_BOOST_ENABLED: u32 = 1 << 2;
/// Driver needs initial frequency probe.
pub const CPUFREQ_HAVE_GOVERNOR_PER_POLICY: u32 = 1 << 3;
/// Driver supports asymmetric per-CPU rates.
pub const CPUFREQ_ASYNC_NOTIFICATION: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governor_names_distinct() {
        let govs = [
            CPUFREQ_GOV_PERFORMANCE,
            CPUFREQ_GOV_POWERSAVE,
            CPUFREQ_GOV_USERSPACE,
            CPUFREQ_GOV_ONDEMAND,
            CPUFREQ_GOV_CONSERVATIVE,
            CPUFREQ_GOV_SCHEDUTIL,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
            // All governor names must be lowercase ASCII (sysfs
            // expects that exact spelling).
            assert!(govs[i].chars().all(|c| c.is_ascii_lowercase()));
        }
    }

    #[test]
    fn test_epp_names_distinct() {
        let epp = [
            CPUFREQ_EPP_PERFORMANCE,
            CPUFREQ_EPP_BALANCE_PERFORMANCE,
            CPUFREQ_EPP_BALANCE_POWER,
            CPUFREQ_EPP_POWER,
            CPUFREQ_EPP_DEFAULT,
        ];
        for i in 0..epp.len() {
            for j in (i + 1)..epp.len() {
                assert_ne!(epp[i], epp[j]);
            }
        }
    }

    #[test]
    fn test_flag_bits_distinct_powers_of_two() {
        let flags = [
            CPUFREQ_CONST_LOOPS,
            CPUFREQ_PM_NO_WARN,
            CPUFREQ_BOOST_ENABLED,
            CPUFREQ_HAVE_GOVERNOR_PER_POLICY,
            CPUFREQ_ASYNC_NOTIFICATION,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
