//! `<linux/power_supply.h>` — Battery status and property constants.
//!
//! These constants define battery-specific status codes, health
//! indicators, and technology identifiers reported through the
//! power_supply subsystem in sysfs.

// ---------------------------------------------------------------------------
// Battery status values
// ---------------------------------------------------------------------------

/// Unknown battery status.
pub const POWER_SUPPLY_STATUS_UNKNOWN: u32 = 0;
/// Battery is charging.
pub const POWER_SUPPLY_STATUS_CHARGING: u32 = 1;
/// Battery is discharging.
pub const POWER_SUPPLY_STATUS_DISCHARGING: u32 = 2;
/// Battery is not charging (plugged in but full or paused).
pub const POWER_SUPPLY_STATUS_NOT_CHARGING: u32 = 3;
/// Battery is full.
pub const POWER_SUPPLY_STATUS_FULL: u32 = 4;

// ---------------------------------------------------------------------------
// Battery health values
// ---------------------------------------------------------------------------

/// Unknown health.
pub const POWER_SUPPLY_HEALTH_UNKNOWN: u32 = 0;
/// Battery is in good condition.
pub const POWER_SUPPLY_HEALTH_GOOD: u32 = 1;
/// Battery is overheated.
pub const POWER_SUPPLY_HEALTH_OVERHEAT: u32 = 2;
/// Battery is dead (won't charge).
pub const POWER_SUPPLY_HEALTH_DEAD: u32 = 3;
/// Battery voltage is too high.
pub const POWER_SUPPLY_HEALTH_OVERVOLTAGE: u32 = 4;
/// Unspecified battery failure.
pub const POWER_SUPPLY_HEALTH_UNSPEC_FAILURE: u32 = 5;
/// Battery is cold (below operating temp).
pub const POWER_SUPPLY_HEALTH_COLD: u32 = 6;
/// Watchdog timer expired.
pub const POWER_SUPPLY_HEALTH_WATCHDOG_TIMER_EXPIRE: u32 = 7;
/// Safety timer expired.
pub const POWER_SUPPLY_HEALTH_SAFETY_TIMER_EXPIRE: u32 = 8;
/// Overcurrent condition.
pub const POWER_SUPPLY_HEALTH_OVERCURRENT: u32 = 9;
/// Battery is calibrating.
pub const POWER_SUPPLY_HEALTH_CALIBRATION_REQUIRED: u32 = 10;
/// Battery is warm (near limit).
pub const POWER_SUPPLY_HEALTH_WARM: u32 = 11;
/// Battery is cool (near limit).
pub const POWER_SUPPLY_HEALTH_COOL: u32 = 12;
/// Battery is hot (above limit).
pub const POWER_SUPPLY_HEALTH_HOT: u32 = 13;

// ---------------------------------------------------------------------------
// Battery technology
// ---------------------------------------------------------------------------

/// Unknown technology.
pub const POWER_SUPPLY_TECHNOLOGY_UNKNOWN: u32 = 0;
/// Nickel-Metal Hydride.
pub const POWER_SUPPLY_TECHNOLOGY_NIMH: u32 = 1;
/// Lithium-Ion.
pub const POWER_SUPPLY_TECHNOLOGY_LION: u32 = 2;
/// Lithium-Polymer.
pub const POWER_SUPPLY_TECHNOLOGY_LIPO: u32 = 3;
/// Lithium-Iron-Phosphate.
pub const POWER_SUPPLY_TECHNOLOGY_LIFE: u32 = 4;
/// Nickel-Cadmium.
pub const POWER_SUPPLY_TECHNOLOGY_NICD: u32 = 5;
/// Lithium-Manganese-Oxide.
pub const POWER_SUPPLY_TECHNOLOGY_LIMN: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let health = [
            POWER_SUPPLY_HEALTH_UNKNOWN, POWER_SUPPLY_HEALTH_GOOD,
            POWER_SUPPLY_HEALTH_OVERHEAT, POWER_SUPPLY_HEALTH_DEAD,
            POWER_SUPPLY_HEALTH_OVERVOLTAGE, POWER_SUPPLY_HEALTH_UNSPEC_FAILURE,
            POWER_SUPPLY_HEALTH_COLD, POWER_SUPPLY_HEALTH_WATCHDOG_TIMER_EXPIRE,
            POWER_SUPPLY_HEALTH_SAFETY_TIMER_EXPIRE,
            POWER_SUPPLY_HEALTH_OVERCURRENT,
            POWER_SUPPLY_HEALTH_CALIBRATION_REQUIRED,
            POWER_SUPPLY_HEALTH_WARM, POWER_SUPPLY_HEALTH_COOL,
            POWER_SUPPLY_HEALTH_HOT,
        ];
        for i in 0..health.len() {
            for j in (i + 1)..health.len() {
                assert_ne!(health[i], health[j]);
            }
        }
    }

    #[test]
    fn test_technology_distinct() {
        let techs = [
            POWER_SUPPLY_TECHNOLOGY_UNKNOWN, POWER_SUPPLY_TECHNOLOGY_NIMH,
            POWER_SUPPLY_TECHNOLOGY_LION, POWER_SUPPLY_TECHNOLOGY_LIPO,
            POWER_SUPPLY_TECHNOLOGY_LIFE, POWER_SUPPLY_TECHNOLOGY_NICD,
            POWER_SUPPLY_TECHNOLOGY_LIMN,
        ];
        for i in 0..techs.len() {
            for j in (i + 1)..techs.len() {
                assert_ne!(techs[i], techs[j]);
            }
        }
    }

    #[test]
    fn test_unknown_is_zero() {
        assert_eq!(POWER_SUPPLY_STATUS_UNKNOWN, 0);
        assert_eq!(POWER_SUPPLY_HEALTH_UNKNOWN, 0);
        assert_eq!(POWER_SUPPLY_TECHNOLOGY_UNKNOWN, 0);
    }
}
