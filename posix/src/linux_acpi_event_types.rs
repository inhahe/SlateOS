//! `<linux/acpi.h>` — ACPI event class and type constants.
//!
//! ACPI generates events for power button presses, lid switches,
//! AC adapter state changes, battery notifications, and thermal
//! zone changes. The kernel routes these to userspace via netlink
//! or the /proc/acpi/event interface.

// ---------------------------------------------------------------------------
// ACPI event classes
// ---------------------------------------------------------------------------

/// Power button event class.
pub const ACPI_EVENT_CLASS_POWER: u32 = 0;
/// Sleep button event class.
pub const ACPI_EVENT_CLASS_SLEEP: u32 = 1;
/// Thermal zone event class.
pub const ACPI_EVENT_CLASS_THERMAL: u32 = 2;
/// AC adapter event class.
pub const ACPI_EVENT_CLASS_AC: u32 = 3;
/// Battery event class.
pub const ACPI_EVENT_CLASS_BATTERY: u32 = 4;
/// Lid switch event class.
pub const ACPI_EVENT_CLASS_LID: u32 = 5;
/// Processor event class.
pub const ACPI_EVENT_CLASS_PROCESSOR: u32 = 6;

// ---------------------------------------------------------------------------
// ACPI bus/device event types
// ---------------------------------------------------------------------------

/// Device check (hot-plug enumeration).
pub const ACPI_NOTIFY_BUS_CHECK: u32 = 0x00;
/// Device check (recheck device status).
pub const ACPI_NOTIFY_DEVICE_CHECK: u32 = 0x01;
/// Device wake (device signaled wake).
pub const ACPI_NOTIFY_DEVICE_WAKE: u32 = 0x02;
/// Eject request (user requested removal).
pub const ACPI_NOTIFY_EJECT_REQUEST: u32 = 0x03;
/// Device check light (device inserted).
pub const ACPI_NOTIFY_DEVICE_CHECK_LIGHT: u32 = 0x04;
/// Frequency mismatch.
pub const ACPI_NOTIFY_FREQUENCY_MISMATCH: u32 = 0x05;
/// Bus mode mismatch.
pub const ACPI_NOTIFY_BUS_MODE_MISMATCH: u32 = 0x06;
/// Power fault.
pub const ACPI_NOTIFY_POWER_FAULT: u32 = 0x07;

// ---------------------------------------------------------------------------
// ACPI battery notify types
// ---------------------------------------------------------------------------

/// Battery status change (charge level update).
pub const ACPI_BATTERY_NOTIFY_STATUS: u32 = 0x80;
/// Battery info change (design capacity, etc.).
pub const ACPI_BATTERY_NOTIFY_INFO: u32 = 0x81;
/// Battery threshold crossed.
pub const ACPI_BATTERY_NOTIFY_THRESHOLD: u32 = 0x82;

// ---------------------------------------------------------------------------
// ACPI thermal notify types
// ---------------------------------------------------------------------------

/// Thermal zone temperature changed.
pub const ACPI_THERMAL_NOTIFY_TEMPERATURE: u32 = 0x80;
/// Thermal zone trip points changed.
pub const ACPI_THERMAL_NOTIFY_THRESHOLDS: u32 = 0x81;
/// Thermal zone device lists changed.
pub const ACPI_THERMAL_NOTIFY_DEVICES: u32 = 0x82;
/// Critical temperature reached.
pub const ACPI_THERMAL_NOTIFY_CRITICAL: u32 = 0xCC;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_classes_distinct() {
        let classes = [
            ACPI_EVENT_CLASS_POWER,
            ACPI_EVENT_CLASS_SLEEP,
            ACPI_EVENT_CLASS_THERMAL,
            ACPI_EVENT_CLASS_AC,
            ACPI_EVENT_CLASS_BATTERY,
            ACPI_EVENT_CLASS_LID,
            ACPI_EVENT_CLASS_PROCESSOR,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_bus_events_distinct() {
        let evts = [
            ACPI_NOTIFY_BUS_CHECK,
            ACPI_NOTIFY_DEVICE_CHECK,
            ACPI_NOTIFY_DEVICE_WAKE,
            ACPI_NOTIFY_EJECT_REQUEST,
            ACPI_NOTIFY_DEVICE_CHECK_LIGHT,
            ACPI_NOTIFY_FREQUENCY_MISMATCH,
            ACPI_NOTIFY_BUS_MODE_MISMATCH,
            ACPI_NOTIFY_POWER_FAULT,
        ];
        for i in 0..evts.len() {
            for j in (i + 1)..evts.len() {
                assert_ne!(evts[i], evts[j]);
            }
        }
    }

    #[test]
    fn test_battery_notify_distinct() {
        assert_ne!(ACPI_BATTERY_NOTIFY_STATUS, ACPI_BATTERY_NOTIFY_INFO);
        assert_ne!(ACPI_BATTERY_NOTIFY_INFO, ACPI_BATTERY_NOTIFY_THRESHOLD);
        assert_ne!(ACPI_BATTERY_NOTIFY_STATUS, ACPI_BATTERY_NOTIFY_THRESHOLD);
    }

    #[test]
    fn test_thermal_notify_distinct() {
        let notifs = [
            ACPI_THERMAL_NOTIFY_TEMPERATURE,
            ACPI_THERMAL_NOTIFY_THRESHOLDS,
            ACPI_THERMAL_NOTIFY_DEVICES,
            ACPI_THERMAL_NOTIFY_CRITICAL,
        ];
        for i in 0..notifs.len() {
            for j in (i + 1)..notifs.len() {
                assert_ne!(notifs[i], notifs[j]);
            }
        }
    }

    #[test]
    fn test_critical_temp_value() {
        assert_eq!(ACPI_THERMAL_NOTIFY_CRITICAL, 0xCC);
    }
}
