//! `<linux/power_supply.h>` — Additional power supply constants.
//!
//! Supplementary power supply constants covering supply types,
//! status values, health states, and technology types.

// ---------------------------------------------------------------------------
// Power supply types
// ---------------------------------------------------------------------------

/// Battery.
pub const POWER_SUPPLY_TYPE_BATTERY: u32 = 0;
/// UPS.
pub const POWER_SUPPLY_TYPE_UPS: u32 = 1;
/// Mains.
pub const POWER_SUPPLY_TYPE_MAINS: u32 = 2;
/// USB.
pub const POWER_SUPPLY_TYPE_USB: u32 = 3;
/// USB DCP.
pub const POWER_SUPPLY_TYPE_USB_DCP: u32 = 4;
/// USB CDP.
pub const POWER_SUPPLY_TYPE_USB_CDP: u32 = 5;
/// USB ACA.
pub const POWER_SUPPLY_TYPE_USB_ACA: u32 = 6;
/// USB Type-C.
pub const POWER_SUPPLY_TYPE_USB_TYPE_C: u32 = 7;
/// USB PD.
pub const POWER_SUPPLY_TYPE_USB_PD: u32 = 8;
/// USB PD DRP.
pub const POWER_SUPPLY_TYPE_USB_PD_DRP: u32 = 9;
/// Wireless.
pub const POWER_SUPPLY_TYPE_WIRELESS: u32 = 10;

// ---------------------------------------------------------------------------
// Power supply status
// ---------------------------------------------------------------------------

/// Unknown status.
pub const POWER_SUPPLY_STATUS_UNKNOWN: u32 = 0;
/// Charging.
pub const POWER_SUPPLY_STATUS_CHARGING: u32 = 1;
/// Discharging.
pub const POWER_SUPPLY_STATUS_DISCHARGING: u32 = 2;
/// Not charging.
pub const POWER_SUPPLY_STATUS_NOT_CHARGING: u32 = 3;
/// Full.
pub const POWER_SUPPLY_STATUS_FULL: u32 = 4;

// ---------------------------------------------------------------------------
// Power supply health
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
/// Watchdog timer expired.
pub const POWER_SUPPLY_HEALTH_WATCHDOG_TIMER_EXPIRE: u32 = 7;
/// Safety timer expired.
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
/// No battery.
pub const POWER_SUPPLY_HEALTH_NO_BATTERY: u32 = 14;

// ---------------------------------------------------------------------------
// Power supply technology
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
/// LiMn.
pub const POWER_SUPPLY_TECHNOLOGY_LIMN: u32 = 6;

// ---------------------------------------------------------------------------
// Power supply capacity level
// ---------------------------------------------------------------------------

/// Unknown level.
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
            POWER_SUPPLY_HEALTH_NO_BATTERY,
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
    fn test_capacity_level_ordering() {
        assert!(POWER_SUPPLY_CAPACITY_LEVEL_UNKNOWN < POWER_SUPPLY_CAPACITY_LEVEL_CRITICAL);
        assert!(POWER_SUPPLY_CAPACITY_LEVEL_CRITICAL < POWER_SUPPLY_CAPACITY_LEVEL_LOW);
        assert!(POWER_SUPPLY_CAPACITY_LEVEL_LOW < POWER_SUPPLY_CAPACITY_LEVEL_NORMAL);
        assert!(POWER_SUPPLY_CAPACITY_LEVEL_NORMAL < POWER_SUPPLY_CAPACITY_LEVEL_HIGH);
        assert!(POWER_SUPPLY_CAPACITY_LEVEL_HIGH < POWER_SUPPLY_CAPACITY_LEVEL_FULL);
    }
}
