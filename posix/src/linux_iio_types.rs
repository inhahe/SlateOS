//! `<linux/iio/types.h>` — Industrial I/O (IIO) subsystem constants.
//!
//! IIO provides a unified framework for sensors and data acquisition
//! devices: accelerometers, gyroscopes, magnetometers, ADCs, DACs,
//! light sensors, pressure sensors, humidity sensors, and more.
//! It supports both polled and triggered (buffered) data capture.

// ---------------------------------------------------------------------------
// IIO channel types
// ---------------------------------------------------------------------------

/// Voltage.
pub const IIO_VOLTAGE: u8 = 0;
/// Current.
pub const IIO_CURRENT: u8 = 1;
/// Power.
pub const IIO_POWER: u8 = 2;
/// Acceleration.
pub const IIO_ACCEL: u8 = 3;
/// Angular velocity (gyroscope).
pub const IIO_ANGL_VEL: u8 = 4;
/// Magnetic field.
pub const IIO_MAGN: u8 = 5;
/// Light (illuminance).
pub const IIO_LIGHT: u8 = 6;
/// Intensity.
pub const IIO_INTENSITY: u8 = 7;
/// Proximity.
pub const IIO_PROXIMITY: u8 = 8;
/// Temperature.
pub const IIO_TEMP: u8 = 9;
/// Capacitance.
pub const IIO_CAPACITANCE: u8 = 10;
/// Angle (inclinometer).
pub const IIO_INCLI: u8 = 11;
/// Rotation.
pub const IIO_ROT: u8 = 12;
/// Pressure.
pub const IIO_PRESSURE: u8 = 13;
/// Humidity (relative).
pub const IIO_HUMIDITYRELATIVE: u8 = 14;
/// Activity (step counter, etc.).
pub const IIO_ACTIVITY: u8 = 15;
/// Steps.
pub const IIO_STEPS: u8 = 16;

// ---------------------------------------------------------------------------
// IIO event types
// ---------------------------------------------------------------------------

/// Threshold event.
pub const IIO_EV_TYPE_THRESH: u8 = 0;
/// Magnitude event.
pub const IIO_EV_TYPE_MAG: u8 = 1;
/// Rate-of-change event.
pub const IIO_EV_TYPE_ROC: u8 = 2;
/// Threshold adaptive event.
pub const IIO_EV_TYPE_THRESH_ADAPTIVE: u8 = 3;
/// Magnitude adaptive event.
pub const IIO_EV_TYPE_MAG_ADAPTIVE: u8 = 4;
/// Change event.
pub const IIO_EV_TYPE_CHANGE: u8 = 5;

// ---------------------------------------------------------------------------
// IIO event directions
// ---------------------------------------------------------------------------

/// Either direction.
pub const IIO_EV_DIR_EITHER: u8 = 0;
/// Rising.
pub const IIO_EV_DIR_RISING: u8 = 1;
/// Falling.
pub const IIO_EV_DIR_FALLING: u8 = 2;

// ---------------------------------------------------------------------------
// IIO modifier types (axis or variant)
// ---------------------------------------------------------------------------

/// X axis.
pub const IIO_MOD_X: u8 = 0;
/// Y axis.
pub const IIO_MOD_Y: u8 = 1;
/// Z axis.
pub const IIO_MOD_Z: u8 = 2;
/// X and Y combined.
pub const IIO_MOD_X_AND_Y: u8 = 3;
/// X and Z combined.
pub const IIO_MOD_X_AND_Z: u8 = 4;
/// Y and Z combined.
pub const IIO_MOD_Y_AND_Z: u8 = 5;
/// Root sum square.
pub const IIO_MOD_ROOT_SUM_SQUARED: u8 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_types_distinct() {
        let types = [
            IIO_VOLTAGE,
            IIO_CURRENT,
            IIO_POWER,
            IIO_ACCEL,
            IIO_ANGL_VEL,
            IIO_MAGN,
            IIO_LIGHT,
            IIO_INTENSITY,
            IIO_PROXIMITY,
            IIO_TEMP,
            IIO_CAPACITANCE,
            IIO_INCLI,
            IIO_ROT,
            IIO_PRESSURE,
            IIO_HUMIDITYRELATIVE,
            IIO_ACTIVITY,
            IIO_STEPS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_event_types_distinct() {
        let types = [
            IIO_EV_TYPE_THRESH,
            IIO_EV_TYPE_MAG,
            IIO_EV_TYPE_ROC,
            IIO_EV_TYPE_THRESH_ADAPTIVE,
            IIO_EV_TYPE_MAG_ADAPTIVE,
            IIO_EV_TYPE_CHANGE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_directions_distinct() {
        let dirs = [IIO_EV_DIR_EITHER, IIO_EV_DIR_RISING, IIO_EV_DIR_FALLING];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_modifiers_distinct() {
        let mods = [
            IIO_MOD_X,
            IIO_MOD_Y,
            IIO_MOD_Z,
            IIO_MOD_X_AND_Y,
            IIO_MOD_X_AND_Z,
            IIO_MOD_Y_AND_Z,
            IIO_MOD_ROOT_SUM_SQUARED,
        ];
        for i in 0..mods.len() {
            for j in (i + 1)..mods.len() {
                assert_ne!(mods[i], mods[j]);
            }
        }
    }
}
