//! `<linux/iio/events.h>` — Industrial I/O event constants.
//!
//! IIO (Industrial I/O) events notify userspace when sensor readings
//! cross configured thresholds. Events are delivered via a character
//! device (/dev/iio:deviceN/events) and include the channel, event
//! type, direction, and timestamp. Used by proximity sensors, light
//! sensors, accelerometers, and other analog/digital converters.

// ---------------------------------------------------------------------------
// IIO event types
// ---------------------------------------------------------------------------

/// Threshold crossed (value exceeded/fell below a limit).
pub const IIO_EV_TYPE_THRESH: u32 = 0;
/// Magnitude threshold (absolute value exceeded).
pub const IIO_EV_TYPE_MAG: u32 = 1;
/// Region-of-change (value entered/left a region).
pub const IIO_EV_TYPE_ROC: u32 = 2;
/// Adaptive threshold (auto-adjusting).
pub const IIO_EV_TYPE_THRESH_ADAPTIVE: u32 = 3;
/// Magnitude adaptive threshold.
pub const IIO_EV_TYPE_MAG_ADAPTIVE: u32 = 4;
/// Change event (any value change).
pub const IIO_EV_TYPE_CHANGE: u32 = 5;

// ---------------------------------------------------------------------------
// IIO event directions
// ---------------------------------------------------------------------------

/// Either direction (rising or falling).
pub const IIO_EV_DIR_EITHER: u32 = 0;
/// Rising direction (value increasing).
pub const IIO_EV_DIR_RISING: u32 = 1;
/// Falling direction (value decreasing).
pub const IIO_EV_DIR_FALLING: u32 = 2;
/// No direction (undirected event).
pub const IIO_EV_DIR_NONE: u32 = 3;

// ---------------------------------------------------------------------------
// IIO channel types (subset, for event context)
// ---------------------------------------------------------------------------

/// Voltage channel.
pub const IIO_VOLTAGE: u32 = 0;
/// Current channel.
pub const IIO_CURRENT: u32 = 1;
/// Temperature channel.
pub const IIO_TEMP: u32 = 9;
/// Proximity channel.
pub const IIO_PROXIMITY: u32 = 12;
/// Light intensity channel.
pub const IIO_LIGHT: u32 = 5;
/// Acceleration channel.
pub const IIO_ACCEL: u32 = 3;
/// Angular velocity channel (gyroscope).
pub const IIO_ANGL_VEL: u32 = 4;
/// Pressure channel.
pub const IIO_PRESSURE: u32 = 13;
/// Humidity channel.
pub const IIO_HUMIDITYRELATIVE: u32 = 14;

// ---------------------------------------------------------------------------
// Event info encoding (64-bit event word layout)
// ---------------------------------------------------------------------------

/// Shift for channel type in event word.
pub const IIO_EV_CHAN_TYPE_SHIFT: u32 = 0;
/// Shift for modifier in event word.
pub const IIO_EV_MOD_SHIFT: u32 = 8;
/// Shift for event type in event word.
pub const IIO_EV_TYPE_SHIFT: u32 = 56;
/// Shift for direction in event word.
pub const IIO_EV_DIR_SHIFT: u32 = 48;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let types = [
            IIO_EV_TYPE_THRESH, IIO_EV_TYPE_MAG, IIO_EV_TYPE_ROC,
            IIO_EV_TYPE_THRESH_ADAPTIVE, IIO_EV_TYPE_MAG_ADAPTIVE,
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
        let dirs = [
            IIO_EV_DIR_EITHER, IIO_EV_DIR_RISING,
            IIO_EV_DIR_FALLING, IIO_EV_DIR_NONE,
        ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_channel_types_distinct() {
        let chans = [
            IIO_VOLTAGE, IIO_CURRENT, IIO_ACCEL, IIO_ANGL_VEL,
            IIO_LIGHT, IIO_TEMP, IIO_PROXIMITY, IIO_PRESSURE,
            IIO_HUMIDITYRELATIVE,
        ];
        for i in 0..chans.len() {
            for j in (i + 1)..chans.len() {
                assert_ne!(chans[i], chans[j]);
            }
        }
    }

    #[test]
    fn test_shifts_non_overlapping() {
        assert_ne!(IIO_EV_CHAN_TYPE_SHIFT, IIO_EV_MOD_SHIFT);
        assert_ne!(IIO_EV_TYPE_SHIFT, IIO_EV_DIR_SHIFT);
        assert!(IIO_EV_DIR_SHIFT < IIO_EV_TYPE_SHIFT);
    }
}
