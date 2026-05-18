//! `<linux/cpufreq.h>` — Additional CPU frequency scaling constants.
//!
//! Supplementary cpufreq constants covering governor types,
//! transition events, policy flags, and relation types.

// ---------------------------------------------------------------------------
// CPU frequency governors
// ---------------------------------------------------------------------------

/// Performance governor.
pub const CPUFREQ_GOV_PERFORMANCE: u32 = 0;
/// Powersave governor.
pub const CPUFREQ_GOV_POWERSAVE: u32 = 1;
/// Userspace governor.
pub const CPUFREQ_GOV_USERSPACE: u32 = 2;
/// On-demand governor.
pub const CPUFREQ_GOV_ONDEMAND: u32 = 3;
/// Conservative governor.
pub const CPUFREQ_GOV_CONSERVATIVE: u32 = 4;
/// Schedutil governor.
pub const CPUFREQ_GOV_SCHEDUTIL: u32 = 5;

// ---------------------------------------------------------------------------
// CPU frequency relation types
// ---------------------------------------------------------------------------

/// Lowest frequency at or above target.
pub const CPUFREQ_RELATION_L: u32 = 0;
/// Highest frequency at or below target.
pub const CPUFREQ_RELATION_H: u32 = 1;
/// Closest to target.
pub const CPUFREQ_RELATION_C: u32 = 2;
/// Efficient frequency.
pub const CPUFREQ_RELATION_E: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// CPU frequency transition notifications
// ---------------------------------------------------------------------------

/// Pre-change notification.
pub const CPUFREQ_PRECHANGE: u32 = 0;
/// Post-change notification.
pub const CPUFREQ_POSTCHANGE: u32 = 1;

// ---------------------------------------------------------------------------
// CPU frequency policy limits
// ---------------------------------------------------------------------------

/// Minimum frequency index.
pub const CPUFREQ_ENTRY_INVALID: u32 = u32::MAX;
/// Eternal transition latency.
pub const CPUFREQ_ETERNAL: u32 = u32::MAX;
/// Table end marker.
pub const CPUFREQ_TABLE_END: u32 = u32::MAX;
/// Boost disabled.
pub const CPUFREQ_BOOST_DISABLED: u32 = 0;
/// Boost enabled.
pub const CPUFREQ_BOOST_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// CPU frequency flags
// ---------------------------------------------------------------------------

/// No frequency limit.
pub const CPUFREQ_NO_LIMIT: u32 = 0;
/// Const loops.
pub const CPUFREQ_CONST_LOOPS: u32 = 1 << 0;
/// Sticky governor.
pub const CPUFREQ_STICKY: u32 = 1 << 1;
/// Need initial frequency setup.
pub const CPUFREQ_NEED_INITIAL_FREQ_CHECK: u32 = 1 << 2;
/// Need update limits.
pub const CPUFREQ_NEED_UPDATE_LIMITS: u32 = 1 << 3;
/// Is cooling device.
pub const CPUFREQ_IS_COOLING_DEV: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Energy Performance Preference (EPP)
// ---------------------------------------------------------------------------

/// Default EPP.
pub const ENERGY_PERF_PREF_DEFAULT: u32 = 0;
/// Performance EPP.
pub const ENERGY_PERF_PREF_PERFORMANCE: u32 = 1;
/// Balance performance EPP.
pub const ENERGY_PERF_PREF_BALANCE_PERFORMANCE: u32 = 2;
/// Balance power EPP.
pub const ENERGY_PERF_PREF_BALANCE_POWER: u32 = 3;
/// Power save EPP.
pub const ENERGY_PERF_PREF_POWER: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governors_distinct() {
        let govs = [
            CPUFREQ_GOV_PERFORMANCE, CPUFREQ_GOV_POWERSAVE,
            CPUFREQ_GOV_USERSPACE, CPUFREQ_GOV_ONDEMAND,
            CPUFREQ_GOV_CONSERVATIVE, CPUFREQ_GOV_SCHEDUTIL,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
        }
    }

    #[test]
    fn test_relations_distinct() {
        let rels = [
            CPUFREQ_RELATION_L, CPUFREQ_RELATION_H,
            CPUFREQ_RELATION_C, CPUFREQ_RELATION_E,
        ];
        for i in 0..rels.len() {
            for j in (i + 1)..rels.len() {
                assert_ne!(rels[i], rels[j]);
            }
        }
    }

    #[test]
    fn test_transitions() {
        assert_eq!(CPUFREQ_PRECHANGE, 0);
        assert_eq!(CPUFREQ_POSTCHANGE, 1);
    }

    #[test]
    fn test_boost() {
        assert_eq!(CPUFREQ_BOOST_DISABLED, 0);
        assert_eq!(CPUFREQ_BOOST_ENABLED, 1);
    }

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            CPUFREQ_CONST_LOOPS, CPUFREQ_STICKY,
            CPUFREQ_NEED_INITIAL_FREQ_CHECK,
            CPUFREQ_NEED_UPDATE_LIMITS,
            CPUFREQ_IS_COOLING_DEV,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_epp_ordering() {
        assert!(ENERGY_PERF_PREF_DEFAULT < ENERGY_PERF_PREF_PERFORMANCE);
        assert!(ENERGY_PERF_PREF_PERFORMANCE < ENERGY_PERF_PREF_BALANCE_PERFORMANCE);
        assert!(ENERGY_PERF_PREF_BALANCE_PERFORMANCE < ENERGY_PERF_PREF_BALANCE_POWER);
        assert!(ENERGY_PERF_PREF_BALANCE_POWER < ENERGY_PERF_PREF_POWER);
    }
}
