//! `<linux/thermal.h>` — ACPI thermal zone constants.
//!
//! The thermal subsystem monitors temperature sensors and applies
//! cooling policies. ACPI defines trip points (passive, active,
//! critical, hot) and cooling device types (fan, processor throttle).
//! The governor selects cooling actions based on current temperature.

// ---------------------------------------------------------------------------
// Thermal trip point types
// ---------------------------------------------------------------------------

/// Active cooling trip point (fan on).
pub const THERMAL_TRIP_ACTIVE: u32 = 0;
/// Passive cooling trip point (CPU throttle).
pub const THERMAL_TRIP_PASSIVE: u32 = 1;
/// Hot trip point (emergency throttle).
pub const THERMAL_TRIP_HOT: u32 = 2;
/// Critical trip point (system shutdown).
pub const THERMAL_TRIP_CRITICAL: u32 = 3;

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
// Thermal zone device modes
// ---------------------------------------------------------------------------

/// Thermal zone is enabled (monitoring active).
pub const THERMAL_DEVICE_ENABLED: u32 = 0;
/// Thermal zone is disabled (no monitoring).
pub const THERMAL_DEVICE_DISABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Thermal governor types
// ---------------------------------------------------------------------------

/// Step-wise governor (gradual cooling adjustment).
pub const THERMAL_GOV_STEP_WISE: u32 = 0;
/// Fair-share governor (proportional cooling).
pub const THERMAL_GOV_FAIR_SHARE: u32 = 1;
/// Bang-bang governor (on/off binary control).
pub const THERMAL_GOV_BANG_BANG: u32 = 2;
/// User-space governor (delegate to userspace daemon).
pub const THERMAL_GOV_USER_SPACE: u32 = 3;
/// Power allocator governor (IPA for SoCs).
pub const THERMAL_GOV_POWER_ALLOCATOR: u32 = 4;

// ---------------------------------------------------------------------------
// Thermal events / notifications
// ---------------------------------------------------------------------------

/// Temperature crossed a trip point (going up).
pub const THERMAL_EVENT_TRIP_CROSSED_UP: u32 = 0;
/// Temperature crossed a trip point (going down).
pub const THERMAL_EVENT_TRIP_CROSSED_DOWN: u32 = 1;
/// Thermal zone created.
pub const THERMAL_EVENT_ZONE_CREATED: u32 = 2;
/// Thermal zone removed.
pub const THERMAL_EVENT_ZONE_REMOVED: u32 = 3;

// ---------------------------------------------------------------------------
// Temperature constants (millidegrees Celsius)
// ---------------------------------------------------------------------------

/// Absolute zero in millidegrees Celsius (invalid/unset marker).
pub const THERMAL_TEMP_INVALID: i32 = -274_000;

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
    fn test_trends_distinct() {
        assert_ne!(THERMAL_TREND_RAISING, THERMAL_TREND_DROPPING);
        assert_ne!(THERMAL_TREND_DROPPING, THERMAL_TREND_STABLE);
        assert_ne!(THERMAL_TREND_RAISING, THERMAL_TREND_STABLE);
    }

    #[test]
    fn test_device_modes() {
        assert_ne!(THERMAL_DEVICE_ENABLED, THERMAL_DEVICE_DISABLED);
    }

    #[test]
    fn test_governor_types_distinct() {
        let govs = [
            THERMAL_GOV_STEP_WISE, THERMAL_GOV_FAIR_SHARE,
            THERMAL_GOV_BANG_BANG, THERMAL_GOV_USER_SPACE,
            THERMAL_GOV_POWER_ALLOCATOR,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let evts = [
            THERMAL_EVENT_TRIP_CROSSED_UP, THERMAL_EVENT_TRIP_CROSSED_DOWN,
            THERMAL_EVENT_ZONE_CREATED, THERMAL_EVENT_ZONE_REMOVED,
        ];
        for i in 0..evts.len() {
            for j in (i + 1)..evts.len() {
                assert_ne!(evts[i], evts[j]);
            }
        }
    }

    #[test]
    fn test_temp_invalid() {
        assert!(THERMAL_TEMP_INVALID < 0);
    }
}
