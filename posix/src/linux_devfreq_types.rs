//! `<linux/devfreq.h>` — Device frequency scaling (devfreq) constants.
//!
//! devfreq dynamically adjusts the operating frequency of devices
//! (GPU, memory controller, bus) based on load. It's similar to
//! cpufreq but for non-CPU devices. Governors determine the scaling
//! policy (performance, powersave, on-demand, etc.).

// ---------------------------------------------------------------------------
// devfreq governors (built-in)
// ---------------------------------------------------------------------------

/// Always run at maximum frequency.
pub const DEVFREQ_GOV_PERFORMANCE: &str = "performance";
/// Always run at minimum frequency.
pub const DEVFREQ_GOV_POWERSAVE: &str = "powersave";
/// Scale based on utilization (increase on busy, decrease on idle).
pub const DEVFREQ_GOV_SIMPLE_ONDEMAND: &str = "simple_ondemand";
/// Userspace-controlled frequency.
pub const DEVFREQ_GOV_USERSPACE: &str = "userspace";
/// Passive (follow another device's frequency).
pub const DEVFREQ_GOV_PASSIVE: &str = "passive";

// ---------------------------------------------------------------------------
// devfreq event types
// ---------------------------------------------------------------------------

/// Frequency change event.
pub const DEVFREQ_TRANSITION_NOTIFIER: u32 = 0;
/// Suspend event.
pub const DEVFREQ_PRECHANGE: u32 = 0;
/// Resume event.
pub const DEVFREQ_POSTCHANGE: u32 = 1;

// ---------------------------------------------------------------------------
// devfreq flags
// ---------------------------------------------------------------------------

/// Device supports multiple OPPs (Operating Performance Points).
pub const DEVFREQ_FLAG_LEAST_UPPER_BOUND: u32 = 0x1;

// ---------------------------------------------------------------------------
// OPP (Operating Performance Point) types
// ---------------------------------------------------------------------------

/// OPP is enabled.
pub const OPP_ENABLED: u32 = 1;
/// OPP is disabled.
pub const OPP_DISABLED: u32 = 0;
/// OPP is suspended.
pub const OPP_SUSPENDED: u32 = 2;

// ---------------------------------------------------------------------------
// Common device frequencies (Hz) — illustrative values
// ---------------------------------------------------------------------------

/// 100 MHz.
pub const FREQ_100MHZ: u64 = 100_000_000;
/// 200 MHz.
pub const FREQ_200MHZ: u64 = 200_000_000;
/// 400 MHz.
pub const FREQ_400MHZ: u64 = 400_000_000;
/// 800 MHz.
pub const FREQ_800MHZ: u64 = 800_000_000;
/// 1 GHz.
pub const FREQ_1GHZ: u64 = 1_000_000_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governors_distinct() {
        let govs = [
            DEVFREQ_GOV_PERFORMANCE,
            DEVFREQ_GOV_POWERSAVE,
            DEVFREQ_GOV_SIMPLE_ONDEMAND,
            DEVFREQ_GOV_USERSPACE,
            DEVFREQ_GOV_PASSIVE,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
        }
    }

    #[test]
    fn test_change_events_distinct() {
        assert_ne!(DEVFREQ_PRECHANGE, DEVFREQ_POSTCHANGE);
    }

    #[test]
    fn test_opp_states_distinct() {
        let states = [OPP_DISABLED, OPP_ENABLED, OPP_SUSPENDED];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_frequencies_ascending() {
        assert!(FREQ_100MHZ < FREQ_200MHZ);
        assert!(FREQ_200MHZ < FREQ_400MHZ);
        assert!(FREQ_400MHZ < FREQ_800MHZ);
        assert!(FREQ_800MHZ < FREQ_1GHZ);
    }
}
