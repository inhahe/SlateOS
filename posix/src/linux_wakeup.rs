//! `<linux/pm_wakeup.h>` — Wakeup source constants.
//!
//! Wakeup sources are the mechanism by which devices signal that
//! they can (or have) woken the system from sleep. Power button,
//! network (Wake-on-LAN), USB, keyboard, RTC alarm, and lid switch
//! are all typical wakeup sources.

// ---------------------------------------------------------------------------
// Wakeup event types
// ---------------------------------------------------------------------------

/// Wakeup from user input (keyboard/mouse/touch).
pub const WAKEUP_EVENT_INPUT: u8 = 0;
/// Wakeup from network (WoL/WoWLAN).
pub const WAKEUP_EVENT_NETWORK: u8 = 1;
/// Wakeup from RTC alarm.
pub const WAKEUP_EVENT_RTC: u8 = 2;
/// Wakeup from power button.
pub const WAKEUP_EVENT_POWER_BUTTON: u8 = 3;
/// Wakeup from lid switch.
pub const WAKEUP_EVENT_LID: u8 = 4;
/// Wakeup from USB device.
pub const WAKEUP_EVENT_USB: u8 = 5;
/// Wakeup from Bluetooth.
pub const WAKEUP_EVENT_BLUETOOTH: u8 = 6;
/// Wakeup from timer.
pub const WAKEUP_EVENT_TIMER: u8 = 7;

// ---------------------------------------------------------------------------
// Wakeup source states
// ---------------------------------------------------------------------------

/// Wakeup source inactive.
pub const WAKEUP_STATE_INACTIVE: u8 = 0;
/// Wakeup source active (holding wakelock).
pub const WAKEUP_STATE_ACTIVE: u8 = 1;
/// Wakeup source autosleep-active.
pub const WAKEUP_STATE_AUTOSLEEP: u8 = 2;

// ---------------------------------------------------------------------------
// Wakeup capability flags
// ---------------------------------------------------------------------------

/// Device can wake system from S3 (suspend).
pub const WAKEUP_CAP_S3: u32 = 1 << 0;
/// Device can wake system from S4 (hibernate).
pub const WAKEUP_CAP_S4: u32 = 1 << 1;
/// Device can wake system from S5 (soft-off).
pub const WAKEUP_CAP_S5: u32 = 1 << 2;
/// Device supports runtime wakeup.
pub const WAKEUP_CAP_RUNTIME: u32 = 1 << 3;
/// Device armed for wakeup.
pub const WAKEUP_CAP_ARMED: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Autosleep states
// ---------------------------------------------------------------------------

/// Autosleep disabled.
pub const AUTOSLEEP_DISABLED: u8 = 0;
/// Autosleep to suspend (S3).
pub const AUTOSLEEP_SUSPEND: u8 = 1;
/// Autosleep to hibernate (S4).
pub const AUTOSLEEP_HIBERNATE: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let events = [
            WAKEUP_EVENT_INPUT,
            WAKEUP_EVENT_NETWORK,
            WAKEUP_EVENT_RTC,
            WAKEUP_EVENT_POWER_BUTTON,
            WAKEUP_EVENT_LID,
            WAKEUP_EVENT_USB,
            WAKEUP_EVENT_BLUETOOTH,
            WAKEUP_EVENT_TIMER,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            WAKEUP_STATE_INACTIVE,
            WAKEUP_STATE_ACTIVE,
            WAKEUP_STATE_AUTOSLEEP,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_caps_no_overlap() {
        let caps = [
            WAKEUP_CAP_S3,
            WAKEUP_CAP_S4,
            WAKEUP_CAP_S5,
            WAKEUP_CAP_RUNTIME,
            WAKEUP_CAP_ARMED,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_autosleep_distinct() {
        let modes = [AUTOSLEEP_DISABLED, AUTOSLEEP_SUSPEND, AUTOSLEEP_HIBERNATE];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
