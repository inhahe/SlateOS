//! `<linux/devfreq.h>` — Additional devfreq constants.
//!
//! Supplementary device frequency scaling constants covering
//! governor types, transition events, and flags.

// ---------------------------------------------------------------------------
// Devfreq governor types
// ---------------------------------------------------------------------------

/// Simple ondemand governor.
pub const DEVFREQ_GOV_SIMPLE_ONDEMAND: u32 = 0;
/// Performance governor.
pub const DEVFREQ_GOV_PERFORMANCE: u32 = 1;
/// Powersave governor.
pub const DEVFREQ_GOV_POWERSAVE: u32 = 2;
/// Userspace governor.
pub const DEVFREQ_GOV_USERSPACE: u32 = 3;
/// Passive governor.
pub const DEVFREQ_GOV_PASSIVE: u32 = 4;

// ---------------------------------------------------------------------------
// Devfreq transition notifications
// ---------------------------------------------------------------------------

/// Pre-change notification.
pub const DEVFREQ_PRECHANGE: u32 = 0;
/// Post-change notification.
pub const DEVFREQ_POSTCHANGE: u32 = 1;

// ---------------------------------------------------------------------------
// Devfreq flags
// ---------------------------------------------------------------------------

/// Device is suspended, don't adjust frequency.
pub const DEVFREQ_FLAG_LEAST_UPPER_BOUND: u32 = 0x1;

// ---------------------------------------------------------------------------
// Devfreq timer types
// ---------------------------------------------------------------------------

/// Deferrable timer.
pub const DEVFREQ_TIMER_DEFERRABLE: u32 = 0;
/// Delayed work timer.
pub const DEVFREQ_TIMER_DELAYED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governors_distinct() {
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
    fn test_transitions_distinct() {
        assert_ne!(DEVFREQ_PRECHANGE, DEVFREQ_POSTCHANGE);
    }

    #[test]
    fn test_timers_distinct() {
        assert_ne!(DEVFREQ_TIMER_DEFERRABLE, DEVFREQ_TIMER_DELAYED);
    }
}
