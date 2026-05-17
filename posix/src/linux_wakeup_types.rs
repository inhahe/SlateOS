//! `<linux/pm_wakeup.h>` — Wakeup source constants.
//!
//! Wakeup sources are devices or events that can wake the system
//! from sleep. When the system is suspended, enabled wakeup sources
//! keep their interrupts armed. If triggered (e.g., keyboard press,
//! network packet, RTC alarm, power button), they abort the suspend
//! or wake the system. Wakeup sources are tracked in
//! /sys/class/wakeup/ and reported via wakeup_count in /sys/power/.

// ---------------------------------------------------------------------------
// Wakeup source states
// ---------------------------------------------------------------------------

/// Wakeup source is inactive.
pub const WAKEUP_STATE_INACTIVE: u32 = 0;
/// Wakeup source is active (event in progress).
pub const WAKEUP_STATE_ACTIVE: u32 = 1;

// ---------------------------------------------------------------------------
// Wakeup source types
// ---------------------------------------------------------------------------

/// Power button wakeup.
pub const WAKEUP_TYPE_POWER_BUTTON: u32 = 0;
/// Keyboard/mouse wakeup.
pub const WAKEUP_TYPE_INPUT: u32 = 1;
/// Network (Wake-on-LAN) wakeup.
pub const WAKEUP_TYPE_NETWORK: u32 = 2;
/// RTC alarm wakeup.
pub const WAKEUP_TYPE_RTC: u32 = 3;
/// USB device wakeup.
pub const WAKEUP_TYPE_USB: u32 = 4;
/// Timer wakeup.
pub const WAKEUP_TYPE_TIMER: u32 = 5;
/// Platform-specific wakeup (ACPI GPE, etc.).
pub const WAKEUP_TYPE_PLATFORM: u32 = 6;

// ---------------------------------------------------------------------------
// Wakeup event flags
// ---------------------------------------------------------------------------

/// Event should prevent system suspend.
pub const WAKEUP_FLAG_PREVENT_SUSPEND: u32 = 0x01;
/// Event is a "hard" wakeup (system must wake immediately).
pub const WAKEUP_FLAG_HARD: u32 = 0x02;
/// Wakeup source supports autosleep integration.
pub const WAKEUP_FLAG_AUTOSLEEP: u32 = 0x04;

// ---------------------------------------------------------------------------
// Wakeup statistics
// ---------------------------------------------------------------------------

/// Maximum number of wakeup sources tracked.
pub const WAKEUP_MAX_SOURCES: u32 = 1024;
/// Wakeup event timeout (ms, how long to stay awake after event).
pub const WAKEUP_EVENT_TIMEOUT_MS: u32 = 5000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        assert_ne!(WAKEUP_STATE_INACTIVE, WAKEUP_STATE_ACTIVE);
    }

    #[test]
    fn test_types_distinct() {
        let types = [
            WAKEUP_TYPE_POWER_BUTTON, WAKEUP_TYPE_INPUT,
            WAKEUP_TYPE_NETWORK, WAKEUP_TYPE_RTC,
            WAKEUP_TYPE_USB, WAKEUP_TYPE_TIMER, WAKEUP_TYPE_PLATFORM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            WAKEUP_FLAG_PREVENT_SUSPEND, WAKEUP_FLAG_HARD,
            WAKEUP_FLAG_AUTOSLEEP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert!(WAKEUP_MAX_SOURCES > 0);
        assert!(WAKEUP_EVENT_TIMEOUT_MS > 0);
    }
}
