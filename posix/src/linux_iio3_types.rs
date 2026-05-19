//! `<linux/iio/types.h>` — Additional IIO (Industrial I/O) constants (part 3).
//!
//! Supplementary IIO constants covering channel types,
//! event types, and modifier values.

// ---------------------------------------------------------------------------
// IIO channel types
// ---------------------------------------------------------------------------

/// Voltage.
pub const IIO_VOLTAGE: u32 = 0;
/// Current.
pub const IIO_CURRENT: u32 = 1;
/// Power.
pub const IIO_POWER: u32 = 2;
/// Acceleration.
pub const IIO_ACCEL: u32 = 3;
/// Angular velocity.
pub const IIO_ANGL_VEL: u32 = 4;
/// Magnetic field.
pub const IIO_MAGN: u32 = 5;
/// Light/illuminance.
pub const IIO_LIGHT: u32 = 6;
/// Intensity.
pub const IIO_INTENSITY: u32 = 7;
/// Proximity.
pub const IIO_PROXIMITY: u32 = 8;
/// Temperature.
pub const IIO_TEMP: u32 = 9;
/// Capacitance.
pub const IIO_CAPACITANCE: u32 = 10;
/// Pressure.
pub const IIO_PRESSURE: u32 = 13;
/// Humidity.
pub const IIO_HUMIDITYRELATIVE: u32 = 14;
/// Steps.
pub const IIO_STEPS: u32 = 21;
/// Activity.
pub const IIO_ACTIVITY: u32 = 18;
/// Rotation.
pub const IIO_ROT: u32 = 15;
/// Angle.
pub const IIO_ANGL: u32 = 16;
/// Timestamp.
pub const IIO_TIMESTAMP: u32 = 17;

// ---------------------------------------------------------------------------
// IIO event types
// ---------------------------------------------------------------------------

/// Threshold event.
pub const IIO_EV_TYPE_THRESH: u32 = 0;
/// Magnitude event.
pub const IIO_EV_TYPE_MAG: u32 = 1;
/// Rate of change event.
pub const IIO_EV_TYPE_ROC: u32 = 2;
/// Threshold adaptive event.
pub const IIO_EV_TYPE_THRESH_ADAPTIVE: u32 = 3;
/// Magnitude adaptive event.
pub const IIO_EV_TYPE_MAG_ADAPTIVE: u32 = 4;
/// Change event.
pub const IIO_EV_TYPE_CHANGE: u32 = 5;
/// Gesture event.
pub const IIO_EV_TYPE_GESTURE: u32 = 6;

// ---------------------------------------------------------------------------
// IIO event directions
// ---------------------------------------------------------------------------

/// Either direction.
pub const IIO_EV_DIR_EITHER: u32 = 0;
/// Rising direction.
pub const IIO_EV_DIR_RISING: u32 = 1;
/// Falling direction.
pub const IIO_EV_DIR_FALLING: u32 = 2;
/// None direction.
pub const IIO_EV_DIR_NONE: u32 = 3;
/// Singletap.
pub const IIO_EV_DIR_SINGLETAP: u32 = 4;
/// Doubletap.
pub const IIO_EV_DIR_DOUBLETAP: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chan_types_distinct() {
        let types = [
            IIO_VOLTAGE, IIO_CURRENT, IIO_POWER, IIO_ACCEL,
            IIO_ANGL_VEL, IIO_MAGN, IIO_LIGHT, IIO_INTENSITY,
            IIO_PROXIMITY, IIO_TEMP, IIO_CAPACITANCE,
            IIO_PRESSURE, IIO_HUMIDITYRELATIVE, IIO_STEPS,
            IIO_ACTIVITY, IIO_ROT, IIO_ANGL, IIO_TIMESTAMP,
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
            IIO_EV_TYPE_THRESH, IIO_EV_TYPE_MAG,
            IIO_EV_TYPE_ROC, IIO_EV_TYPE_THRESH_ADAPTIVE,
            IIO_EV_TYPE_MAG_ADAPTIVE, IIO_EV_TYPE_CHANGE,
            IIO_EV_TYPE_GESTURE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_event_dirs_distinct() {
        let dirs = [
            IIO_EV_DIR_EITHER, IIO_EV_DIR_RISING,
            IIO_EV_DIR_FALLING, IIO_EV_DIR_NONE,
            IIO_EV_DIR_SINGLETAP, IIO_EV_DIR_DOUBLETAP,
        ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }
}
