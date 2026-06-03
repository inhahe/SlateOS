//! ACPI thermal — `/sys/class/thermal/`, trip points, cooling devices.
//!
//! Linux exposes ACPI thermal-zone temperatures, trip points, and
//! associated cooling devices (fans, CPU freq capping) under
//! `/sys/class/thermal/`. `thermald`, `lm-sensors`, and `tlp` read
//! these.

// ---------------------------------------------------------------------------
// Sysfs roots
// ---------------------------------------------------------------------------

pub const SYS_CLASS_THERMAL: &str = "/sys/class/thermal";
pub const SYS_CLASS_HWMON: &str = "/sys/class/hwmon";

pub const THERMAL_ZONE_PREFIX: &str = "thermal_zone";
pub const COOLING_DEVICE_PREFIX: &str = "cooling_device";

// ---------------------------------------------------------------------------
// Per-zone attribute names (relative to /sys/class/thermal/thermal_zoneN/)
// ---------------------------------------------------------------------------

pub const TZ_ATTR_TYPE: &str = "type";
pub const TZ_ATTR_TEMP: &str = "temp";
pub const TZ_ATTR_MODE: &str = "mode";
pub const TZ_ATTR_POLICY: &str = "policy";
pub const TZ_ATTR_AVAILABLE_POLICIES: &str = "available_policies";
pub const TZ_ATTR_K_PO: &str = "k_po";
pub const TZ_ATTR_K_PU: &str = "k_pu";
pub const TZ_ATTR_K_I: &str = "k_i";
pub const TZ_ATTR_K_D: &str = "k_d";
pub const TZ_ATTR_INTEGRAL_CUTOFF: &str = "integral_cutoff";
pub const TZ_ATTR_SUSTAINABLE_POWER: &str = "sustainable_power";

// ---------------------------------------------------------------------------
// Trip-point types (`/sys/class/thermal/thermal_zoneN/trip_point_N_type`)
// ---------------------------------------------------------------------------

pub const TRIP_TYPE_CRITICAL: &str = "critical";
pub const TRIP_TYPE_HOT: &str = "hot";
pub const TRIP_TYPE_PASSIVE: &str = "passive";
pub const TRIP_TYPE_ACTIVE: &str = "active";

// ---------------------------------------------------------------------------
// Governor / policy names (`/sys/class/thermal/thermal_zoneN/policy`)
// ---------------------------------------------------------------------------

pub const THERMAL_POLICY_STEP_WISE: &str = "step_wise";
pub const THERMAL_POLICY_FAIR_SHARE: &str = "fair_share";
pub const THERMAL_POLICY_BANG_BANG: &str = "bang_bang";
pub const THERMAL_POLICY_USER_SPACE: &str = "user_space";
pub const THERMAL_POLICY_POWER_ALLOCATOR: &str = "power_allocator";

// ---------------------------------------------------------------------------
// Temperature unit (millidegrees Celsius — the kernel convention)
// ---------------------------------------------------------------------------

/// 1 °C in the sysfs millidegree unit.
pub const THERMAL_MDEG_PER_DEG_C: i32 = 1000;

/// Sentinel value used by `/sys/class/thermal/.../temp` when invalid.
pub const THERMAL_TEMP_INVALID: i32 = -274_000; // colder than 0 K

// ---------------------------------------------------------------------------
// Cooling-device states
// ---------------------------------------------------------------------------

pub const TZ_COOLING_DEVICE_MIN_STATE: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_roots_under_sys_class() {
        assert!(SYS_CLASS_THERMAL.starts_with("/sys/class/"));
        assert!(SYS_CLASS_HWMON.starts_with("/sys/class/"));
        // The zone and cooling-device prefixes don't include the root.
        assert!(!THERMAL_ZONE_PREFIX.starts_with('/'));
        assert!(!COOLING_DEVICE_PREFIX.starts_with('/'));
    }

    #[test]
    fn test_zone_attr_names_distinct() {
        let a = [
            TZ_ATTR_TYPE,
            TZ_ATTR_TEMP,
            TZ_ATTR_MODE,
            TZ_ATTR_POLICY,
            TZ_ATTR_AVAILABLE_POLICIES,
            TZ_ATTR_K_PO,
            TZ_ATTR_K_PU,
            TZ_ATTR_K_I,
            TZ_ATTR_K_D,
            TZ_ATTR_INTEGRAL_CUTOFF,
            TZ_ATTR_SUSTAINABLE_POWER,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn test_trip_types_distinct() {
        let t = [
            TRIP_TYPE_CRITICAL,
            TRIP_TYPE_HOT,
            TRIP_TYPE_PASSIVE,
            TRIP_TYPE_ACTIVE,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
        // "critical" is the only mandatory trip per the ACPI spec.
        assert_eq!(TRIP_TYPE_CRITICAL, "critical");
    }

    #[test]
    fn test_policy_names_distinct() {
        let p = [
            THERMAL_POLICY_STEP_WISE,
            THERMAL_POLICY_FAIR_SHARE,
            THERMAL_POLICY_BANG_BANG,
            THERMAL_POLICY_USER_SPACE,
            THERMAL_POLICY_POWER_ALLOCATOR,
        ];
        for i in 0..p.len() {
            for j in (i + 1)..p.len() {
                assert_ne!(p[i], p[j]);
            }
        }
    }

    #[test]
    fn test_temperature_unit_conversion() {
        // Kernel reports in millidegrees C — 1 °C = 1000.
        assert_eq!(THERMAL_MDEG_PER_DEG_C, 1000);
        // Multiply-by-MDEG must round-trip a positive degC.
        let degc: i32 = 42;
        let mdeg = degc * THERMAL_MDEG_PER_DEG_C;
        assert_eq!(mdeg / THERMAL_MDEG_PER_DEG_C, degc);
    }

    #[test]
    fn test_invalid_sentinel_below_absolute_zero() {
        // The sentinel must be colder than 0 K (-273.15 °C) so it can't
        // be confused with a real reading.
        assert!(THERMAL_TEMP_INVALID < -273 * THERMAL_MDEG_PER_DEG_C);
        assert_eq!(THERMAL_TEMP_INVALID, -274_000);
    }

    #[test]
    fn test_cooling_min_state_zero() {
        assert_eq!(TZ_COOLING_DEVICE_MIN_STATE, 0);
    }
}
