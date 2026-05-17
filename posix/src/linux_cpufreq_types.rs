//! `<linux/cpufreq.h>` — CPU frequency scaling (cpufreq) constants.
//!
//! The cpufreq subsystem manages CPU frequency and voltage scaling
//! (DVFS). Governors decide when to change frequency based on load
//! patterns. Drivers communicate with hardware (ACPI P-states,
//! Intel SpeedStep/HWP, AMD Pstate) to effect frequency changes.
//! The framework supports per-CPU policies with min/max limits,
//! boost (turbo) control, and energy-performance preferences.

// ---------------------------------------------------------------------------
// cpufreq governor IDs
// ---------------------------------------------------------------------------

/// Performance governor (always max frequency).
pub const CPUFREQ_GOV_PERFORMANCE: u32 = 0;
/// Powersave governor (always min frequency).
pub const CPUFREQ_GOV_POWERSAVE: u32 = 1;
/// Ondemand governor (reactive, ramp up on load).
pub const CPUFREQ_GOV_ONDEMAND: u32 = 2;
/// Conservative governor (gradual frequency steps).
pub const CPUFREQ_GOV_CONSERVATIVE: u32 = 3;
/// Schedutil governor (scheduler-driven, uses PELT).
pub const CPUFREQ_GOV_SCHEDUTIL: u32 = 4;
/// Userspace governor (userspace controls frequency).
pub const CPUFREQ_GOV_USERSPACE: u32 = 5;

// ---------------------------------------------------------------------------
// cpufreq policy/relation
// ---------------------------------------------------------------------------

/// Target the lowest frequency >= target.
pub const CPUFREQ_RELATION_L: u32 = 0;
/// Target the highest frequency <= target.
pub const CPUFREQ_RELATION_H: u32 = 1;
/// Target closest frequency to target.
pub const CPUFREQ_RELATION_C: u32 = 2;

// ---------------------------------------------------------------------------
// cpufreq transition notifications
// ---------------------------------------------------------------------------

/// Pre-change notification (about to change frequency).
pub const CPUFREQ_PRECHANGE: u32 = 0;
/// Post-change notification (frequency changed).
pub const CPUFREQ_POSTCHANGE: u32 = 1;

// ---------------------------------------------------------------------------
// cpufreq boost/turbo
// ---------------------------------------------------------------------------

/// Boost disabled.
pub const CPUFREQ_BOOST_DISABLED: u32 = 0;
/// Boost enabled (allow turbo frequencies).
pub const CPUFREQ_BOOST_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Energy Performance Preference (EPP) hints
// ---------------------------------------------------------------------------

/// Default EPP (balanced).
pub const CPUFREQ_EPP_DEFAULT: u32 = 0;
/// Performance EPP (favor speed).
pub const CPUFREQ_EPP_PERFORMANCE: u32 = 0;
/// Balance-performance EPP.
pub const CPUFREQ_EPP_BALANCE_PERFORMANCE: u32 = 128;
/// Balance-power EPP.
pub const CPUFREQ_EPP_BALANCE_POWER: u32 = 192;
/// Power EPP (favor energy saving).
pub const CPUFREQ_EPP_POWER: u32 = 255;

// ---------------------------------------------------------------------------
// cpufreq driver flags
// ---------------------------------------------------------------------------

/// Driver needs frequency table.
pub const CPUFREQ_NEED_INITIAL_FREQ_CHECK: u32 = 1 << 0;
/// Driver supports online/offline.
pub const CPUFREQ_IS_COOLING_DEV: u32 = 1 << 1;
/// Driver handles intermediate freqs (not actual hw states).
pub const CPUFREQ_HAVE_GOVERNOR_PER_POLICY: u32 = 1 << 2;
/// Hardware-managed P-states (HWP).
pub const CPUFREQ_HW_PSTATE: u32 = 1 << 3;

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
            CPUFREQ_GOV_ONDEMAND, CPUFREQ_GOV_CONSERVATIVE,
            CPUFREQ_GOV_SCHEDUTIL, CPUFREQ_GOV_USERSPACE,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
        }
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
    fn test_transitions_distinct() {
        assert_ne!(CPUFREQ_PRECHANGE, CPUFREQ_POSTCHANGE);
    }

    #[test]
    fn test_boost_distinct() {
        assert_ne!(CPUFREQ_BOOST_DISABLED, CPUFREQ_BOOST_ENABLED);
    }

    #[test]
    fn test_epp_ordered() {
        // EPP values increase from performance toward power saving
        assert!(CPUFREQ_EPP_PERFORMANCE <= CPUFREQ_EPP_BALANCE_PERFORMANCE);
        assert!(CPUFREQ_EPP_BALANCE_PERFORMANCE < CPUFREQ_EPP_BALANCE_POWER);
        assert!(CPUFREQ_EPP_BALANCE_POWER < CPUFREQ_EPP_POWER);
    }

    #[test]
    fn test_driver_flags_no_overlap() {
        let flags = [
            CPUFREQ_NEED_INITIAL_FREQ_CHECK,
            CPUFREQ_IS_COOLING_DEV,
            CPUFREQ_HAVE_GOVERNOR_PER_POLICY,
            CPUFREQ_HW_PSTATE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
