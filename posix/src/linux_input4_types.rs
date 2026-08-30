//! `<linux/input.h>` — Additional input constants (part 4).
//!
//! Supplementary input constants covering force feedback
//! types, effect status, and input properties.

// ---------------------------------------------------------------------------
// Force feedback effect types
// ---------------------------------------------------------------------------

/// Rumble effect.
pub const FF_RUMBLE: u16 = 0x50;
/// Periodic effect.
pub const FF_PERIODIC: u16 = 0x51;
/// Constant force.
pub const FF_CONSTANT: u16 = 0x52;
/// Spring effect.
pub const FF_SPRING: u16 = 0x53;
/// Friction effect.
pub const FF_FRICTION: u16 = 0x54;
/// Damper effect.
pub const FF_DAMPER: u16 = 0x55;
/// Inertia effect.
pub const FF_INERTIA: u16 = 0x56;
/// Ramp effect.
pub const FF_RAMP: u16 = 0x57;

// ---------------------------------------------------------------------------
// Force feedback periodic waveforms
// ---------------------------------------------------------------------------

/// Square wave.
pub const FF_SQUARE: u16 = 0x58;
/// Triangle wave.
pub const FF_TRIANGLE: u16 = 0x59;
/// Sine wave.
pub const FF_SINE: u16 = 0x5A;
/// Sawtooth up.
pub const FF_SAW_UP: u16 = 0x5B;
/// Sawtooth down.
pub const FF_SAW_DOWN: u16 = 0x5C;
/// Custom waveform.
pub const FF_CUSTOM: u16 = 0x5D;

// ---------------------------------------------------------------------------
// Input device properties
// ---------------------------------------------------------------------------

/// Direct input device (touchscreen).
pub const INPUT_PROP_DIRECT: u32 = 0x01;
/// Pointer device (trackpad, mouse).
pub const INPUT_PROP_POINTER: u32 = 0x00;
/// Button pad (no separate buttons).
pub const INPUT_PROP_BUTTONPAD: u32 = 0x02;
/// Semi-multi-touch.
pub const INPUT_PROP_SEMI_MT: u32 = 0x03;
/// Top button pad.
pub const INPUT_PROP_TOPBUTTONPAD: u32 = 0x04;
/// Pointing stick.
pub const INPUT_PROP_POINTING_STICK: u32 = 0x05;
/// Accelerometer.
pub const INPUT_PROP_ACCELEROMETER: u32 = 0x06;

// ---------------------------------------------------------------------------
// Input event codes — misc events
// ---------------------------------------------------------------------------

/// Serial number.
pub const MSC_SERIAL: u32 = 0x00;
/// Pulse per second.
pub const MSC_PULSELED: u32 = 0x01;
/// Gesture.
pub const MSC_GESTURE: u32 = 0x02;
/// Raw data.
pub const MSC_RAW: u32 = 0x03;
/// Scan code.
pub const MSC_SCAN: u32 = 0x04;
/// Timestamp.
pub const MSC_TIMESTAMP: u32 = 0x05;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ff_types_distinct() {
        let types = [
            FF_RUMBLE,
            FF_PERIODIC,
            FF_CONSTANT,
            FF_SPRING,
            FF_FRICTION,
            FF_DAMPER,
            FF_INERTIA,
            FF_RAMP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ff_waveforms_distinct() {
        let waveforms = [
            FF_SQUARE,
            FF_TRIANGLE,
            FF_SINE,
            FF_SAW_UP,
            FF_SAW_DOWN,
            FF_CUSTOM,
        ];
        for i in 0..waveforms.len() {
            for j in (i + 1)..waveforms.len() {
                assert_ne!(waveforms[i], waveforms[j]);
            }
        }
    }

    #[test]
    fn test_input_props_distinct() {
        let props = [
            INPUT_PROP_POINTER,
            INPUT_PROP_DIRECT,
            INPUT_PROP_BUTTONPAD,
            INPUT_PROP_SEMI_MT,
            INPUT_PROP_TOPBUTTONPAD,
            INPUT_PROP_POINTING_STICK,
            INPUT_PROP_ACCELEROMETER,
        ];
        for i in 0..props.len() {
            for j in (i + 1)..props.len() {
                assert_ne!(props[i], props[j]);
            }
        }
    }

    #[test]
    fn test_msc_events_distinct() {
        let events = [
            MSC_SERIAL,
            MSC_PULSELED,
            MSC_GESTURE,
            MSC_RAW,
            MSC_SCAN,
            MSC_TIMESTAMP,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
