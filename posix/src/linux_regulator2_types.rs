//! `<linux/regulator/consumer.h>` — Additional voltage regulator constants.
//!
//! Supplementary regulator constants covering modes,
//! events, status values, and change reasons.

// ---------------------------------------------------------------------------
// Regulator operating modes
// ---------------------------------------------------------------------------

/// Fast mode (highest performance).
pub const REGULATOR_MODE_FAST: u32 = 0x01;
/// Normal mode.
pub const REGULATOR_MODE_NORMAL: u32 = 0x02;
/// Idle mode (low power).
pub const REGULATOR_MODE_IDLE: u32 = 0x04;
/// Standby mode (lowest power).
pub const REGULATOR_MODE_STANDBY: u32 = 0x08;

// ---------------------------------------------------------------------------
// Regulator events
// ---------------------------------------------------------------------------

/// Under-voltage.
pub const REGULATOR_EVENT_UNDER_VOLTAGE: u32 = 0x01;
/// Over-current.
pub const REGULATOR_EVENT_OVER_CURRENT: u32 = 0x02;
/// Regulation out.
pub const REGULATOR_EVENT_REGULATION_OUT: u32 = 0x04;
/// Fail.
pub const REGULATOR_EVENT_FAIL: u32 = 0x08;
/// Over-temperature.
pub const REGULATOR_EVENT_OVER_TEMP: u32 = 0x10;
/// Force disable.
pub const REGULATOR_EVENT_FORCE_DISABLE: u32 = 0x20;
/// Voltage change.
pub const REGULATOR_EVENT_VOLTAGE_CHANGE: u32 = 0x40;
/// Disable.
pub const REGULATOR_EVENT_DISABLE: u32 = 0x80;
/// Pre-voltage change.
pub const REGULATOR_EVENT_PRE_VOLTAGE_CHANGE: u32 = 0x100;
/// Abort voltage change.
pub const REGULATOR_EVENT_ABORT_VOLTAGE_CHANGE: u32 = 0x200;
/// Pre-disable.
pub const REGULATOR_EVENT_PRE_DISABLE: u32 = 0x400;
/// Abort disable.
pub const REGULATOR_EVENT_ABORT_DISABLE: u32 = 0x800;
/// Enable.
pub const REGULATOR_EVENT_ENABLE: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Regulator status
// ---------------------------------------------------------------------------

/// Off.
pub const REGULATOR_STATUS_OFF: u32 = 0;
/// On.
pub const REGULATOR_STATUS_ON: u32 = 1;
/// Error.
pub const REGULATOR_STATUS_ERROR: u32 = 2;
/// Fast.
pub const REGULATOR_STATUS_FAST: u32 = 3;
/// Normal.
pub const REGULATOR_STATUS_NORMAL: u32 = 4;
/// Idle.
pub const REGULATOR_STATUS_IDLE: u32 = 5;
/// Standby.
pub const REGULATOR_STATUS_STANDBY: u32 = 6;
/// Bypass.
pub const REGULATOR_STATUS_BYPASS: u32 = 7;
/// Undefined.
pub const REGULATOR_STATUS_UNDEFINED: u32 = 8;

// ---------------------------------------------------------------------------
// Regulator change reasons
// ---------------------------------------------------------------------------

/// User request.
pub const REGULATOR_CHANGE_USER: u32 = 0x01;
/// System requirement.
pub const REGULATOR_CHANGE_SYSTEM: u32 = 0x02;
/// Thermal event.
pub const REGULATOR_CHANGE_THERMAL: u32 = 0x04;
/// Drms (dynamic).
pub const REGULATOR_CHANGE_DRMS: u32 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_power_of_two() {
        let modes = [
            REGULATOR_MODE_FAST,
            REGULATOR_MODE_NORMAL,
            REGULATOR_MODE_IDLE,
            REGULATOR_MODE_STANDBY,
        ];
        for m in &modes {
            assert!(m.is_power_of_two(), "0x{:02x} not power of two", m);
        }
    }

    #[test]
    fn test_modes_no_overlap() {
        let modes = [
            REGULATOR_MODE_FAST,
            REGULATOR_MODE_NORMAL,
            REGULATOR_MODE_IDLE,
            REGULATOR_MODE_STANDBY,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_events_power_of_two() {
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
        for e in &events {
            assert!(e.is_power_of_two(), "0x{:04x} not power of two", e);
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
    fn test_change_reasons_no_overlap() {
        let reasons = [
            REGULATOR_CHANGE_USER,
            REGULATOR_CHANGE_SYSTEM,
            REGULATOR_CHANGE_THERMAL,
            REGULATOR_CHANGE_DRMS,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_eq!(reasons[i] & reasons[j], 0);
            }
        }
    }
}
