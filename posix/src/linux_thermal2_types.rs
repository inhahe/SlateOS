//! `<linux/thermal.h>` (additional) — Thermal management extended constants.
//!
//! This module covers additional thermal framework constants beyond
//! basic zone management: governor algorithms (how to respond to
//! temperature changes), cooling device types, trip point characteristics,
//! and thermal emergency handling. The thermal framework continuously
//! monitors temperature sensors and activates cooling measures (fan
//! speed, CPU frequency reduction, GPU throttling) as needed.

// ---------------------------------------------------------------------------
// Thermal governors
// ---------------------------------------------------------------------------

/// Step-wise governor (reduce cooling level step by step).
pub const THERMAL_GOV_STEP_WISE: u32 = 0;
/// Fair-share governor (distribute cooling proportionally).
pub const THERMAL_GOV_FAIR_SHARE: u32 = 1;
/// Power-allocator governor (IPA, Intel DPTF style).
pub const THERMAL_GOV_POWER_ALLOCATOR: u32 = 2;
/// Bang-bang governor (on/off, no proportional control).
pub const THERMAL_GOV_BANG_BANG: u32 = 3;
/// User-space governor (delegate decisions to userspace).
pub const THERMAL_GOV_USER_SPACE: u32 = 4;

// ---------------------------------------------------------------------------
// Trip point types
// ---------------------------------------------------------------------------

/// Active trip (turn on active cooling, e.g., fan).
pub const THERMAL_TRIP_ACTIVE: u32 = 0;
/// Passive trip (reduce performance to lower temperature).
pub const THERMAL_TRIP_PASSIVE: u32 = 1;
/// Hot trip (warning, log event).
pub const THERMAL_TRIP_HOT: u32 = 2;
/// Critical trip (emergency shutdown to prevent damage).
pub const THERMAL_TRIP_CRITICAL: u32 = 3;

// ---------------------------------------------------------------------------
// Thermal trends
// ---------------------------------------------------------------------------

/// Temperature is rising.
pub const THERMAL_TREND_RAISING: u32 = 0;
/// Temperature is stable.
pub const THERMAL_TREND_STABLE: u32 = 1;
/// Temperature is dropping.
pub const THERMAL_TREND_DROPPING: u32 = 2;

// ---------------------------------------------------------------------------
// Thermal notification events
// ---------------------------------------------------------------------------

/// Temperature crossed a trip point (going up).
pub const THERMAL_EVENT_TRIP_UP: u32 = 0;
/// Temperature crossed a trip point (going down).
pub const THERMAL_EVENT_TRIP_DOWN: u32 = 1;
/// Thermal zone was created.
pub const THERMAL_EVENT_ZONE_ADDED: u32 = 2;
/// Thermal zone was removed.
pub const THERMAL_EVENT_ZONE_REMOVED: u32 = 3;
/// Cooling device state changed.
pub const THERMAL_EVENT_COOLING_CHANGED: u32 = 4;

// ---------------------------------------------------------------------------
// Temperature constants (millidegrees Celsius)
// ---------------------------------------------------------------------------

/// Invalid/unset temperature reading.
pub const THERMAL_TEMP_INVALID: i32 = -274000;
/// Maximum representable temperature.
pub const THERMAL_TEMP_MAX: i32 = 200000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governors_distinct() {
        let govs = [
            THERMAL_GOV_STEP_WISE,
            THERMAL_GOV_FAIR_SHARE,
            THERMAL_GOV_POWER_ALLOCATOR,
            THERMAL_GOV_BANG_BANG,
            THERMAL_GOV_USER_SPACE,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
        }
    }

    #[test]
    fn test_trip_types_distinct() {
        let trips = [
            THERMAL_TRIP_ACTIVE,
            THERMAL_TRIP_PASSIVE,
            THERMAL_TRIP_HOT,
            THERMAL_TRIP_CRITICAL,
        ];
        for i in 0..trips.len() {
            for j in (i + 1)..trips.len() {
                assert_ne!(trips[i], trips[j]);
            }
        }
    }

    #[test]
    fn test_trends_distinct() {
        let trends = [
            THERMAL_TREND_RAISING,
            THERMAL_TREND_STABLE,
            THERMAL_TREND_DROPPING,
        ];
        for i in 0..trends.len() {
            for j in (i + 1)..trends.len() {
                assert_ne!(trends[i], trends[j]);
            }
        }
    }

    #[test]
    fn test_temp_constants() {
        assert!(THERMAL_TEMP_INVALID < 0);
        assert!(THERMAL_TEMP_MAX > 0);
    }
}
