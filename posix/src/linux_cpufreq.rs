//! `<linux/cpufreq.h>` — CPU frequency scaling constants.
//!
//! The cpufreq subsystem allows dynamic voltage and frequency
//! scaling (DVFS) of CPUs. Governors decide the target frequency
//! based on workload, and drivers communicate with hardware to
//! apply changes. Userspace can interact via sysfs.

// ---------------------------------------------------------------------------
// Transition events
// ---------------------------------------------------------------------------

/// Pre-change notification.
pub const CPUFREQ_PRECHANGE: u32 = 0;
/// Post-change notification.
pub const CPUFREQ_POSTCHANGE: u32 = 1;

// ---------------------------------------------------------------------------
// Policy / relation constants
// ---------------------------------------------------------------------------

/// Relation: target frequency or lower.
pub const CPUFREQ_RELATION_L: u32 = 0;
/// Relation: target frequency or higher.
pub const CPUFREQ_RELATION_H: u32 = 1;
/// Relation: closest to target.
pub const CPUFREQ_RELATION_C: u32 = 2;

// ---------------------------------------------------------------------------
// Governor names
// ---------------------------------------------------------------------------

/// Performance governor — always max frequency.
pub const CPUFREQ_GOV_PERFORMANCE: &str = "performance";
/// Powersave governor — always min frequency.
pub const CPUFREQ_GOV_POWERSAVE: &str = "powersave";
/// Userspace governor — frequency set by user.
pub const CPUFREQ_GOV_USERSPACE: &str = "userspace";
/// On-demand governor — scale based on load.
pub const CPUFREQ_GOV_ONDEMAND: &str = "ondemand";
/// Conservative governor — gradual scaling.
pub const CPUFREQ_GOV_CONSERVATIVE: &str = "conservative";
/// Schedutil governor — scheduler-driven scaling.
pub const CPUFREQ_GOV_SCHEDUTIL: &str = "schedutil";

// ---------------------------------------------------------------------------
// Boost states
// ---------------------------------------------------------------------------

/// Boost disabled.
pub const CPUFREQ_BOOST_DISABLED: u32 = 0;
/// Boost enabled.
pub const CPUFREQ_BOOST_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Frequency table sentinel / flags
// ---------------------------------------------------------------------------

/// End of frequency table.
pub const CPUFREQ_TABLE_END: u32 = u32::MAX;
/// Entry is invalid (skip).
pub const CPUFREQ_ENTRY_INVALID: u32 = u32::MAX;

/// Frequency is in kHz.
pub const CPUFREQ_ETERNAL: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Energy-performance preference hints (EPP)
// ---------------------------------------------------------------------------

/// Default / no preference.
pub const CPUFREQ_EPP_DEFAULT: u32 = 0;
/// Performance oriented.
pub const CPUFREQ_EPP_PERFORMANCE: u32 = 0;
/// Balance-performance.
pub const CPUFREQ_EPP_BALANCE_PERFORMANCE: u32 = 128;
/// Balance-power.
pub const CPUFREQ_EPP_BALANCE_POWERSAVE: u32 = 192;
/// Powersave oriented.
pub const CPUFREQ_EPP_POWERSAVE: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transition_events_distinct() {
        assert_ne!(CPUFREQ_PRECHANGE, CPUFREQ_POSTCHANGE);
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
        }
    }

    #[test]
    fn test_boost_states() {
        assert_eq!(CPUFREQ_BOOST_DISABLED, 0);
        assert_eq!(CPUFREQ_BOOST_ENABLED, 1);
    }

    #[test]
    fn test_table_end() {
        assert_eq!(CPUFREQ_TABLE_END, u32::MAX);
    }

    #[test]
    fn test_epp_ordering() {
        // EPP values should increase from performance to powersave.
        assert!(CPUFREQ_EPP_PERFORMANCE <= CPUFREQ_EPP_BALANCE_PERFORMANCE);
        assert!(CPUFREQ_EPP_BALANCE_PERFORMANCE < CPUFREQ_EPP_BALANCE_POWERSAVE);
        assert!(CPUFREQ_EPP_BALANCE_POWERSAVE < CPUFREQ_EPP_POWERSAVE);
    }

    #[test]
    fn test_epp_values() {
        assert_eq!(CPUFREQ_EPP_PERFORMANCE, 0);
        assert_eq!(CPUFREQ_EPP_BALANCE_PERFORMANCE, 128);
        assert_eq!(CPUFREQ_EPP_BALANCE_POWERSAVE, 192);
        assert_eq!(CPUFREQ_EPP_POWERSAVE, 255);
    }
}
