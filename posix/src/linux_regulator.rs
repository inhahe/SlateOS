//! `<linux/regulator/consumer.h>` — Voltage regulator constants.
//!
//! The regulator framework manages power supply rails (LDOs, buck
//! converters, switches) on embedded and SoC platforms. Userspace
//! sees regulator state via sysfs; kernel drivers use the consumer
//! API to request voltage/current settings.

// ---------------------------------------------------------------------------
// Regulator modes
// ---------------------------------------------------------------------------

/// Fast mode — high current, lowest regulation accuracy.
pub const REGULATOR_MODE_FAST: u32 = 0x01;
/// Normal mode — standard operating mode.
pub const REGULATOR_MODE_NORMAL: u32 = 0x02;
/// Idle mode — device idle, regulator may reduce power.
pub const REGULATOR_MODE_IDLE: u32 = 0x04;
/// Standby mode — device in standby, lowest power.
pub const REGULATOR_MODE_STANDBY: u32 = 0x08;

// ---------------------------------------------------------------------------
// Regulator events
// ---------------------------------------------------------------------------

/// Under-voltage event.
pub const REGULATOR_EVENT_UNDER_VOLTAGE: u32 = 0x01;
/// Over-current event.
pub const REGULATOR_EVENT_OVER_CURRENT: u32 = 0x02;
/// Regulation out of spec.
pub const REGULATOR_EVENT_REGULATION_OUT: u32 = 0x04;
/// Regulator failure.
pub const REGULATOR_EVENT_FAIL: u32 = 0x08;
/// Over-temperature event.
pub const REGULATOR_EVENT_OVER_TEMP: u32 = 0x10;
/// Force disable event.
pub const REGULATOR_EVENT_FORCE_DISABLE: u32 = 0x20;
/// Voltage change event.
pub const REGULATOR_EVENT_VOLTAGE_CHANGE: u32 = 0x40;
/// Disable event.
pub const REGULATOR_EVENT_DISABLE: u32 = 0x80;
/// Pre-voltage change event.
pub const REGULATOR_EVENT_PRE_VOLTAGE_CHANGE: u32 = 0x100;
/// Abort voltage change.
pub const REGULATOR_EVENT_ABORT_VOLTAGE_CHANGE: u32 = 0x200;
/// Pre-disable event.
pub const REGULATOR_EVENT_PRE_DISABLE: u32 = 0x400;
/// Abort disable event.
pub const REGULATOR_EVENT_ABORT_DISABLE: u32 = 0x800;
/// Enable event.
pub const REGULATOR_EVENT_ENABLE: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Regulator status values
// ---------------------------------------------------------------------------

/// Regulator is off.
pub const REGULATOR_STATUS_OFF: i32 = 0;
/// Regulator is on.
pub const REGULATOR_STATUS_ON: i32 = 1;
/// Regulator in error state.
pub const REGULATOR_STATUS_ERROR: i32 = 2;
/// Fast mode.
pub const REGULATOR_STATUS_FAST: i32 = 3;
/// Normal mode.
pub const REGULATOR_STATUS_NORMAL: i32 = 4;
/// Idle mode.
pub const REGULATOR_STATUS_IDLE: i32 = 5;
/// Standby mode.
pub const REGULATOR_STATUS_STANDBY: i32 = 6;
/// Bypass mode.
pub const REGULATOR_STATUS_BYPASS: i32 = 7;
/// Not defined/undefined.
pub const REGULATOR_STATUS_UNDEFINED: i32 = 8;

// ---------------------------------------------------------------------------
// Voltage change direction
// ---------------------------------------------------------------------------

/// Voltage change data.
pub const REGULATOR_CHANGE_VOLTAGE: u32 = 0x01;
/// Current change data.
pub const REGULATOR_CHANGE_CURRENT: u32 = 0x02;
/// Status change data.
pub const REGULATOR_CHANGE_STATUS: u32 = 0x04;
/// Mode change data.
pub const REGULATOR_CHANGE_MODE: u32 = 0x08;
/// Drms mode change data.
pub const REGULATOR_CHANGE_DRMS: u32 = 0x10;
/// Bypass change data.
pub const REGULATOR_CHANGE_BYPASS: u32 = 0x20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_are_powers_of_two() {
        let modes = [
            REGULATOR_MODE_FAST,
            REGULATOR_MODE_NORMAL,
            REGULATOR_MODE_IDLE,
            REGULATOR_MODE_STANDBY,
        ];
        for mode in &modes {
            assert!(mode.is_power_of_two(), "0x{:x} is not a power of two", mode);
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            REGULATOR_MODE_FAST,
            REGULATOR_MODE_NORMAL,
            REGULATOR_MODE_IDLE,
            REGULATOR_MODE_STANDBY,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_events_are_powers_of_two() {
        let events = [
            REGULATOR_EVENT_UNDER_VOLTAGE,
            REGULATOR_EVENT_OVER_CURRENT,
            REGULATOR_EVENT_REGULATION_OUT,
            REGULATOR_EVENT_FAIL,
            REGULATOR_EVENT_OVER_TEMP,
            REGULATOR_EVENT_FORCE_DISABLE,
            REGULATOR_EVENT_VOLTAGE_CHANGE,
            REGULATOR_EVENT_DISABLE,
            REGULATOR_EVENT_PRE_VOLTAGE_CHANGE,
            REGULATOR_EVENT_ABORT_VOLTAGE_CHANGE,
            REGULATOR_EVENT_PRE_DISABLE,
            REGULATOR_EVENT_ABORT_DISABLE,
            REGULATOR_EVENT_ENABLE,
        ];
        for event in &events {
            assert!(
                event.is_power_of_two(),
                "0x{:x} is not a power of two",
                event
            );
        }
    }

    #[test]
    fn test_status_distinct() {
        let statuses = [
            REGULATOR_STATUS_OFF,
            REGULATOR_STATUS_ON,
            REGULATOR_STATUS_ERROR,
            REGULATOR_STATUS_FAST,
            REGULATOR_STATUS_NORMAL,
            REGULATOR_STATUS_IDLE,
            REGULATOR_STATUS_STANDBY,
            REGULATOR_STATUS_BYPASS,
            REGULATOR_STATUS_UNDEFINED,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_change_flags_are_powers_of_two() {
        let flags = [
            REGULATOR_CHANGE_VOLTAGE,
            REGULATOR_CHANGE_CURRENT,
            REGULATOR_CHANGE_STATUS,
            REGULATOR_CHANGE_MODE,
            REGULATOR_CHANGE_DRMS,
            REGULATOR_CHANGE_BYPASS,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_status_values() {
        assert_eq!(REGULATOR_STATUS_OFF, 0);
        assert_eq!(REGULATOR_STATUS_ON, 1);
        assert_eq!(REGULATOR_STATUS_UNDEFINED, 8);
    }
}
