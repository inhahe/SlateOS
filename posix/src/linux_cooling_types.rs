//! `<linux/thermal.h>` (cooling subset) — Cooling device constants.
//!
//! Cooling devices reduce system temperature by either reducing heat
//! generation (passive cooling: CPU/GPU frequency reduction) or
//! increasing heat dissipation (active cooling: fans). Each cooling
//! device has a state range (0 = maximum cooling, max_state = minimum
//! cooling or off). The thermal framework's governor selects the
//! appropriate cooling state based on current temperature and trip
//! points.

// ---------------------------------------------------------------------------
// Cooling device types
// ---------------------------------------------------------------------------

/// Processor cooling (frequency scaling / throttling).
pub const COOLING_TYPE_PROCESSOR: u32 = 0;
/// Fan cooling (speed control).
pub const COOLING_TYPE_FAN: u32 = 1;
/// GPU cooling (frequency/power cap).
pub const COOLING_TYPE_GPU: u32 = 2;
/// Memory bandwidth cooling (throttle memory bus).
pub const COOLING_TYPE_MEMORY: u32 = 3;
/// Device power limit (generic power cap).
pub const COOLING_TYPE_POWER: u32 = 4;
/// Display backlight dimming.
pub const COOLING_TYPE_BACKLIGHT: u32 = 5;

// ---------------------------------------------------------------------------
// Cooling device state
// ---------------------------------------------------------------------------

/// Maximum cooling (lowest temperature, highest fan speed).
pub const COOLING_STATE_MAX: u32 = 0;
/// No cooling (highest temperature, fan off).
pub const COOLING_STATE_NONE: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Fan speed modes
// ---------------------------------------------------------------------------

/// Fan is off.
pub const FAN_STATE_OFF: u32 = 0;
/// Fan is at low speed.
pub const FAN_STATE_LOW: u32 = 1;
/// Fan is at medium speed.
pub const FAN_STATE_MEDIUM: u32 = 2;
/// Fan is at high speed.
pub const FAN_STATE_HIGH: u32 = 3;
/// Fan is at full speed (maximum cooling).
pub const FAN_STATE_FULL: u32 = 4;

// ---------------------------------------------------------------------------
// Processor throttle levels
// ---------------------------------------------------------------------------

/// No throttling (full performance).
pub const CPU_THROTTLE_NONE: u32 = 0;
/// Light throttling (reduce 1-2 P-states).
pub const CPU_THROTTLE_LIGHT: u32 = 1;
/// Medium throttling (half performance).
pub const CPU_THROTTLE_MEDIUM: u32 = 2;
/// Heavy throttling (minimum performance).
pub const CPU_THROTTLE_HEAVY: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cooling_types_distinct() {
        let types = [
            COOLING_TYPE_PROCESSOR,
            COOLING_TYPE_FAN,
            COOLING_TYPE_GPU,
            COOLING_TYPE_MEMORY,
            COOLING_TYPE_POWER,
            COOLING_TYPE_BACKLIGHT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_fan_states_ordered() {
        assert!(FAN_STATE_OFF < FAN_STATE_LOW);
        assert!(FAN_STATE_LOW < FAN_STATE_MEDIUM);
        assert!(FAN_STATE_MEDIUM < FAN_STATE_HIGH);
        assert!(FAN_STATE_HIGH < FAN_STATE_FULL);
    }

    #[test]
    fn test_throttle_levels_ordered() {
        assert!(CPU_THROTTLE_NONE < CPU_THROTTLE_LIGHT);
        assert!(CPU_THROTTLE_LIGHT < CPU_THROTTLE_MEDIUM);
        assert!(CPU_THROTTLE_MEDIUM < CPU_THROTTLE_HEAVY);
    }

    #[test]
    fn test_cooling_state_extremes() {
        assert_eq!(COOLING_STATE_MAX, 0);
        assert_ne!(COOLING_STATE_MAX, COOLING_STATE_NONE);
    }
}
