//! `<linux/iio/types.h>` + `<linux/iio/events.h>` — Industrial I/O constants.
//!
//! The IIO subsystem handles sensors: accelerometers, gyroscopes,
//! magnetometers, light sensors, pressure sensors, ADCs, DACs, and
//! similar devices. Data is exposed via sysfs and character devices
//! with trigger-based buffer support.

// ---------------------------------------------------------------------------
// IIO channel types (iio_chan_type enum equivalent)
// ---------------------------------------------------------------------------

/// Voltage channel.
pub const IIO_VOLTAGE: u32 = 0;
/// Current channel.
pub const IIO_CURRENT: u32 = 1;
/// Power channel.
pub const IIO_POWER: u32 = 2;
/// Acceleration.
pub const IIO_ACCEL: u32 = 3;
/// Angular velocity (gyroscope).
pub const IIO_ANGL_VEL: u32 = 4;
/// Magnetic field.
pub const IIO_MAGN: u32 = 5;
/// Light intensity.
pub const IIO_LIGHT: u32 = 6;
/// Light intensity (IR).
pub const IIO_INTENSITY: u32 = 7;
/// Proximity.
pub const IIO_PROXIMITY: u32 = 8;
/// Temperature.
pub const IIO_TEMP: u32 = 9;
/// Inclinometer.
pub const IIO_INCLI: u32 = 10;
/// Rotation.
pub const IIO_ROT: u32 = 11;
/// Angle.
pub const IIO_ANGL: u32 = 12;
/// Timestamp.
pub const IIO_TIMESTAMP: u32 = 13;
/// Capacitance.
pub const IIO_CAPACITANCE: u32 = 14;
/// Altitude (pressure derived).
pub const IIO_ALTVOLTAGE: u32 = 15;
/// CCT (correlated color temperature).
pub const IIO_CCT: u32 = 16;
/// Pressure.
pub const IIO_PRESSURE: u32 = 17;
/// Humidity (relative).
pub const IIO_HUMIDITYRELATIVE: u32 = 18;
/// Activity.
pub const IIO_ACTIVITY: u32 = 19;
/// Steps.
pub const IIO_STEPS: u32 = 20;
/// Energy.
pub const IIO_ENERGY: u32 = 21;
/// Distance.
pub const IIO_DISTANCE: u32 = 22;
/// Velocity.
pub const IIO_VELOCITY: u32 = 23;
/// Concentration.
pub const IIO_CONCENTRATION: u32 = 24;
/// Resistance.
pub const IIO_RESISTANCE: u32 = 25;
/// PH.
pub const IIO_PH: u32 = 26;
/// UV index.
pub const IIO_UVINDEX: u32 = 27;
/// Electrical conductivity.
pub const IIO_ELECTRICALCONDUCTIVITY: u32 = 28;
/// Count.
pub const IIO_COUNT: u32 = 29;
/// Index.
pub const IIO_INDEX: u32 = 30;
/// Gravity.
pub const IIO_GRAVITY: u32 = 31;
/// Position/Displacement (flex).
pub const IIO_POSITIONRELATIVE: u32 = 32;
/// Phase.
pub const IIO_PHASE: u32 = 33;
/// Mass concentration.
pub const IIO_MASSCONCENTRATION: u32 = 34;

// ---------------------------------------------------------------------------
// IIO event types
// ---------------------------------------------------------------------------

/// Threshold event.
pub const IIO_EV_TYPE_THRESH: u32 = 0;
/// Magnitude event.
pub const IIO_EV_TYPE_MAG: u32 = 1;
/// Rate-of-change event.
pub const IIO_EV_TYPE_ROC: u32 = 2;
/// Threshold adaptive.
pub const IIO_EV_TYPE_THRESH_ADAPTIVE: u32 = 3;
/// Magnitude adaptive.
pub const IIO_EV_TYPE_MAG_ADAPTIVE: u32 = 4;
/// Change event.
pub const IIO_EV_TYPE_CHANGE: u32 = 5;

// ---------------------------------------------------------------------------
// IIO event directions
// ---------------------------------------------------------------------------

/// Either direction.
pub const IIO_EV_DIR_EITHER: u32 = 0;
/// Rising.
pub const IIO_EV_DIR_RISING: u32 = 1;
/// Falling.
pub const IIO_EV_DIR_FALLING: u32 = 2;
/// None/unspecified.
pub const IIO_EV_DIR_NONE: u32 = 3;

// ---------------------------------------------------------------------------
// IIO modifier types
// ---------------------------------------------------------------------------

/// No modifier.
pub const IIO_NO_MOD: u32 = 0;
/// X axis.
pub const IIO_MOD_X: u32 = 1;
/// Y axis.
pub const IIO_MOD_Y: u32 = 2;
/// Z axis.
pub const IIO_MOD_Z: u32 = 3;
/// X and Y axes combined.
pub const IIO_MOD_X_AND_Y: u32 = 4;
/// X and Z axes combined.
pub const IIO_MOD_X_AND_Z: u32 = 5;
/// Y and Z axes combined.
pub const IIO_MOD_Y_AND_Z: u32 = 6;
/// X, Y, and Z axes combined.
pub const IIO_MOD_X_AND_Y_AND_Z: u32 = 7;
/// Root sum squared.
pub const IIO_MOD_ROOT_SUM_SQUARED_X_Y: u32 = 10;
/// Light (clear).
pub const IIO_MOD_LIGHT_CLEAR: u32 = 14;
/// Light (red).
pub const IIO_MOD_LIGHT_RED: u32 = 15;
/// Light (green).
pub const IIO_MOD_LIGHT_GREEN: u32 = 16;
/// Light (blue).
pub const IIO_MOD_LIGHT_BLUE: u32 = 17;
/// Light (UV).
pub const IIO_MOD_LIGHT_UV: u32 = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chan_types_distinct() {
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
            IIO_INCLI,
            IIO_ROT,
            IIO_ANGL,
            IIO_TIMESTAMP,
            IIO_CAPACITANCE,
            IIO_ALTVOLTAGE,
            IIO_CCT,
            IIO_PRESSURE,
            IIO_HUMIDITYRELATIVE,
            IIO_ACTIVITY,
            IIO_STEPS,
            IIO_ENERGY,
            IIO_DISTANCE,
            IIO_VELOCITY,
            IIO_CONCENTRATION,
            IIO_RESISTANCE,
            IIO_PH,
            IIO_UVINDEX,
            IIO_ELECTRICALCONDUCTIVITY,
            IIO_COUNT,
            IIO_INDEX,
            IIO_GRAVITY,
            IIO_POSITIONRELATIVE,
            IIO_PHASE,
            IIO_MASSCONCENTRATION,
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
    fn test_event_dirs_distinct() {
        let dirs = [
            IIO_EV_DIR_EITHER,
            IIO_EV_DIR_RISING,
            IIO_EV_DIR_FALLING,
            IIO_EV_DIR_NONE,
        ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_axis_modifiers_distinct() {
        let mods = [
            IIO_NO_MOD,
            IIO_MOD_X,
            IIO_MOD_Y,
            IIO_MOD_Z,
            IIO_MOD_X_AND_Y,
            IIO_MOD_X_AND_Z,
            IIO_MOD_Y_AND_Z,
            IIO_MOD_X_AND_Y_AND_Z,
        ];
        for i in 0..mods.len() {
            for j in (i + 1)..mods.len() {
                assert_ne!(mods[i], mods[j]);
            }
        }
    }

    #[test]
    fn test_channel_type_values() {
        assert_eq!(IIO_VOLTAGE, 0);
        assert_eq!(IIO_ACCEL, 3);
        assert_eq!(IIO_TEMP, 9);
        assert_eq!(IIO_PRESSURE, 17);
    }

    #[test]
    fn test_light_modifiers_distinct() {
        let mods = [
            IIO_MOD_LIGHT_CLEAR,
            IIO_MOD_LIGHT_RED,
            IIO_MOD_LIGHT_GREEN,
            IIO_MOD_LIGHT_BLUE,
            IIO_MOD_LIGHT_UV,
        ];
        for i in 0..mods.len() {
            for j in (i + 1)..mods.len() {
                assert_ne!(mods[i], mods[j]);
            }
        }
    }
}
