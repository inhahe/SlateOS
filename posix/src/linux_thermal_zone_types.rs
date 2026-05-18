//! `<linux/thermal.h>` — Thermal zone and trip point constants.
//!
//! The thermal framework monitors hardware temperatures and applies
//! cooling policies when trip points are reached. These constants
//! define trip types, thermal zone states, cooling device types,
//! and governor modes.

// ---------------------------------------------------------------------------
// Thermal trip point types
// ---------------------------------------------------------------------------

/// Active cooling trip (fan speeds up).
pub const THERMAL_TRIP_ACTIVE: u32 = 0;
/// Passive cooling trip (throttle CPU).
pub const THERMAL_TRIP_PASSIVE: u32 = 1;
/// Hot trip (warning threshold).
pub const THERMAL_TRIP_HOT: u32 = 2;
/// Critical trip (emergency shutdown).
pub const THERMAL_TRIP_CRITICAL: u32 = 3;

// ---------------------------------------------------------------------------
// Thermal zone device states
// ---------------------------------------------------------------------------

/// Zone is enabled and monitoring.
pub const THERMAL_DEVICE_ENABLED: u32 = 0;
/// Zone is disabled (not monitoring).
pub const THERMAL_DEVICE_DISABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Thermal trends
// ---------------------------------------------------------------------------

/// Temperature is rising.
pub const THERMAL_TREND_RAISING: u32 = 0;
/// Temperature is dropping.
pub const THERMAL_TREND_DROPPING: u32 = 1;
/// Temperature is stable.
pub const THERMAL_TREND_STABLE: u32 = 2;

// ---------------------------------------------------------------------------
// Cooling device types
// ---------------------------------------------------------------------------

/// Processor cooling (frequency throttling).
pub const THERMAL_COOLING_PROCESSOR: u32 = 0;
/// Fan cooling device.
pub const THERMAL_COOLING_FAN: u32 = 1;
/// Power capping device.
pub const THERMAL_COOLING_POWER: u32 = 2;

// ---------------------------------------------------------------------------
// Thermal event notifications
// ---------------------------------------------------------------------------

/// Temperature crossed a trip point (going up).
pub const THERMAL_EVENT_TRIP_CROSSED: u32 = 0;
/// Temperature changed significantly.
pub const THERMAL_EVENT_TEMP_SAMPLE: u32 = 1;
/// Cooling device state changed.
pub const THERMAL_EVENT_CDEV_UPDATE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trip_types_distinct() {
        let trips = [
            THERMAL_TRIP_ACTIVE, THERMAL_TRIP_PASSIVE,
            THERMAL_TRIP_HOT, THERMAL_TRIP_CRITICAL,
        ];
        for i in 0..trips.len() {
            for j in (i + 1)..trips.len() {
                assert_ne!(trips[i], trips[j]);
            }
        }
    }

    #[test]
    fn test_device_states() {
        assert_eq!(THERMAL_DEVICE_ENABLED, 0);
        assert_eq!(THERMAL_DEVICE_DISABLED, 1);
    }

    #[test]
    fn test_trends_distinct() {
        let trends = [
            THERMAL_TREND_RAISING, THERMAL_TREND_DROPPING,
            THERMAL_TREND_STABLE,
        ];
        for i in 0..trends.len() {
            for j in (i + 1)..trends.len() {
                assert_ne!(trends[i], trends[j]);
            }
        }
    }

    #[test]
    fn test_cooling_types_distinct() {
        let types = [
            THERMAL_COOLING_PROCESSOR, THERMAL_COOLING_FAN,
            THERMAL_COOLING_POWER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            THERMAL_EVENT_TRIP_CROSSED, THERMAL_EVENT_TEMP_SAMPLE,
            THERMAL_EVENT_CDEV_UPDATE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
