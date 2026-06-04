//! `<linux/thermal.h>` — Cooling device sysfs interface constants.
//!
//! Cooling devices throttle heat-producing hardware (CPU freq, fan
//! speed, processor idle injection). The thermal core binds them to
//! trip points so that crossing a trip activates a cooling level.

// ---------------------------------------------------------------------------
// Sysfs paths under /sys/class/thermal
// ---------------------------------------------------------------------------

pub const COOLING_SYSFS_ROOT: &str = "/sys/class/thermal";
pub const COOLING_SYSFS_DEVICE_PREFIX: &str = "cooling_device";
pub const COOLING_SYSFS_ZONE_PREFIX: &str = "thermal_zone";

// ---------------------------------------------------------------------------
// Cooling device attribute files
// ---------------------------------------------------------------------------

pub const COOLING_ATTR_TYPE: &str = "type";
pub const COOLING_ATTR_MAX_STATE: &str = "max_state";
pub const COOLING_ATTR_CUR_STATE: &str = "cur_state";
pub const COOLING_ATTR_STATS_TIME: &str = "stats/time_in_state_ms";
pub const COOLING_ATTR_STATS_TOTAL_TRANS: &str = "stats/total_trans";

// ---------------------------------------------------------------------------
// Trip point types
// ---------------------------------------------------------------------------

pub const THERMAL_TRIP_ACTIVE: u32 = 0;
pub const THERMAL_TRIP_PASSIVE: u32 = 1;
pub const THERMAL_TRIP_HOT: u32 = 2;
pub const THERMAL_TRIP_CRITICAL: u32 = 3;

// ---------------------------------------------------------------------------
// Common cooling device type strings
// ---------------------------------------------------------------------------

pub const COOLING_TYPE_PROCESSOR: &str = "Processor";
pub const COOLING_TYPE_FAN: &str = "Fan";
pub const COOLING_TYPE_THERMAL_ZONE: &str = "thermal-zone";

// ---------------------------------------------------------------------------
// Special cur_state value: 0 means no throttling.
// ---------------------------------------------------------------------------

pub const COOLING_STATE_DISABLED: u32 = 0;

// ---------------------------------------------------------------------------
// Governor names
// ---------------------------------------------------------------------------

pub const THERMAL_GOV_STEP_WISE: &str = "step_wise";
pub const THERMAL_GOV_FAIR_SHARE: &str = "fair_share";
pub const THERMAL_GOV_USER_SPACE: &str = "user_space";
pub const THERMAL_GOV_POWER_ALLOCATOR: &str = "power_allocator";
pub const THERMAL_GOV_BANG_BANG: &str = "bang_bang";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_under_class_thermal() {
        assert_eq!(COOLING_SYSFS_ROOT, "/sys/class/thermal");
        assert!(COOLING_SYSFS_ROOT.starts_with("/sys/class/"));
        assert!(COOLING_SYSFS_DEVICE_PREFIX.starts_with("cooling_"));
        assert!(COOLING_SYSFS_ZONE_PREFIX.starts_with("thermal_"));
    }

    #[test]
    fn test_attr_files_distinct() {
        let a = [
            COOLING_ATTR_TYPE,
            COOLING_ATTR_MAX_STATE,
            COOLING_ATTR_CUR_STATE,
            COOLING_ATTR_STATS_TIME,
            COOLING_ATTR_STATS_TOTAL_TRANS,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // Stats files are nested under stats/.
        assert!(COOLING_ATTR_STATS_TIME.starts_with("stats/"));
        assert!(COOLING_ATTR_STATS_TOTAL_TRANS.starts_with("stats/"));
    }

    #[test]
    fn test_trip_types_dense_0_to_3() {
        let t = [
            THERMAL_TRIP_ACTIVE,
            THERMAL_TRIP_PASSIVE,
            THERMAL_TRIP_HOT,
            THERMAL_TRIP_CRITICAL,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // CRITICAL is the most severe.
        assert!(THERMAL_TRIP_CRITICAL > THERMAL_TRIP_HOT);
        assert!(THERMAL_TRIP_HOT > THERMAL_TRIP_PASSIVE);
    }

    #[test]
    fn test_type_strings_distinct() {
        assert_ne!(COOLING_TYPE_PROCESSOR, COOLING_TYPE_FAN);
        assert_ne!(COOLING_TYPE_FAN, COOLING_TYPE_THERMAL_ZONE);
        // "Processor" and "Fan" are capitalized (ACPI convention).
        assert!(COOLING_TYPE_PROCESSOR
            .chars()
            .next()
            .unwrap()
            .is_ascii_uppercase());
        assert!(COOLING_TYPE_FAN.chars().next().unwrap().is_ascii_uppercase());
    }

    #[test]
    fn test_disabled_state_is_zero() {
        assert_eq!(COOLING_STATE_DISABLED, 0);
    }

    #[test]
    fn test_governor_names_lowercase_with_underscore() {
        for g in [
            THERMAL_GOV_STEP_WISE,
            THERMAL_GOV_FAIR_SHARE,
            THERMAL_GOV_USER_SPACE,
            THERMAL_GOV_POWER_ALLOCATOR,
            THERMAL_GOV_BANG_BANG,
        ] {
            for c in g.chars() {
                assert!(c.is_ascii_lowercase() || c == '_');
            }
        }
    }
}
