//! `<linux/thermal.h>` — Thermal management constants.
//!
//! The Linux thermal framework monitors CPU/GPU/SoC temperatures and
//! takes actions (throttling, fan control, shutdown) to prevent
//! overheating. Userspace reads thermal zones via sysfs and receives
//! events via Generic Netlink.

// ---------------------------------------------------------------------------
// Thermal zone types (Generic Netlink event types)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const THERMAL_GENL_CMD_UNSPEC: u8 = 0;
/// Temperature changed event.
pub const THERMAL_GENL_CMD_TZ_GET_TEMP: u8 = 1;
/// Trip point crossed event.
pub const THERMAL_GENL_CMD_TZ_GET_TRIP: u8 = 2;
/// Cooling device state query.
pub const THERMAL_GENL_CMD_CDEV_GET: u8 = 3;
/// Zone created notification.
pub const THERMAL_GENL_EVENT_TZ_CREATE: u8 = 4;
/// Zone deleted notification.
pub const THERMAL_GENL_EVENT_TZ_DELETE: u8 = 5;
/// Zone disabled notification.
pub const THERMAL_GENL_EVENT_TZ_DISABLE: u8 = 6;
/// Zone enabled notification.
pub const THERMAL_GENL_EVENT_TZ_ENABLE: u8 = 7;
/// Trip point changed.
pub const THERMAL_GENL_EVENT_TZ_TRIP_CHANGE: u8 = 8;
/// Trip point crossed up.
pub const THERMAL_GENL_EVENT_TZ_TRIP_UP: u8 = 9;
/// Trip point crossed down.
pub const THERMAL_GENL_EVENT_TZ_TRIP_DOWN: u8 = 10;
/// Cooling device add.
pub const THERMAL_GENL_EVENT_CDEV_ADD: u8 = 11;
/// Cooling device delete.
pub const THERMAL_GENL_EVENT_CDEV_DELETE: u8 = 12;
/// Cooling device state update.
pub const THERMAL_GENL_EVENT_CDEV_STATE_UPDATE: u8 = 13;

// ---------------------------------------------------------------------------
// Thermal trip types
// ---------------------------------------------------------------------------

/// Active cooling trip point (fan speed increase).
pub const THERMAL_TRIP_ACTIVE: u32 = 0;
/// Passive cooling trip point (throttling).
pub const THERMAL_TRIP_PASSIVE: u32 = 1;
/// Hot trip point (critical warning).
pub const THERMAL_TRIP_HOT: u32 = 2;
/// Critical trip point (emergency shutdown).
pub const THERMAL_TRIP_CRITICAL: u32 = 3;

// ---------------------------------------------------------------------------
// Thermal trends
// ---------------------------------------------------------------------------

/// Temperature rising.
pub const THERMAL_TREND_RAISING: u32 = 0;
/// Temperature dropping.
pub const THERMAL_TREND_DROPPING: u32 = 1;
/// Temperature stable.
pub const THERMAL_TREND_STABLE: u32 = 2;

// ---------------------------------------------------------------------------
// Thermal Generic Netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const THERMAL_GENL_ATTR_TZ_ID: u16 = 1;
/// Zone name.
pub const THERMAL_GENL_ATTR_TZ_NAME: u16 = 2;
/// Current temperature (millidegrees C).
pub const THERMAL_GENL_ATTR_TZ_TEMP: u16 = 3;
/// Trip point index.
pub const THERMAL_GENL_ATTR_TZ_TRIP_ID: u16 = 4;
/// Trip point temperature.
pub const THERMAL_GENL_ATTR_TZ_TRIP_TEMP: u16 = 5;
/// Trip point type.
pub const THERMAL_GENL_ATTR_TZ_TRIP_TYPE: u16 = 6;
/// Cooling device ID.
pub const THERMAL_GENL_ATTR_TZ_CDEV_ID: u16 = 7;
/// Cooling device maximum state.
pub const THERMAL_GENL_ATTR_TZ_CDEV_MAX_STATE: u16 = 8;
/// Cooling device current state.
pub const THERMAL_GENL_ATTR_TZ_CDEV_CUR_STATE: u16 = 9;
/// Governor name.
pub const THERMAL_GENL_ATTR_TZ_GOV_NAME: u16 = 10;

// ---------------------------------------------------------------------------
// Thermal governors (strings)
// ---------------------------------------------------------------------------

/// Step-wise governor.
pub const THERMAL_GOV_STEP_WISE: &str = "step_wise";
/// Fair-share governor.
pub const THERMAL_GOV_FAIR_SHARE: &str = "fair_share";
/// Bang-bang governor.
pub const THERMAL_GOV_BANG_BANG: &str = "bang_bang";
/// User-space governor.
pub const THERMAL_GOV_USER_SPACE: &str = "user_space";
/// Power allocator governor.
pub const THERMAL_GOV_POWER_ALLOCATOR: &str = "power_allocator";

// ---------------------------------------------------------------------------
// Generic Netlink family name
// ---------------------------------------------------------------------------

/// Thermal Generic Netlink family name.
pub const THERMAL_GENL_FAMILY_NAME: &str = "thermal";

/// Maximum thermal zone name length.
pub const THERMAL_NAME_LENGTH: usize = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genl_cmds_distinct() {
        let cmds = [
            THERMAL_GENL_CMD_UNSPEC,
            THERMAL_GENL_CMD_TZ_GET_TEMP,
            THERMAL_GENL_CMD_TZ_GET_TRIP,
            THERMAL_GENL_CMD_CDEV_GET,
            THERMAL_GENL_EVENT_TZ_CREATE,
            THERMAL_GENL_EVENT_TZ_DELETE,
            THERMAL_GENL_EVENT_TZ_DISABLE,
            THERMAL_GENL_EVENT_TZ_ENABLE,
            THERMAL_GENL_EVENT_TZ_TRIP_CHANGE,
            THERMAL_GENL_EVENT_TZ_TRIP_UP,
            THERMAL_GENL_EVENT_TZ_TRIP_DOWN,
            THERMAL_GENL_EVENT_CDEV_ADD,
            THERMAL_GENL_EVENT_CDEV_DELETE,
            THERMAL_GENL_EVENT_CDEV_STATE_UPDATE,
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
            THERMAL_TREND_DROPPING,
            THERMAL_TREND_STABLE,
        ];
        for i in 0..trends.len() {
            for j in (i + 1)..trends.len() {
                assert_ne!(trends[i], trends[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            THERMAL_GENL_ATTR_TZ_ID,
            THERMAL_GENL_ATTR_TZ_NAME,
            THERMAL_GENL_ATTR_TZ_TEMP,
            THERMAL_GENL_ATTR_TZ_TRIP_ID,
            THERMAL_GENL_ATTR_TZ_TRIP_TEMP,
            THERMAL_GENL_ATTR_TZ_TRIP_TYPE,
            THERMAL_GENL_ATTR_TZ_CDEV_ID,
            THERMAL_GENL_ATTR_TZ_CDEV_MAX_STATE,
            THERMAL_GENL_ATTR_TZ_CDEV_CUR_STATE,
            THERMAL_GENL_ATTR_TZ_GOV_NAME,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_trip_values() {
        assert_eq!(THERMAL_TRIP_ACTIVE, 0);
        assert_eq!(THERMAL_TRIP_CRITICAL, 3);
    }

    #[test]
    fn test_governors() {
        assert_eq!(THERMAL_GOV_STEP_WISE, "step_wise");
        assert_eq!(THERMAL_GOV_BANG_BANG, "bang_bang");
        assert_eq!(THERMAL_GOV_POWER_ALLOCATOR, "power_allocator");
    }

    #[test]
    fn test_family_name() {
        assert_eq!(THERMAL_GENL_FAMILY_NAME, "thermal");
    }

    #[test]
    fn test_name_length() {
        assert_eq!(THERMAL_NAME_LENGTH, 20);
    }
}
