//! `<linux/power_supply.h>` — Power supply subsystem constants.
//!
//! The power supply subsystem reports battery status, AC adapter state,
//! and USB power delivery info via sysfs. Used by upower, tlp, and
//! desktop battery widgets.

// ---------------------------------------------------------------------------
// Power supply types
// ---------------------------------------------------------------------------

/// Battery.
pub const POWER_SUPPLY_TYPE_BATTERY: u32 = 1;
/// UPS (uninterruptible power supply).
pub const POWER_SUPPLY_TYPE_UPS: u32 = 2;
/// AC mains.
pub const POWER_SUPPLY_TYPE_MAINS: u32 = 3;
/// USB Type-A/B.
pub const POWER_SUPPLY_TYPE_USB: u32 = 4;
/// USB Dedicated Charging Port.
pub const POWER_SUPPLY_TYPE_USB_DCP: u32 = 5;
/// USB Charging Downstream Port.
pub const POWER_SUPPLY_TYPE_USB_CDP: u32 = 6;
/// USB Accessory Charger Adapter.
pub const POWER_SUPPLY_TYPE_USB_ACA: u32 = 7;
/// USB Type-C.
pub const POWER_SUPPLY_TYPE_USB_TYPE_C: u32 = 8;
/// USB Power Delivery.
pub const POWER_SUPPLY_TYPE_USB_PD: u32 = 9;
/// USB PD DRP.
pub const POWER_SUPPLY_TYPE_USB_PD_DRP: u32 = 10;
/// Apple Brick ID.
pub const POWER_SUPPLY_TYPE_APPLE_BRICK_ID: u32 = 11;
/// Wireless.
pub const POWER_SUPPLY_TYPE_WIRELESS: u32 = 12;

// ---------------------------------------------------------------------------
// Power supply status
// ---------------------------------------------------------------------------

/// Unknown status.
pub const POWER_SUPPLY_STATUS_UNKNOWN: u32 = 0;
/// Charging.
pub const POWER_SUPPLY_STATUS_CHARGING: u32 = 1;
/// Discharging.
pub const POWER_SUPPLY_STATUS_DISCHARGING: u32 = 2;
/// Not charging (plugged in but not charging).
pub const POWER_SUPPLY_STATUS_NOT_CHARGING: u32 = 3;
/// Full.
pub const POWER_SUPPLY_STATUS_FULL: u32 = 4;

// ---------------------------------------------------------------------------
// Battery health
// ---------------------------------------------------------------------------

/// Unknown health.
pub const POWER_SUPPLY_HEALTH_UNKNOWN: u32 = 0;
/// Good.
pub const POWER_SUPPLY_HEALTH_GOOD: u32 = 1;
/// Overheat.
pub const POWER_SUPPLY_HEALTH_OVERHEAT: u32 = 2;
/// Dead.
pub const POWER_SUPPLY_HEALTH_DEAD: u32 = 3;
/// Over voltage.
pub const POWER_SUPPLY_HEALTH_OVERVOLTAGE: u32 = 4;
/// Unspecified failure.
pub const POWER_SUPPLY_HEALTH_UNSPEC_FAILURE: u32 = 5;
/// Cold.
pub const POWER_SUPPLY_HEALTH_COLD: u32 = 6;
/// Watchdog timer expire.
pub const POWER_SUPPLY_HEALTH_WATCHDOG_TIMER_EXPIRE: u32 = 7;
/// Safety timer expire.
pub const POWER_SUPPLY_HEALTH_SAFETY_TIMER_EXPIRE: u32 = 8;
/// Over current.
pub const POWER_SUPPLY_HEALTH_OVERCURRENT: u32 = 9;
/// Calibration required.
pub const POWER_SUPPLY_HEALTH_CALIBRATION_REQUIRED: u32 = 10;
/// Warm.
pub const POWER_SUPPLY_HEALTH_WARM: u32 = 11;
/// Cool.
pub const POWER_SUPPLY_HEALTH_COOL: u32 = 12;
/// Hot.
pub const POWER_SUPPLY_HEALTH_HOT: u32 = 13;

// ---------------------------------------------------------------------------
// Battery technology
// ---------------------------------------------------------------------------

/// Unknown technology.
pub const POWER_SUPPLY_TECHNOLOGY_UNKNOWN: u32 = 0;
/// NiMH.
pub const POWER_SUPPLY_TECHNOLOGY_NIMH: u32 = 1;
/// Li-ion.
pub const POWER_SUPPLY_TECHNOLOGY_LION: u32 = 2;
/// Li-poly.
pub const POWER_SUPPLY_TECHNOLOGY_LIPO: u32 = 3;
/// LiFe.
pub const POWER_SUPPLY_TECHNOLOGY_LIFE: u32 = 4;
/// NiCd.
pub const POWER_SUPPLY_TECHNOLOGY_NICD: u32 = 5;
/// Lead-Acid (LiMn).
pub const POWER_SUPPLY_TECHNOLOGY_LIMN: u32 = 6;

// ---------------------------------------------------------------------------
// Charge types
// ---------------------------------------------------------------------------

/// Unknown charge type.
pub const POWER_SUPPLY_CHARGE_TYPE_UNKNOWN: u32 = 0;
/// Not charging.
pub const POWER_SUPPLY_CHARGE_TYPE_NONE: u32 = 1;
/// Trickle charge.
pub const POWER_SUPPLY_CHARGE_TYPE_TRICKLE: u32 = 2;
/// Fast charge.
pub const POWER_SUPPLY_CHARGE_TYPE_FAST: u32 = 3;
/// Standard charge.
pub const POWER_SUPPLY_CHARGE_TYPE_STANDARD: u32 = 4;
/// Adaptive charge.
pub const POWER_SUPPLY_CHARGE_TYPE_ADAPTIVE: u32 = 5;
/// Custom charge.
pub const POWER_SUPPLY_CHARGE_TYPE_CUSTOM: u32 = 6;
/// Long-life charge.
pub const POWER_SUPPLY_CHARGE_TYPE_LONGLIFE: u32 = 7;
/// Bypass charge.
pub const POWER_SUPPLY_CHARGE_TYPE_BYPASS: u32 = 8;

// ---------------------------------------------------------------------------
// Capacity levels
// ---------------------------------------------------------------------------

/// Unknown capacity level.
pub const POWER_SUPPLY_CAPACITY_LEVEL_UNKNOWN: u32 = 0;
/// Critical.
pub const POWER_SUPPLY_CAPACITY_LEVEL_CRITICAL: u32 = 1;
/// Low.
pub const POWER_SUPPLY_CAPACITY_LEVEL_LOW: u32 = 2;
/// Normal.
pub const POWER_SUPPLY_CAPACITY_LEVEL_NORMAL: u32 = 3;
/// High.
pub const POWER_SUPPLY_CAPACITY_LEVEL_HIGH: u32 = 4;
/// Full.
pub const POWER_SUPPLY_CAPACITY_LEVEL_FULL: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            POWER_SUPPLY_TYPE_BATTERY,
            POWER_SUPPLY_TYPE_UPS,
            POWER_SUPPLY_TYPE_MAINS,
            POWER_SUPPLY_TYPE_USB,
            POWER_SUPPLY_TYPE_USB_DCP,
            POWER_SUPPLY_TYPE_USB_CDP,
            POWER_SUPPLY_TYPE_USB_ACA,
            POWER_SUPPLY_TYPE_USB_TYPE_C,
            POWER_SUPPLY_TYPE_USB_PD,
            POWER_SUPPLY_TYPE_USB_PD_DRP,
            POWER_SUPPLY_TYPE_APPLE_BRICK_ID,
            POWER_SUPPLY_TYPE_WIRELESS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_status_distinct() {
        let statuses = [
            POWER_SUPPLY_STATUS_UNKNOWN,
            POWER_SUPPLY_STATUS_CHARGING,
            POWER_SUPPLY_STATUS_DISCHARGING,
            POWER_SUPPLY_STATUS_NOT_CHARGING,
            POWER_SUPPLY_STATUS_FULL,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_health_distinct() {
        let healths = [
            POWER_SUPPLY_HEALTH_UNKNOWN,
            POWER_SUPPLY_HEALTH_GOOD,
            POWER_SUPPLY_HEALTH_OVERHEAT,
            POWER_SUPPLY_HEALTH_DEAD,
            POWER_SUPPLY_HEALTH_OVERVOLTAGE,
            POWER_SUPPLY_HEALTH_UNSPEC_FAILURE,
            POWER_SUPPLY_HEALTH_COLD,
            POWER_SUPPLY_HEALTH_WATCHDOG_TIMER_EXPIRE,
            POWER_SUPPLY_HEALTH_SAFETY_TIMER_EXPIRE,
            POWER_SUPPLY_HEALTH_OVERCURRENT,
            POWER_SUPPLY_HEALTH_CALIBRATION_REQUIRED,
            POWER_SUPPLY_HEALTH_WARM,
            POWER_SUPPLY_HEALTH_COOL,
            POWER_SUPPLY_HEALTH_HOT,
        ];
        for i in 0..healths.len() {
            for j in (i + 1)..healths.len() {
                assert_ne!(healths[i], healths[j]);
            }
        }
    }

    #[test]
    fn test_technology_distinct() {
        let techs = [
            POWER_SUPPLY_TECHNOLOGY_UNKNOWN,
            POWER_SUPPLY_TECHNOLOGY_NIMH,
            POWER_SUPPLY_TECHNOLOGY_LION,
            POWER_SUPPLY_TECHNOLOGY_LIPO,
            POWER_SUPPLY_TECHNOLOGY_LIFE,
            POWER_SUPPLY_TECHNOLOGY_NICD,
            POWER_SUPPLY_TECHNOLOGY_LIMN,
        ];
        for i in 0..techs.len() {
            for j in (i + 1)..techs.len() {
                assert_ne!(techs[i], techs[j]);
            }
        }
    }

    #[test]
    fn test_charge_types_distinct() {
        let charges = [
            POWER_SUPPLY_CHARGE_TYPE_UNKNOWN,
            POWER_SUPPLY_CHARGE_TYPE_NONE,
            POWER_SUPPLY_CHARGE_TYPE_TRICKLE,
            POWER_SUPPLY_CHARGE_TYPE_FAST,
            POWER_SUPPLY_CHARGE_TYPE_STANDARD,
            POWER_SUPPLY_CHARGE_TYPE_ADAPTIVE,
            POWER_SUPPLY_CHARGE_TYPE_CUSTOM,
            POWER_SUPPLY_CHARGE_TYPE_LONGLIFE,
            POWER_SUPPLY_CHARGE_TYPE_BYPASS,
        ];
        for i in 0..charges.len() {
            for j in (i + 1)..charges.len() {
                assert_ne!(charges[i], charges[j]);
            }
        }
    }

    #[test]
    fn test_capacity_levels_distinct() {
        let levels = [
            POWER_SUPPLY_CAPACITY_LEVEL_UNKNOWN,
            POWER_SUPPLY_CAPACITY_LEVEL_CRITICAL,
            POWER_SUPPLY_CAPACITY_LEVEL_LOW,
            POWER_SUPPLY_CAPACITY_LEVEL_NORMAL,
            POWER_SUPPLY_CAPACITY_LEVEL_HIGH,
            POWER_SUPPLY_CAPACITY_LEVEL_FULL,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_status_values() {
        assert_eq!(POWER_SUPPLY_STATUS_CHARGING, 1);
        assert_eq!(POWER_SUPPLY_STATUS_FULL, 4);
    }

    #[test]
    fn test_type_values() {
        assert_eq!(POWER_SUPPLY_TYPE_BATTERY, 1);
        assert_eq!(POWER_SUPPLY_TYPE_MAINS, 3);
    }
}
