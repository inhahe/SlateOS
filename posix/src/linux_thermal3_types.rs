//! `<linux/thermal.h>` — Additional thermal constants (part 3).
//!
//! Supplementary thermal constants covering genl commands,
//! trip types, and thermal event types.

// ---------------------------------------------------------------------------
// Thermal genl commands
// ---------------------------------------------------------------------------

/// Unspec.
pub const THERMAL_GENL_CMD_UNSPEC: u32 = 0;
/// Thermal zone get.
pub const THERMAL_GENL_CMD_TZ_GET_ID: u32 = 1;
/// Thermal zone get trip.
pub const THERMAL_GENL_CMD_TZ_GET_TRIP: u32 = 2;
/// Thermal zone get temperature.
pub const THERMAL_GENL_CMD_TZ_GET_TEMP: u32 = 3;
/// Thermal zone get governor.
pub const THERMAL_GENL_CMD_TZ_GET_GOV: u32 = 4;
/// Cooling device get.
pub const THERMAL_GENL_CMD_CDEV_GET: u32 = 5;

// ---------------------------------------------------------------------------
// Thermal trip types
// ---------------------------------------------------------------------------

/// Active trip.
pub const THERMAL_TRIP_ACTIVE: u32 = 0;
/// Passive trip.
pub const THERMAL_TRIP_PASSIVE: u32 = 1;
/// Hot trip.
pub const THERMAL_TRIP_HOT: u32 = 2;
/// Critical trip.
pub const THERMAL_TRIP_CRITICAL: u32 = 3;

// ---------------------------------------------------------------------------
// Thermal event types
// ---------------------------------------------------------------------------

/// Unspecified event.
pub const THERMAL_EVENT_UNSPECIFIED: u32 = 0;
/// Temperature threshold up.
pub const THERMAL_EVENT_TEMP_UP: u32 = 1;
/// Temperature threshold down.
pub const THERMAL_EVENT_TEMP_DOWN: u32 = 2;
/// Trip violated.
pub const THERMAL_EVENT_TRIP_VIOLATED: u32 = 3;
/// Trip changed.
pub const THERMAL_EVENT_TRIP_CHANGED: u32 = 4;
/// Keep alive.
pub const THERMAL_EVENT_KEEP_ALIVE: u32 = 5;

// ---------------------------------------------------------------------------
// Thermal trends
// ---------------------------------------------------------------------------

/// Raising temperature.
pub const THERMAL_TREND_RAISING: u32 = 0;
/// Dropping temperature.
pub const THERMAL_TREND_DROPPING: u32 = 1;
/// Stable temperature.
pub const THERMAL_TREND_STABLE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genl_cmds_distinct() {
        let cmds = [
            THERMAL_GENL_CMD_UNSPEC, THERMAL_GENL_CMD_TZ_GET_ID,
            THERMAL_GENL_CMD_TZ_GET_TRIP, THERMAL_GENL_CMD_TZ_GET_TEMP,
            THERMAL_GENL_CMD_TZ_GET_GOV, THERMAL_GENL_CMD_CDEV_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

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
    fn test_events_distinct() {
        let events = [
            THERMAL_EVENT_UNSPECIFIED, THERMAL_EVENT_TEMP_UP,
            THERMAL_EVENT_TEMP_DOWN, THERMAL_EVENT_TRIP_VIOLATED,
            THERMAL_EVENT_TRIP_CHANGED, THERMAL_EVENT_KEEP_ALIVE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
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
}
