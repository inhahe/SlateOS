//! `<linux/regulator/consumer.h>` — Voltage/current regulator constants.
//!
//! The Linux regulator framework manages power supplies (voltage and
//! current regulators) on embedded systems. It handles enable/disable,
//! voltage/current setting, and power sequencing for PMICs (Power
//! Management ICs) that supply different voltage rails.

// ---------------------------------------------------------------------------
// Regulator modes
// ---------------------------------------------------------------------------

/// Fast/normal mode (highest performance).
pub const REGULATOR_MODE_FAST: u32 = 0x01;
/// Normal mode.
pub const REGULATOR_MODE_NORMAL: u32 = 0x02;
/// Idle mode (intermediate power saving).
pub const REGULATOR_MODE_IDLE: u32 = 0x04;
/// Standby mode (lowest power, slowest response).
pub const REGULATOR_MODE_STANDBY: u32 = 0x08;

// ---------------------------------------------------------------------------
// Regulator events
// ---------------------------------------------------------------------------

/// Under voltage.
pub const REGULATOR_EVENT_UNDER_VOLTAGE: u32 = 0x01;
/// Over current.
pub const REGULATOR_EVENT_OVER_CURRENT: u32 = 0x02;
/// Regulation out (can't maintain voltage).
pub const REGULATOR_EVENT_REGULATION_OUT: u32 = 0x04;
/// Failed (hardware fault).
pub const REGULATOR_EVENT_FAIL: u32 = 0x08;
/// Over temperature.
pub const REGULATOR_EVENT_OVER_TEMP: u32 = 0x10;
/// Force disabled.
pub const REGULATOR_EVENT_FORCE_DISABLE: u32 = 0x20;
/// Voltage changed.
pub const REGULATOR_EVENT_VOLTAGE_CHANGE: u32 = 0x40;
/// Regulator disabled.
pub const REGULATOR_EVENT_DISABLE: u32 = 0x80;
/// Pre-voltage change notification.
pub const REGULATOR_EVENT_PRE_VOLTAGE_CHANGE: u32 = 0x100;

// ---------------------------------------------------------------------------
// Regulator change reasons
// ---------------------------------------------------------------------------

/// User requested change.
pub const REGULATOR_CHANGE_USER: u32 = 0x01;
/// System/platform constraint change.
pub const REGULATOR_CHANGE_SYSTEM: u32 = 0x02;
/// Voltage droop compensation.
pub const REGULATOR_CHANGE_DROOP: u32 = 0x04;

// ---------------------------------------------------------------------------
// Regulator status
// ---------------------------------------------------------------------------

/// Regulator is off.
pub const REGULATOR_STATUS_OFF: u8 = 0;
/// Regulator is on.
pub const REGULATOR_STATUS_ON: u8 = 1;
/// Regulator in error state.
pub const REGULATOR_STATUS_ERROR: u8 = 2;
/// Fast mode active.
pub const REGULATOR_STATUS_FAST: u8 = 3;
/// Normal mode active.
pub const REGULATOR_STATUS_NORMAL: u8 = 4;
/// Idle mode active.
pub const REGULATOR_STATUS_IDLE: u8 = 5;
/// Standby mode active.
pub const REGULATOR_STATUS_STANDBY: u8 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_no_overlap() {
        let modes = [
            REGULATOR_MODE_FAST, REGULATOR_MODE_NORMAL,
            REGULATOR_MODE_IDLE, REGULATOR_MODE_STANDBY,
        ];
        for i in 0..modes.len() {
            assert!(modes[i].is_power_of_two());
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_events_no_overlap() {
        let events = [
            REGULATOR_EVENT_UNDER_VOLTAGE, REGULATOR_EVENT_OVER_CURRENT,
            REGULATOR_EVENT_REGULATION_OUT, REGULATOR_EVENT_FAIL,
            REGULATOR_EVENT_OVER_TEMP, REGULATOR_EVENT_FORCE_DISABLE,
            REGULATOR_EVENT_VOLTAGE_CHANGE, REGULATOR_EVENT_DISABLE,
            REGULATOR_EVENT_PRE_VOLTAGE_CHANGE,
        ];
        for i in 0..events.len() {
            assert!(events[i].is_power_of_two());
            for j in (i + 1)..events.len() {
                assert_eq!(events[i] & events[j], 0);
            }
        }
    }

    #[test]
    fn test_status_values_distinct() {
        let statuses = [
            REGULATOR_STATUS_OFF, REGULATOR_STATUS_ON,
            REGULATOR_STATUS_ERROR, REGULATOR_STATUS_FAST,
            REGULATOR_STATUS_NORMAL, REGULATOR_STATUS_IDLE,
            REGULATOR_STATUS_STANDBY,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }
}
