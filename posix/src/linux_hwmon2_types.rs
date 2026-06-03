//! `<linux/hwmon.h>` — Additional hardware monitoring constants.
//!
//! Supplementary hwmon constants covering sensor types,
//! channel attributes, and alarm bits.

// ---------------------------------------------------------------------------
// Sensor types (hwmon_sensor_types)
// ---------------------------------------------------------------------------

/// Chip sensor.
pub const HWMON_TYPE_CHIP: u32 = 0;
/// Temperature sensor.
pub const HWMON_TYPE_TEMP: u32 = 1;
/// Voltage sensor.
pub const HWMON_TYPE_IN: u32 = 2;
/// Current sensor.
pub const HWMON_TYPE_CURR: u32 = 3;
/// Power sensor.
pub const HWMON_TYPE_POWER: u32 = 4;
/// Energy sensor.
pub const HWMON_TYPE_ENERGY: u32 = 5;
/// Humidity sensor.
pub const HWMON_TYPE_HUMIDITY: u32 = 6;
/// Fan sensor.
pub const HWMON_TYPE_FAN: u32 = 7;
/// PWM channel.
pub const HWMON_TYPE_PWM: u32 = 8;
/// Intrusion sensor.
pub const HWMON_TYPE_INTRUSION: u32 = 9;

// ---------------------------------------------------------------------------
// Temperature channel attributes (HWMON_T_*)
// ---------------------------------------------------------------------------

/// Input temperature.
pub const HWMON_T_INPUT: u32 = 1 << 0;
/// Temperature type.
pub const HWMON_T_TYPE: u32 = 1 << 1;
/// Maximum temperature.
pub const HWMON_T_MAX: u32 = 1 << 2;
/// Maximum hysteresis.
pub const HWMON_T_MAX_HYST: u32 = 1 << 3;
/// Minimum temperature.
pub const HWMON_T_MIN: u32 = 1 << 4;
/// Min hysteresis.
pub const HWMON_T_MIN_HYST: u32 = 1 << 5;
/// Critical temperature.
pub const HWMON_T_CRIT: u32 = 1 << 6;
/// Critical hysteresis.
pub const HWMON_T_CRIT_HYST: u32 = 1 << 7;
/// Emergency temperature.
pub const HWMON_T_EMERGENCY: u32 = 1 << 8;
/// Emergency hysteresis.
pub const HWMON_T_EMERGENCY_HYST: u32 = 1 << 9;
/// Alarm.
pub const HWMON_T_ALARM: u32 = 1 << 12;
/// Max alarm.
pub const HWMON_T_MAX_ALARM: u32 = 1 << 13;
/// Min alarm.
pub const HWMON_T_MIN_ALARM: u32 = 1 << 14;
/// Critical alarm.
pub const HWMON_T_CRIT_ALARM: u32 = 1 << 15;
/// Emergency alarm.
pub const HWMON_T_EMERGENCY_ALARM: u32 = 1 << 16;
/// Label.
pub const HWMON_T_LABEL: u32 = 1 << 17;
/// Offset.
pub const HWMON_T_OFFSET: u32 = 1 << 18;

// ---------------------------------------------------------------------------
// Fan channel attributes (HWMON_F_*)
// ---------------------------------------------------------------------------

/// Input speed.
pub const HWMON_F_INPUT: u32 = 1 << 0;
/// Minimum speed.
pub const HWMON_F_MIN: u32 = 1 << 1;
/// Maximum speed.
pub const HWMON_F_MAX: u32 = 1 << 2;
/// Divisor.
pub const HWMON_F_DIV: u32 = 1 << 3;
/// Pulses.
pub const HWMON_F_PULSES: u32 = 1 << 4;
/// Target speed.
pub const HWMON_F_TARGET: u32 = 1 << 5;
/// Alarm.
pub const HWMON_F_ALARM: u32 = 1 << 6;
/// Min alarm.
pub const HWMON_F_MIN_ALARM: u32 = 1 << 7;
/// Max alarm.
pub const HWMON_F_MAX_ALARM: u32 = 1 << 8;
/// Fault.
pub const HWMON_F_FAULT: u32 = 1 << 9;
/// Label.
pub const HWMON_F_LABEL: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_types_sequential() {
        assert_eq!(HWMON_TYPE_CHIP, 0);
        assert_eq!(HWMON_TYPE_TEMP, 1);
        assert_eq!(HWMON_TYPE_INTRUSION, 9);
    }

    #[test]
    fn test_sensor_types_distinct() {
        let types = [
            HWMON_TYPE_CHIP,
            HWMON_TYPE_TEMP,
            HWMON_TYPE_IN,
            HWMON_TYPE_CURR,
            HWMON_TYPE_POWER,
            HWMON_TYPE_ENERGY,
            HWMON_TYPE_HUMIDITY,
            HWMON_TYPE_FAN,
            HWMON_TYPE_PWM,
            HWMON_TYPE_INTRUSION,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_temp_attrs_power_of_two() {
        let attrs = [
            HWMON_T_INPUT,
            HWMON_T_TYPE,
            HWMON_T_MAX,
            HWMON_T_MAX_HYST,
            HWMON_T_MIN,
            HWMON_T_MIN_HYST,
            HWMON_T_CRIT,
            HWMON_T_CRIT_HYST,
            HWMON_T_EMERGENCY,
            HWMON_T_EMERGENCY_HYST,
            HWMON_T_ALARM,
            HWMON_T_MAX_ALARM,
            HWMON_T_MIN_ALARM,
            HWMON_T_CRIT_ALARM,
            HWMON_T_EMERGENCY_ALARM,
            HWMON_T_LABEL,
            HWMON_T_OFFSET,
        ];
        for a in &attrs {
            assert!(a.is_power_of_two(), "0x{:08x} not power of two", a);
        }
    }

    #[test]
    fn test_fan_attrs_power_of_two() {
        let attrs = [
            HWMON_F_INPUT,
            HWMON_F_MIN,
            HWMON_F_MAX,
            HWMON_F_DIV,
            HWMON_F_PULSES,
            HWMON_F_TARGET,
            HWMON_F_ALARM,
            HWMON_F_MIN_ALARM,
            HWMON_F_MAX_ALARM,
            HWMON_F_FAULT,
            HWMON_F_LABEL,
        ];
        for a in &attrs {
            assert!(a.is_power_of_two(), "0x{:08x} not power of two", a);
        }
    }
}
