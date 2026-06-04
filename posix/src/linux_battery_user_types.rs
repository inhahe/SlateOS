//! Battery / `power_supply` sysfs surface.
//!
//! Linux exposes batteries (and AC adaptors, USB chargers, etc.)
//! through `/sys/class/power_supply/`. Each device advertises a `type`
//! attribute plus a stable set of named integer/string properties.
//! Userspace tools (`upower`, `acpid`, GNOME/KDE power applets) read
//! these.

// ---------------------------------------------------------------------------
// sysfs root and common attribute names
// ---------------------------------------------------------------------------

pub const SYS_CLASS_POWER_SUPPLY: &str = "/sys/class/power_supply";
pub const POWER_SUPPLY_TYPE: &str = "type";
pub const POWER_SUPPLY_STATUS: &str = "status";
pub const POWER_SUPPLY_PRESENT: &str = "present";
pub const POWER_SUPPLY_ONLINE: &str = "online";
pub const POWER_SUPPLY_CAPACITY: &str = "capacity";
pub const POWER_SUPPLY_CAPACITY_LEVEL: &str = "capacity_level";
pub const POWER_SUPPLY_TECHNOLOGY: &str = "technology";

// ---------------------------------------------------------------------------
// `type` string values
// ---------------------------------------------------------------------------

pub const POWER_SUPPLY_TYPE_UNKNOWN: &str = "Unknown";
pub const POWER_SUPPLY_TYPE_BATTERY: &str = "Battery";
pub const POWER_SUPPLY_TYPE_UPS: &str = "UPS";
pub const POWER_SUPPLY_TYPE_MAINS: &str = "Mains";
pub const POWER_SUPPLY_TYPE_USB: &str = "USB";
pub const POWER_SUPPLY_TYPE_USB_DCP: &str = "USB_DCP";
pub const POWER_SUPPLY_TYPE_USB_CDP: &str = "USB_CDP";
pub const POWER_SUPPLY_TYPE_USB_PD: &str = "USB_PD";

// ---------------------------------------------------------------------------
// `status` string values
// ---------------------------------------------------------------------------

pub const POWER_SUPPLY_STATUS_UNKNOWN: &str = "Unknown";
pub const POWER_SUPPLY_STATUS_CHARGING: &str = "Charging";
pub const POWER_SUPPLY_STATUS_DISCHARGING: &str = "Discharging";
pub const POWER_SUPPLY_STATUS_NOT_CHARGING: &str = "Not charging";
pub const POWER_SUPPLY_STATUS_FULL: &str = "Full";

// ---------------------------------------------------------------------------
// `capacity_level` string values
// ---------------------------------------------------------------------------

pub const POWER_SUPPLY_CAPACITY_LEVEL_UNKNOWN: &str = "Unknown";
pub const POWER_SUPPLY_CAPACITY_LEVEL_CRITICAL: &str = "Critical";
pub const POWER_SUPPLY_CAPACITY_LEVEL_LOW: &str = "Low";
pub const POWER_SUPPLY_CAPACITY_LEVEL_NORMAL: &str = "Normal";
pub const POWER_SUPPLY_CAPACITY_LEVEL_HIGH: &str = "High";
pub const POWER_SUPPLY_CAPACITY_LEVEL_FULL: &str = "Full";

// ---------------------------------------------------------------------------
// `technology` string values
// ---------------------------------------------------------------------------

pub const POWER_SUPPLY_TECH_UNKNOWN: &str = "Unknown";
pub const POWER_SUPPLY_TECH_NIMH: &str = "NiMH";
pub const POWER_SUPPLY_TECH_LION: &str = "Li-ion";
pub const POWER_SUPPLY_TECH_LIPO: &str = "Li-poly";
pub const POWER_SUPPLY_TECH_LIFE: &str = "LiFe";
pub const POWER_SUPPLY_TECH_NICD: &str = "NiCd";
pub const POWER_SUPPLY_TECH_LIMN: &str = "LiMn";

// ---------------------------------------------------------------------------
// Numeric percentage bounds
// ---------------------------------------------------------------------------

pub const POWER_SUPPLY_CAPACITY_MIN: u32 = 0;
pub const POWER_SUPPLY_CAPACITY_MAX: u32 = 100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_root_path() {
        assert_eq!(SYS_CLASS_POWER_SUPPLY, "/sys/class/power_supply");
        assert!(SYS_CLASS_POWER_SUPPLY.starts_with("/sys/class/"));
    }

    #[test]
    fn test_attribute_names_distinct_and_no_slash() {
        let a = [
            POWER_SUPPLY_TYPE,
            POWER_SUPPLY_STATUS,
            POWER_SUPPLY_PRESENT,
            POWER_SUPPLY_ONLINE,
            POWER_SUPPLY_CAPACITY,
            POWER_SUPPLY_CAPACITY_LEVEL,
            POWER_SUPPLY_TECHNOLOGY,
        ];
        for &v in &a {
            assert!(!v.contains('/'));
            assert!(!v.is_empty());
        }
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_type_strings_distinct() {
        let t = [
            POWER_SUPPLY_TYPE_UNKNOWN,
            POWER_SUPPLY_TYPE_BATTERY,
            POWER_SUPPLY_TYPE_UPS,
            POWER_SUPPLY_TYPE_MAINS,
            POWER_SUPPLY_TYPE_USB,
            POWER_SUPPLY_TYPE_USB_DCP,
            POWER_SUPPLY_TYPE_USB_CDP,
            POWER_SUPPLY_TYPE_USB_PD,
        ];
        for (i, &x) in t.iter().enumerate() {
            for &y in &t[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // USB subtypes all start with "USB".
        assert!(POWER_SUPPLY_TYPE_USB_DCP.starts_with("USB"));
        assert!(POWER_SUPPLY_TYPE_USB_CDP.starts_with("USB"));
        assert!(POWER_SUPPLY_TYPE_USB_PD.starts_with("USB"));
    }

    #[test]
    fn test_status_strings_capitalised() {
        let s = [
            POWER_SUPPLY_STATUS_UNKNOWN,
            POWER_SUPPLY_STATUS_CHARGING,
            POWER_SUPPLY_STATUS_DISCHARGING,
            POWER_SUPPLY_STATUS_NOT_CHARGING,
            POWER_SUPPLY_STATUS_FULL,
        ];
        for &v in &s {
            // First letter is uppercase ASCII (sysfs writes them that way).
            assert!(v.as_bytes()[0].is_ascii_uppercase());
        }
        // "Not charging" is the only multi-word value.
        assert!(POWER_SUPPLY_STATUS_NOT_CHARGING.contains(' '));
    }

    #[test]
    fn test_capacity_levels_have_full_at_top() {
        // The level strings are an ordered scale, but the numeric
        // ordering is implicit — verified by membership.
        let levels = [
            POWER_SUPPLY_CAPACITY_LEVEL_UNKNOWN,
            POWER_SUPPLY_CAPACITY_LEVEL_CRITICAL,
            POWER_SUPPLY_CAPACITY_LEVEL_LOW,
            POWER_SUPPLY_CAPACITY_LEVEL_NORMAL,
            POWER_SUPPLY_CAPACITY_LEVEL_HIGH,
            POWER_SUPPLY_CAPACITY_LEVEL_FULL,
        ];
        for (i, &x) in levels.iter().enumerate() {
            for &y in &levels[i + 1..] {
                assert_ne!(x, y);
            }
        }
        assert_eq!(*levels.last().unwrap(), "Full");
    }

    #[test]
    fn test_tech_strings_distinct() {
        let t = [
            POWER_SUPPLY_TECH_UNKNOWN,
            POWER_SUPPLY_TECH_NIMH,
            POWER_SUPPLY_TECH_LION,
            POWER_SUPPLY_TECH_LIPO,
            POWER_SUPPLY_TECH_LIFE,
            POWER_SUPPLY_TECH_NICD,
            POWER_SUPPLY_TECH_LIMN,
        ];
        for (i, &x) in t.iter().enumerate() {
            for &y in &t[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // Li-* family members start with 'Li'.
        for &v in &[
            POWER_SUPPLY_TECH_LION,
            POWER_SUPPLY_TECH_LIPO,
            POWER_SUPPLY_TECH_LIFE,
            POWER_SUPPLY_TECH_LIMN,
        ] {
            assert!(v.starts_with("Li"));
        }
    }

    #[test]
    fn test_capacity_bounds() {
        assert_eq!(POWER_SUPPLY_CAPACITY_MIN, 0);
        assert_eq!(POWER_SUPPLY_CAPACITY_MAX, 100);
        assert!(POWER_SUPPLY_CAPACITY_MIN < POWER_SUPPLY_CAPACITY_MAX);
    }
}
