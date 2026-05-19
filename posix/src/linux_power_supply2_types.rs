//! `<linux/power_supply.h>` — Additional power supply constants.
//!
//! Supplementary power supply constants covering types,
//! status values, and health values.

// ---------------------------------------------------------------------------
// Power supply types
// ---------------------------------------------------------------------------

/// Battery.
pub const POWER_SUPPLY_TYPE_BATTERY: u32 = 1;
/// UPS.
pub const POWER_SUPPLY_TYPE_UPS: u32 = 2;
/// Mains (AC).
pub const POWER_SUPPLY_TYPE_MAINS: u32 = 3;
/// USB.
pub const POWER_SUPPLY_TYPE_USB: u32 = 4;
/// USB DCP.
pub const POWER_SUPPLY_TYPE_USB_DCP: u32 = 5;
/// USB CDP.
pub const POWER_SUPPLY_TYPE_USB_CDP: u32 = 6;
/// USB ACA.
pub const POWER_SUPPLY_TYPE_USB_ACA: u32 = 7;
/// USB Type-C.
pub const POWER_SUPPLY_TYPE_USB_TYPE_C: u32 = 8;
/// USB PD.
pub const POWER_SUPPLY_TYPE_USB_PD: u32 = 9;
/// USB PD DRP.
pub const POWER_SUPPLY_TYPE_USB_PD_DRP: u32 = 10;
/// Apple brick ID.
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
/// Unspec failure.
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
/// No battery.
pub const POWER_SUPPLY_HEALTH_NO_BATTERY: u32 = 14;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            POWER_SUPPLY_TYPE_BATTERY, POWER_SUPPLY_TYPE_UPS,
            POWER_SUPPLY_TYPE_MAINS, POWER_SUPPLY_TYPE_USB,
            POWER_SUPPLY_TYPE_USB_DCP, POWER_SUPPLY_TYPE_USB_CDP,
            POWER_SUPPLY_TYPE_USB_ACA, POWER_SUPPLY_TYPE_USB_TYPE_C,
            POWER_SUPPLY_TYPE_USB_PD, POWER_SUPPLY_TYPE_USB_PD_DRP,
            POWER_SUPPLY_TYPE_APPLE_BRICK_ID, POWER_SUPPLY_TYPE_WIRELESS,
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
            POWER_SUPPLY_STATUS_UNKNOWN, POWER_SUPPLY_STATUS_CHARGING,
            POWER_SUPPLY_STATUS_DISCHARGING, POWER_SUPPLY_STATUS_NOT_CHARGING,
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
            POWER_SUPPLY_HEALTH_UNKNOWN, POWER_SUPPLY_HEALTH_GOOD,
            POWER_SUPPLY_HEALTH_OVERHEAT, POWER_SUPPLY_HEALTH_DEAD,
            POWER_SUPPLY_HEALTH_OVERVOLTAGE, POWER_SUPPLY_HEALTH_UNSPEC_FAILURE,
            POWER_SUPPLY_HEALTH_COLD, POWER_SUPPLY_HEALTH_WATCHDOG_TIMER_EXPIRE,
            POWER_SUPPLY_HEALTH_SAFETY_TIMER_EXPIRE,
            POWER_SUPPLY_HEALTH_OVERCURRENT,
            POWER_SUPPLY_HEALTH_CALIBRATION_REQUIRED,
            POWER_SUPPLY_HEALTH_WARM, POWER_SUPPLY_HEALTH_COOL,
            POWER_SUPPLY_HEALTH_HOT, POWER_SUPPLY_HEALTH_NO_BATTERY,
        ];
        for i in 0..healths.len() {
            for j in (i + 1)..healths.len() {
                assert_ne!(healths[i], healths[j]);
            }
        }
    }
}
