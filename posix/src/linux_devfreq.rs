//! `<linux/devfreq.h>` — Device frequency scaling constants.
//!
//! devfreq provides dynamic voltage and frequency scaling for
//! non-CPU devices (GPU, memory bus, etc.). Similar to cpufreq
//! but for arbitrary devices. Governors choose frequencies based
//! on device utilization statistics.

// ---------------------------------------------------------------------------
// Governor names
// ---------------------------------------------------------------------------

/// Simple on-demand governor.
pub const DEVFREQ_GOV_SIMPLE_ONDEMAND: &str = "simple_ondemand";
/// Performance governor — always max.
pub const DEVFREQ_GOV_PERFORMANCE: &str = "performance";
/// Powersave governor — always min.
pub const DEVFREQ_GOV_POWERSAVE: &str = "powersave";
/// Userspace governor.
pub const DEVFREQ_GOV_USERSPACE: &str = "userspace";
/// Passive governor — follows another devfreq device.
pub const DEVFREQ_GOV_PASSIVE: &str = "passive";

// ---------------------------------------------------------------------------
// Transition events
// ---------------------------------------------------------------------------

/// Pre-change notification.
pub const DEVFREQ_PRECHANGE: u32 = 0;
/// Post-change notification.
pub const DEVFREQ_POSTCHANGE: u32 = 1;

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// Device supports frequency table.
pub const DEVFREQ_FLAG_LEAST_UPPER_BOUND: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Timer types
// ---------------------------------------------------------------------------

/// Deferrable timer (does not wake from idle).
pub const DEVFREQ_TIMER_DEFERRABLE: u32 = 0;
/// Delayed timer (normal timer).
pub const DEVFREQ_TIMER_DELAYED: u32 = 1;

// ---------------------------------------------------------------------------
// Transition table
// ---------------------------------------------------------------------------

/// Initial frequency (ask driver for the current frequency).
pub const DEVFREQ_INITIAL_FREQ: u32 = 0;

/// Polling interval disabled.
pub const DEVFREQ_POLLING_DISABLED: u32 = 0;

// ---------------------------------------------------------------------------
// Simple on-demand governor tunables
// ---------------------------------------------------------------------------

/// Default upthreshold (percentage).
pub const DEVFREQ_ONDEMAND_UPTHRESHOLD_DEFAULT: u32 = 90;
/// Default downdifferential.
pub const DEVFREQ_ONDEMAND_DOWNDIFF_DEFAULT: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governor_names_distinct() {
        let govs = [
            DEVFREQ_GOV_SIMPLE_ONDEMAND,
            DEVFREQ_GOV_PERFORMANCE,
            DEVFREQ_GOV_POWERSAVE,
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
    fn test_transition_events_distinct() {
        assert_ne!(DEVFREQ_PRECHANGE, DEVFREQ_POSTCHANGE);
    }

    #[test]
    fn test_timer_types_distinct() {
        assert_ne!(DEVFREQ_TIMER_DEFERRABLE, DEVFREQ_TIMER_DELAYED);
    }

    #[test]
    fn test_flag_is_power_of_two() {
        assert!(DEVFREQ_FLAG_LEAST_UPPER_BOUND.is_power_of_two());
    }

    #[test]
    fn test_ondemand_defaults() {
        assert_eq!(DEVFREQ_ONDEMAND_UPTHRESHOLD_DEFAULT, 90);
        assert_eq!(DEVFREQ_ONDEMAND_DOWNDIFF_DEFAULT, 5);
        assert!(DEVFREQ_ONDEMAND_UPTHRESHOLD_DEFAULT > DEVFREQ_ONDEMAND_DOWNDIFF_DEFAULT);
    }

    #[test]
    fn test_initial_freq() {
        assert_eq!(DEVFREQ_INITIAL_FREQ, 0);
    }

    #[test]
    fn test_polling_disabled() {
        assert_eq!(DEVFREQ_POLLING_DISABLED, 0);
    }
}
