//! `<linux/cpufreq.h>` — CPU frequency governor and policy constants.
//!
//! CPU frequency scaling governors decide how and when to change
//! the CPU clock frequency. These constants define governor policy
//! types, frequency limits, and transition notification events.

// ---------------------------------------------------------------------------
// CPUFreq governor policies
// ---------------------------------------------------------------------------

/// Performance: always run at maximum frequency.
pub const CPUFREQ_POLICY_PERFORMANCE: u32 = 1;
/// Powersave: always run at minimum frequency.
pub const CPUFREQ_POLICY_POWERSAVE: u32 = 2;
/// Userspace: frequency set by userspace.
pub const CPUFREQ_POLICY_USERSPACE: u32 = 3;
/// Ondemand: scale based on CPU load.
pub const CPUFREQ_POLICY_ONDEMAND: u32 = 4;
/// Conservative: gradual frequency changes.
pub const CPUFREQ_POLICY_CONSERVATIVE: u32 = 5;
/// Schedutil: use scheduler utilization data.
pub const CPUFREQ_POLICY_SCHEDUTIL: u32 = 6;

// ---------------------------------------------------------------------------
// CPUFreq transition events
// ---------------------------------------------------------------------------

/// Pre-change notification.
pub const CPUFREQ_PRECHANGE: u32 = 0;
/// Post-change notification.
pub const CPUFREQ_POSTCHANGE: u32 = 1;

// ---------------------------------------------------------------------------
// CPUFreq relation types (for target_freq selection)
// ---------------------------------------------------------------------------

/// Select lowest frequency >= target.
pub const CPUFREQ_RELATION_L: u32 = 0;
/// Select highest frequency <= target.
pub const CPUFREQ_RELATION_H: u32 = 1;
/// Select closest frequency to target.
pub const CPUFREQ_RELATION_C: u32 = 2;

// ---------------------------------------------------------------------------
// CPUFreq boost/turbo
// ---------------------------------------------------------------------------

/// Boost disabled.
pub const CPUFREQ_BOOST_DISABLED: u32 = 0;
/// Boost enabled (turbo frequencies available).
pub const CPUFREQ_BOOST_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_distinct() {
        let policies = [
            CPUFREQ_POLICY_PERFORMANCE, CPUFREQ_POLICY_POWERSAVE,
            CPUFREQ_POLICY_USERSPACE, CPUFREQ_POLICY_ONDEMAND,
            CPUFREQ_POLICY_CONSERVATIVE, CPUFREQ_POLICY_SCHEDUTIL,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_transitions() {
        assert_eq!(CPUFREQ_PRECHANGE, 0);
        assert_eq!(CPUFREQ_POSTCHANGE, 1);
    }

    #[test]
    fn test_relations_distinct() {
        let rels = [CPUFREQ_RELATION_L, CPUFREQ_RELATION_H, CPUFREQ_RELATION_C];
        for i in 0..rels.len() {
            for j in (i + 1)..rels.len() {
                assert_ne!(rels[i], rels[j]);
            }
        }
    }

    #[test]
    fn test_boost_states() {
        assert_eq!(CPUFREQ_BOOST_DISABLED, 0);
        assert_eq!(CPUFREQ_BOOST_ENABLED, 1);
    }
}
