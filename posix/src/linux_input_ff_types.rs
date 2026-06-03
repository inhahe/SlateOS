//! `<linux/input.h>` (force-feedback subset) — haptic effect type codes.
//!
//! Force feedback (FF) allows devices like game controllers, steering
//! wheels, and joysticks to produce physical sensations — rumble,
//! resistance, vibration patterns, etc. The kernel's FF subsystem
//! provides a uniform API: userspace uploads effect descriptors and
//! triggers playback via `EV_FF` events.

// ---------------------------------------------------------------------------
// Force-feedback effect types
// ---------------------------------------------------------------------------

/// Rumble effect (simple vibration with strong/weak motors).
pub const FF_RUMBLE: u16 = 0x50;
/// Periodic effect (sine, square, triangle, sawtooth waves).
pub const FF_PERIODIC: u16 = 0x51;
/// Constant force in one direction.
pub const FF_CONSTANT: u16 = 0x52;
/// Spring effect (position-dependent force).
pub const FF_SPRING: u16 = 0x53;
/// Friction effect (velocity-dependent damping).
pub const FF_FRICTION: u16 = 0x54;
/// Damper effect (velocity-dependent resistance).
pub const FF_DAMPER: u16 = 0x55;
/// Inertia effect (acceleration-dependent force).
pub const FF_INERTIA: u16 = 0x56;
/// Ramp effect (linearly changing force).
pub const FF_RAMP: u16 = 0x57;

// ---------------------------------------------------------------------------
// Periodic waveform types
// ---------------------------------------------------------------------------

/// Square wave.
pub const FF_SQUARE: u16 = 0x58;
/// Triangle wave.
pub const FF_TRIANGLE: u16 = 0x59;
/// Sine wave.
pub const FF_SINE: u16 = 0x5A;
/// Sawtooth up wave.
pub const FF_SAW_UP: u16 = 0x5B;
/// Sawtooth down wave.
pub const FF_SAW_DOWN: u16 = 0x5C;
/// Custom waveform (user-supplied samples).
pub const FF_CUSTOM: u16 = 0x5D;

// ---------------------------------------------------------------------------
// Control codes
// ---------------------------------------------------------------------------

/// Set overall device gain (0–0xFFFF).
pub const FF_GAIN: u16 = 0x60;
/// Set device auto-centre spring strength.
pub const FF_AUTOCENTER: u16 = 0x61;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum FF code.
pub const FF_MAX: u16 = 0x7F;
/// Number of FF codes.
pub const FF_CNT: u16 = 0x80;
/// Maximum simultaneous effects (common device limit).
pub const FF_MAX_EFFECTS: u16 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_types_distinct() {
        let effects = [
            FF_RUMBLE,
            FF_PERIODIC,
            FF_CONSTANT,
            FF_SPRING,
            FF_FRICTION,
            FF_DAMPER,
            FF_INERTIA,
            FF_RAMP,
        ];
        for i in 0..effects.len() {
            for j in (i + 1)..effects.len() {
                assert_ne!(
                    effects[i], effects[j],
                    "effect types {} and {} collide",
                    i, j
                );
            }
        }
    }

    #[test]
    fn test_waveforms_distinct() {
        let waves = [
            FF_SQUARE,
            FF_TRIANGLE,
            FF_SINE,
            FF_SAW_UP,
            FF_SAW_DOWN,
            FF_CUSTOM,
        ];
        for i in 0..waves.len() {
            for j in (i + 1)..waves.len() {
                assert_ne!(waves[i], waves[j], "waveforms {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_effects_sequential() {
        assert_eq!(FF_PERIODIC, FF_RUMBLE + 1);
        assert_eq!(FF_CONSTANT, FF_PERIODIC + 1);
        assert_eq!(FF_SPRING, FF_CONSTANT + 1);
        assert_eq!(FF_FRICTION, FF_SPRING + 1);
        assert_eq!(FF_DAMPER, FF_FRICTION + 1);
        assert_eq!(FF_INERTIA, FF_DAMPER + 1);
        assert_eq!(FF_RAMP, FF_INERTIA + 1);
    }

    #[test]
    fn test_waveforms_sequential() {
        assert_eq!(FF_TRIANGLE, FF_SQUARE + 1);
        assert_eq!(FF_SINE, FF_TRIANGLE + 1);
        assert_eq!(FF_SAW_UP, FF_SINE + 1);
        assert_eq!(FF_SAW_DOWN, FF_SAW_UP + 1);
        assert_eq!(FF_CUSTOM, FF_SAW_DOWN + 1);
    }

    #[test]
    fn test_control_codes() {
        assert_ne!(FF_GAIN, FF_AUTOCENTER);
        assert!(FF_GAIN > FF_CUSTOM);
        assert!(FF_AUTOCENTER > FF_GAIN);
    }

    #[test]
    fn test_all_within_max() {
        let all = [
            FF_RUMBLE,
            FF_PERIODIC,
            FF_CONSTANT,
            FF_SPRING,
            FF_FRICTION,
            FF_DAMPER,
            FF_INERTIA,
            FF_RAMP,
            FF_SQUARE,
            FF_TRIANGLE,
            FF_SINE,
            FF_SAW_UP,
            FF_SAW_DOWN,
            FF_CUSTOM,
            FF_GAIN,
            FF_AUTOCENTER,
        ];
        for &f in &all {
            assert!(f <= FF_MAX, "FF code 0x{:02X} exceeds FF_MAX", f);
        }
    }

    #[test]
    fn test_ff_cnt() {
        assert_eq!(FF_CNT, FF_MAX + 1);
    }
}
