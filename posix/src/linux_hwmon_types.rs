//! `<linux/hwmon.h>` — Hardware monitoring sensor constants.
//!
//! hwmon exposes hardware sensors (temperature, voltage, fan speed,
//! power, current, humidity) via sysfs. Each sensor has a type, index,
//! and attribute (input, max, min, crit, label). Used by lm-sensors,
//! system monitors, and thermal management daemons.

// ---------------------------------------------------------------------------
// Sensor types (hwmon_sensor_types)
// ---------------------------------------------------------------------------

/// Temperature sensor.
pub const HWMON_T_TEMP: u32 = 0;
/// Voltage sensor.
pub const HWMON_T_IN: u32 = 1;
/// Current sensor.
pub const HWMON_T_CURR: u32 = 2;
/// Power sensor.
pub const HWMON_T_POWER: u32 = 3;
/// Energy sensor.
pub const HWMON_T_ENERGY: u32 = 4;
/// Humidity sensor.
pub const HWMON_T_HUMIDITY: u32 = 5;
/// Fan speed sensor.
pub const HWMON_T_FAN: u32 = 6;
/// PWM control.
pub const HWMON_T_PWM: u32 = 7;
/// Intrusion detector.
pub const HWMON_T_INTRUSION: u32 = 8;

// ---------------------------------------------------------------------------
// Temperature attributes
// ---------------------------------------------------------------------------

/// Current temperature (millidegrees C).
pub const HWMON_TEMP_INPUT: u32 = 1 << 0;
/// Maximum temperature.
pub const HWMON_TEMP_MAX: u32 = 1 << 1;
/// Maximum hysteresis.
pub const HWMON_TEMP_MAX_HYST: u32 = 1 << 2;
/// Minimum temperature.
pub const HWMON_TEMP_MIN: u32 = 1 << 3;
/// Critical temperature.
pub const HWMON_TEMP_CRIT: u32 = 1 << 4;
/// Critical hysteresis.
pub const HWMON_TEMP_CRIT_HYST: u32 = 1 << 5;
/// Emergency temperature.
pub const HWMON_TEMP_EMERGENCY: u32 = 1 << 6;
/// Sensor label.
pub const HWMON_TEMP_LABEL: u32 = 1 << 7;
/// Alarm flag.
pub const HWMON_TEMP_ALARM: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Fan attributes
// ---------------------------------------------------------------------------

/// Current fan speed (RPM).
pub const HWMON_FAN_INPUT: u32 = 1 << 0;
/// Minimum fan speed.
pub const HWMON_FAN_MIN: u32 = 1 << 1;
/// Maximum fan speed.
pub const HWMON_FAN_MAX: u32 = 1 << 2;
/// Fan target speed.
pub const HWMON_FAN_TARGET: u32 = 1 << 3;
/// Fan alarm.
pub const HWMON_FAN_ALARM: u32 = 1 << 4;
/// Fan label.
pub const HWMON_FAN_LABEL: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_types_distinct() {
        let types = [
            HWMON_T_TEMP, HWMON_T_IN, HWMON_T_CURR, HWMON_T_POWER,
            HWMON_T_ENERGY, HWMON_T_HUMIDITY, HWMON_T_FAN,
            HWMON_T_PWM, HWMON_T_INTRUSION,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_temp_attrs_no_overlap() {
        let attrs = [
            HWMON_TEMP_INPUT, HWMON_TEMP_MAX, HWMON_TEMP_MAX_HYST,
            HWMON_TEMP_MIN, HWMON_TEMP_CRIT, HWMON_TEMP_CRIT_HYST,
            HWMON_TEMP_EMERGENCY, HWMON_TEMP_LABEL, HWMON_TEMP_ALARM,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_fan_attrs_no_overlap() {
        let attrs = [
            HWMON_FAN_INPUT, HWMON_FAN_MIN, HWMON_FAN_MAX,
            HWMON_FAN_TARGET, HWMON_FAN_ALARM, HWMON_FAN_LABEL,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }
}
