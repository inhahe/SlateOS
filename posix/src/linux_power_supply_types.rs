//! `<linux/power_supply.h>` — Power supply subsystem constants.
//!
//! The power_supply subsystem provides a unified interface for
//! batteries, AC adapters, and USB chargers. Each power supply
//! exports properties (charge level, voltage, current, status, etc.)
//! via sysfs and can send uevents on state changes. Used by battery
//! monitors, power managers, and UPS daemons.

// ---------------------------------------------------------------------------
// Power supply types
// ---------------------------------------------------------------------------

/// Unknown type.
pub const POWER_SUPPLY_TYPE_UNKNOWN: u32 = 0;
/// Battery (discharges).
pub const POWER_SUPPLY_TYPE_BATTERY: u32 = 1;
/// UPS (Uninterruptible Power Supply).
pub const POWER_SUPPLY_TYPE_UPS: u32 = 2;
/// Mains / AC adapter.
pub const POWER_SUPPLY_TYPE_MAINS: u32 = 3;
/// USB charger (SDP - Standard Downstream Port).
pub const POWER_SUPPLY_TYPE_USB: u32 = 4;
/// USB DCP (Dedicated Charging Port).
pub const POWER_SUPPLY_TYPE_USB_DCP: u32 = 5;
/// USB CDP (Charging Downstream Port).
pub const POWER_SUPPLY_TYPE_USB_CDP: u32 = 6;
/// USB-C adapter.
pub const POWER_SUPPLY_TYPE_USB_TYPE_C: u32 = 9;
/// USB Power Delivery.
pub const POWER_SUPPLY_TYPE_USB_PD: u32 = 10;
/// Wireless charger.
pub const POWER_SUPPLY_TYPE_WIRELESS: u32 = 11;

// ---------------------------------------------------------------------------
// Power supply status
// ---------------------------------------------------------------------------

/// Status unknown.
pub const POWER_SUPPLY_STATUS_UNKNOWN: u32 = 0;
/// Battery is charging.
pub const POWER_SUPPLY_STATUS_CHARGING: u32 = 1;
/// Battery is discharging.
pub const POWER_SUPPLY_STATUS_DISCHARGING: u32 = 2;
/// Battery not charging (full or inhibited).
pub const POWER_SUPPLY_STATUS_NOT_CHARGING: u32 = 3;
/// Battery is full.
pub const POWER_SUPPLY_STATUS_FULL: u32 = 4;

// ---------------------------------------------------------------------------
// Battery health
// ---------------------------------------------------------------------------

/// Health unknown.
pub const POWER_SUPPLY_HEALTH_UNKNOWN: u32 = 0;
/// Battery is healthy.
pub const POWER_SUPPLY_HEALTH_GOOD: u32 = 1;
/// Battery overheated.
pub const POWER_SUPPLY_HEALTH_OVERHEAT: u32 = 2;
/// Battery has dead cell(s).
pub const POWER_SUPPLY_HEALTH_DEAD: u32 = 3;
/// Over-voltage condition.
pub const POWER_SUPPLY_HEALTH_OVERVOLTAGE: u32 = 4;
/// Unspecified failure.
pub const POWER_SUPPLY_HEALTH_UNSPEC_FAILURE: u32 = 5;
/// Battery is cold (below operating temperature).
pub const POWER_SUPPLY_HEALTH_COLD: u32 = 6;
/// Watchdog timer expired.
pub const POWER_SUPPLY_HEALTH_WATCHDOG_TIMER_EXPIRE: u32 = 7;
/// Safety timer expired.
pub const POWER_SUPPLY_HEALTH_SAFETY_TIMER_EXPIRE: u32 = 8;
/// Over-current protection.
pub const POWER_SUPPLY_HEALTH_OVERCURRENT: u32 = 9;

// ---------------------------------------------------------------------------
// Battery technology
// ---------------------------------------------------------------------------

/// Unknown technology.
pub const POWER_SUPPLY_TECHNOLOGY_UNKNOWN: u32 = 0;
/// Nickel-Metal Hydride.
pub const POWER_SUPPLY_TECHNOLOGY_NIMH: u32 = 1;
/// Lithium-ion.
pub const POWER_SUPPLY_TECHNOLOGY_LION: u32 = 2;
/// Lithium-polymer.
pub const POWER_SUPPLY_TECHNOLOGY_LIPO: u32 = 3;
/// Lithium-iron-phosphate.
pub const POWER_SUPPLY_TECHNOLOGY_LIFE: u32 = 4;
/// Nickel-Cadmium.
pub const POWER_SUPPLY_TECHNOLOGY_NICD: u32 = 5;
/// Lithium-manganese-nickel.
pub const POWER_SUPPLY_TECHNOLOGY_LIMN: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            POWER_SUPPLY_TYPE_UNKNOWN,
            POWER_SUPPLY_TYPE_BATTERY,
            POWER_SUPPLY_TYPE_UPS,
            POWER_SUPPLY_TYPE_MAINS,
            POWER_SUPPLY_TYPE_USB,
            POWER_SUPPLY_TYPE_USB_DCP,
            POWER_SUPPLY_TYPE_USB_CDP,
            POWER_SUPPLY_TYPE_USB_TYPE_C,
            POWER_SUPPLY_TYPE_USB_PD,
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
            POWER_SUPPLY_HEALTH_COLD,
            POWER_SUPPLY_HEALTH_OVERCURRENT,
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
}
