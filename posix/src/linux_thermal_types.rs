//! `<linux/thermal.h>` — Thermal management subsystem constants.
//!
//! The Linux thermal framework monitors temperature sensors and
//! applies cooling policies (throttling CPUs, adjusting fan speeds,
//! shutting down). It links thermal zones (sensors) with cooling
//! devices via governors that decide cooling actions.

// ---------------------------------------------------------------------------
// Thermal zone trip types
// ---------------------------------------------------------------------------

/// Active trip (start cooling device).
pub const THERMAL_TRIP_ACTIVE: u8 = 0;
/// Passive trip (throttle, reduce performance).
pub const THERMAL_TRIP_PASSIVE: u8 = 1;
/// Hot trip (warning).
pub const THERMAL_TRIP_HOT: u8 = 2;
/// Critical trip (emergency shutdown).
pub const THERMAL_TRIP_CRITICAL: u8 = 3;

// ---------------------------------------------------------------------------
// Thermal trend
// ---------------------------------------------------------------------------

/// Temperature is raising.
pub const THERMAL_TREND_RAISING: u8 = 0;
/// Temperature is dropping.
pub const THERMAL_TREND_DROPPING: u8 = 1;
/// Temperature is stable.
pub const THERMAL_TREND_STABLE: u8 = 2;

// ---------------------------------------------------------------------------
// Governor types (well-known names)
// ---------------------------------------------------------------------------

/// Step-wise governor.
pub const THERMAL_GOV_STEP_WISE: &str = "step_wise";
/// Fair-share governor.
pub const THERMAL_GOV_FAIR_SHARE: &str = "fair_share";
/// Power allocator governor.
pub const THERMAL_GOV_POWER_ALLOCATOR: &str = "power_allocator";
/// User-space governor.
pub const THERMAL_GOV_USER_SPACE: &str = "user_space";
/// Bang-bang governor.
pub const THERMAL_GOV_BANG_BANG: &str = "bang_bang";

// ---------------------------------------------------------------------------
// Cooling device types
// ---------------------------------------------------------------------------

/// CPU frequency cooling.
pub const THERMAL_COOLING_CPUFREQ: u8 = 0;
/// Fan cooling.
pub const THERMAL_COOLING_FAN: u8 = 1;
/// GPU frequency cooling.
pub const THERMAL_COOLING_GPUFREQ: u8 = 2;
/// Device power cooling.
pub const THERMAL_COOLING_DEVFREQ: u8 = 3;

// ---------------------------------------------------------------------------
// Temperature constants
// ---------------------------------------------------------------------------

/// Invalid/uninitialized temperature.
pub const THERMAL_TEMP_INVALID: i32 = -274000;
/// Temperature unit (millidegrees Celsius).
pub const THERMAL_TEMP_UNIT: &str = "millidegrees C";

// ---------------------------------------------------------------------------
// Thermal netlink event types
// ---------------------------------------------------------------------------

/// Temperature threshold crossed (up).
pub const THERMAL_EVENT_THRESHOLD_UP: u32 = 0;
/// Temperature threshold crossed (down).
pub const THERMAL_EVENT_THRESHOLD_DOWN: u32 = 1;
/// Critical temperature event.
pub const THERMAL_EVENT_CRITICAL: u32 = 2;
/// Device power limit changed.
pub const THERMAL_EVENT_POWER_LIMIT: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trip_types_distinct() {
        let types = [
            THERMAL_TRIP_ACTIVE, THERMAL_TRIP_PASSIVE,
            THERMAL_TRIP_HOT, THERMAL_TRIP_CRITICAL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_trend_values_distinct() {
        let trends = [THERMAL_TREND_RAISING, THERMAL_TREND_DROPPING, THERMAL_TREND_STABLE];
        for i in 0..trends.len() {
            for j in (i + 1)..trends.len() {
                assert_ne!(trends[i], trends[j]);
            }
        }
    }

    #[test]
    fn test_governor_names_distinct() {
        let govs = [
            THERMAL_GOV_STEP_WISE, THERMAL_GOV_FAIR_SHARE,
            THERMAL_GOV_POWER_ALLOCATOR, THERMAL_GOV_USER_SPACE,
            THERMAL_GOV_BANG_BANG,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
        }
    }

    #[test]
    fn test_cooling_types_distinct() {
        let types = [
            THERMAL_COOLING_CPUFREQ, THERMAL_COOLING_FAN,
            THERMAL_COOLING_GPUFREQ, THERMAL_COOLING_DEVFREQ,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_invalid_temp() {
        assert!(THERMAL_TEMP_INVALID < 0);
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            THERMAL_EVENT_THRESHOLD_UP, THERMAL_EVENT_THRESHOLD_DOWN,
            THERMAL_EVENT_CRITICAL, THERMAL_EVENT_POWER_LIMIT,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
