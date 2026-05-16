//! `<linux/hwmon.h>` — Hardware monitoring constants.
//!
//! The hwmon subsystem exposes temperature, voltage, fan speed, and
//! power sensors via sysfs. Userspace tools like `lm-sensors` and
//! system monitors read these values for health reporting.

// ---------------------------------------------------------------------------
// Sensor types (hwmon_sensor_types enum equivalent)
// ---------------------------------------------------------------------------

/// Chip-level attributes.
pub const HWMON_TYPE_CHIP: u32 = 0;
/// Temperature sensor.
pub const HWMON_TYPE_TEMP: u32 = 1;
/// Input voltage sensor.
pub const HWMON_TYPE_IN: u32 = 2;
/// Current sensor.
pub const HWMON_TYPE_CURR: u32 = 3;
/// Power sensor.
pub const HWMON_TYPE_POWER: u32 = 4;
/// Energy sensor.
pub const HWMON_TYPE_ENERGY: u32 = 5;
/// Humidity sensor.
pub const HWMON_TYPE_HUMIDITY: u32 = 6;
/// Fan speed sensor.
pub const HWMON_TYPE_FAN: u32 = 7;
/// PWM output.
pub const HWMON_TYPE_PWM: u32 = 8;
/// Intrusion detection.
pub const HWMON_TYPE_INTRUSION: u32 = 9;

// ---------------------------------------------------------------------------
// Temperature attributes (hwmon_temp_attributes bit flags)
// ---------------------------------------------------------------------------

/// Enable/disable sensor.
pub const HWMON_T_ENABLE: u32 = 1 << 0;
/// Current temperature reading.
pub const HWMON_T_INPUT: u32 = 1 << 1;
/// Minimum temperature.
pub const HWMON_T_MIN: u32 = 1 << 2;
/// Maximum temperature.
pub const HWMON_T_MAX: u32 = 1 << 3;
/// Critical temperature.
pub const HWMON_T_CRIT: u32 = 1 << 4;
/// Critical hysteresis.
pub const HWMON_T_CRIT_HYST: u32 = 1 << 5;
/// Emergency temperature.
pub const HWMON_T_EMERGENCY: u32 = 1 << 6;
/// Emergency hysteresis.
pub const HWMON_T_EMERGENCY_HYST: u32 = 1 << 7;
/// Lowest recorded value.
pub const HWMON_T_LOWEST: u32 = 1 << 8;
/// Highest recorded value.
pub const HWMON_T_HIGHEST: u32 = 1 << 9;
/// Minimum alarm.
pub const HWMON_T_MIN_ALARM: u32 = 1 << 10;
/// Maximum alarm.
pub const HWMON_T_MAX_ALARM: u32 = 1 << 11;
/// Critical alarm.
pub const HWMON_T_CRIT_ALARM: u32 = 1 << 12;
/// Sensor label.
pub const HWMON_T_LABEL: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// Fan attributes
// ---------------------------------------------------------------------------

/// Enable/disable fan.
pub const HWMON_F_ENABLE: u32 = 1 << 0;
/// Fan speed input (RPM).
pub const HWMON_F_INPUT: u32 = 1 << 1;
/// Minimum fan speed.
pub const HWMON_F_MIN: u32 = 1 << 2;
/// Maximum fan speed.
pub const HWMON_F_MAX: u32 = 1 << 3;
/// Fan target speed.
pub const HWMON_F_TARGET: u32 = 1 << 4;
/// Fan fault.
pub const HWMON_F_FAULT: u32 = 1 << 5;
/// Minimum alarm.
pub const HWMON_F_MIN_ALARM: u32 = 1 << 6;
/// Maximum alarm.
pub const HWMON_F_MAX_ALARM: u32 = 1 << 7;
/// Fan label.
pub const HWMON_F_LABEL: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// PWM attributes
// ---------------------------------------------------------------------------

/// PWM output value (0-255).
pub const HWMON_PWM_INPUT: u32 = 1 << 0;
/// PWM enable mode.
pub const HWMON_PWM_ENABLE: u32 = 1 << 1;
/// PWM frequency.
pub const HWMON_PWM_FREQ: u32 = 1 << 2;
/// PWM auto-channels temperature.
pub const HWMON_PWM_AUTO_CHANNELS_TEMP: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Voltage attributes
// ---------------------------------------------------------------------------

/// Enable/disable voltage sensor.
pub const HWMON_I_ENABLE: u32 = 1 << 0;
/// Voltage reading.
pub const HWMON_I_INPUT: u32 = 1 << 1;
/// Minimum voltage.
pub const HWMON_I_MIN: u32 = 1 << 2;
/// Maximum voltage.
pub const HWMON_I_MAX: u32 = 1 << 3;
/// Voltage label.
pub const HWMON_I_LABEL: u32 = 1 << 4;
/// Minimum alarm.
pub const HWMON_I_MIN_ALARM: u32 = 1 << 5;
/// Maximum alarm.
pub const HWMON_I_MAX_ALARM: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_types_distinct() {
        let types = [
            HWMON_TYPE_CHIP, HWMON_TYPE_TEMP, HWMON_TYPE_IN,
            HWMON_TYPE_CURR, HWMON_TYPE_POWER, HWMON_TYPE_ENERGY,
            HWMON_TYPE_HUMIDITY, HWMON_TYPE_FAN, HWMON_TYPE_PWM,
            HWMON_TYPE_INTRUSION,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_temp_attrs_are_powers_of_two() {
        let attrs = [
            HWMON_T_ENABLE, HWMON_T_INPUT, HWMON_T_MIN, HWMON_T_MAX,
            HWMON_T_CRIT, HWMON_T_CRIT_HYST, HWMON_T_EMERGENCY,
            HWMON_T_EMERGENCY_HYST, HWMON_T_LOWEST, HWMON_T_HIGHEST,
            HWMON_T_MIN_ALARM, HWMON_T_MAX_ALARM, HWMON_T_CRIT_ALARM,
            HWMON_T_LABEL,
        ];
        for attr in &attrs {
            assert!(attr.is_power_of_two(), "0x{:x} is not a power of two", attr);
        }
    }

    #[test]
    fn test_fan_attrs_are_powers_of_two() {
        let attrs = [
            HWMON_F_ENABLE, HWMON_F_INPUT, HWMON_F_MIN, HWMON_F_MAX,
            HWMON_F_TARGET, HWMON_F_FAULT, HWMON_F_MIN_ALARM,
            HWMON_F_MAX_ALARM, HWMON_F_LABEL,
        ];
        for attr in &attrs {
            assert!(attr.is_power_of_two());
        }
    }

    #[test]
    fn test_pwm_attrs_are_powers_of_two() {
        let attrs = [
            HWMON_PWM_INPUT, HWMON_PWM_ENABLE, HWMON_PWM_FREQ,
            HWMON_PWM_AUTO_CHANNELS_TEMP,
        ];
        for attr in &attrs {
            assert!(attr.is_power_of_two());
        }
    }

    #[test]
    fn test_voltage_attrs_are_powers_of_two() {
        let attrs = [
            HWMON_I_ENABLE, HWMON_I_INPUT, HWMON_I_MIN, HWMON_I_MAX,
            HWMON_I_LABEL, HWMON_I_MIN_ALARM, HWMON_I_MAX_ALARM,
        ];
        for attr in &attrs {
            assert!(attr.is_power_of_two());
        }
    }

    #[test]
    fn test_sensor_type_values() {
        assert_eq!(HWMON_TYPE_CHIP, 0);
        assert_eq!(HWMON_TYPE_TEMP, 1);
        assert_eq!(HWMON_TYPE_FAN, 7);
    }
}
