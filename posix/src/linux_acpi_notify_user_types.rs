//! ACPI `_Lxx` / `_Exx` GPE and `Notify(...)` opcode values.
//!
//! When firmware fires a Notify on an ACPI device, the kernel routes
//! it to the corresponding `acpi_driver` and to userspace via the
//! `acpi_event` netlink family. The numeric codes are defined in
//! ACPI 6.5 §5.6.5 — *Device Object Notification Values*.

// ---------------------------------------------------------------------------
// Notify-value categories (from acpi/actypes.h)
// ---------------------------------------------------------------------------

pub const ACPI_NOTIFY_STANDARD_FIRST: u32 = 0x00;
pub const ACPI_NOTIFY_STANDARD_LAST: u32 = 0x7F;
pub const ACPI_NOTIFY_DEVICE_FIRST: u32 = 0x80;
pub const ACPI_NOTIFY_DEVICE_LAST: u32 = 0xBF;
pub const ACPI_NOTIFY_HARDWARE_FIRST: u32 = 0xC0;
pub const ACPI_NOTIFY_HARDWARE_LAST: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Common standard Notify values (§5.6.6 of ACPI 6.5)
// ---------------------------------------------------------------------------

pub const ACPI_NOTIFY_BUS_CHECK: u32 = 0x00;
pub const ACPI_NOTIFY_DEVICE_CHECK: u32 = 0x01;
pub const ACPI_NOTIFY_DEVICE_WAKE: u32 = 0x02;
pub const ACPI_NOTIFY_EJECT_REQUEST: u32 = 0x03;
pub const ACPI_NOTIFY_DEVICE_CHECK_LIGHT: u32 = 0x04;
pub const ACPI_NOTIFY_FREQUENCY_MISMATCH: u32 = 0x05;
pub const ACPI_NOTIFY_BUS_MODE_MISMATCH: u32 = 0x06;
pub const ACPI_NOTIFY_POWER_FAULT: u32 = 0x07;
pub const ACPI_NOTIFY_CAPABILITIES_CHECK: u32 = 0x08;

// ---------------------------------------------------------------------------
// Device-specific notify subsets
// ---------------------------------------------------------------------------

/// Battery and AC adapter device-specific notify values.
pub const ACPI_NOTIFY_BATTERY_STATUS: u32 = 0x80;
pub const ACPI_NOTIFY_BATTERY_INFORMATION: u32 = 0x81;
pub const ACPI_NOTIFY_BATTERY_DEVICE_CHECK: u32 = 0x82;
pub const ACPI_NOTIFY_AC_STATUS: u32 = 0x80;

/// Thermal-zone notify values.
pub const ACPI_NOTIFY_THERMAL_TEMP_CHANGED: u32 = 0x80;
pub const ACPI_NOTIFY_THERMAL_TRIP_POINTS_CHANGED: u32 = 0x81;
pub const ACPI_NOTIFY_THERMAL_DEVICE_LISTS_CHANGED: u32 = 0x82;
pub const ACPI_NOTIFY_THERMAL_RELATIONSHIP_CHANGED: u32 = 0x83;

/// Video notify values (ACPI 6.5 §B.6.1).
pub const ACPI_NOTIFY_VIDEO_CYCLE_OUTPUT: u32 = 0x80;
pub const ACPI_NOTIFY_VIDEO_NEXT_OUTPUT: u32 = 0x81;
pub const ACPI_NOTIFY_VIDEO_PREV_OUTPUT: u32 = 0x82;
pub const ACPI_NOTIFY_VIDEO_BRIGHTNESS_CYCLE: u32 = 0x85;
pub const ACPI_NOTIFY_VIDEO_BRIGHTNESS_INC: u32 = 0x86;
pub const ACPI_NOTIFY_VIDEO_BRIGHTNESS_DEC: u32 = 0x87;
pub const ACPI_NOTIFY_VIDEO_BRIGHTNESS_ZERO: u32 = 0x88;

// ---------------------------------------------------------------------------
// GPE register block IDs
// ---------------------------------------------------------------------------

pub const ACPI_GPE_REG_GPE0: u32 = 0;
pub const ACPI_GPE_REG_GPE1: u32 = 1;

// ---------------------------------------------------------------------------
// GPE handler `wake/run` types
// ---------------------------------------------------------------------------

pub const ACPI_GPE_TYPE_WAKE: u8 = 0x01;
pub const ACPI_GPE_TYPE_RUNTIME: u8 = 0x02;
pub const ACPI_GPE_TYPE_WAKE_RUN: u8 = ACPI_GPE_TYPE_WAKE | ACPI_GPE_TYPE_RUNTIME;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_ranges_partition_byte() {
        // The three ranges cover 0..256 exactly.
        assert_eq!(ACPI_NOTIFY_STANDARD_FIRST, 0);
        assert_eq!(ACPI_NOTIFY_STANDARD_LAST + 1, ACPI_NOTIFY_DEVICE_FIRST);
        assert_eq!(ACPI_NOTIFY_DEVICE_LAST + 1, ACPI_NOTIFY_HARDWARE_FIRST);
        assert_eq!(ACPI_NOTIFY_HARDWARE_LAST, 0xFF);
    }

    #[test]
    fn test_standard_codes_dense_0_to_8() {
        let s = [
            ACPI_NOTIFY_BUS_CHECK,
            ACPI_NOTIFY_DEVICE_CHECK,
            ACPI_NOTIFY_DEVICE_WAKE,
            ACPI_NOTIFY_EJECT_REQUEST,
            ACPI_NOTIFY_DEVICE_CHECK_LIGHT,
            ACPI_NOTIFY_FREQUENCY_MISMATCH,
            ACPI_NOTIFY_BUS_MODE_MISMATCH,
            ACPI_NOTIFY_POWER_FAULT,
            ACPI_NOTIFY_CAPABILITIES_CHECK,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
            // All standard codes live in the standard range.
            assert!(v <= ACPI_NOTIFY_STANDARD_LAST);
        }
    }

    #[test]
    fn test_battery_thermal_video_in_device_range() {
        let d = [
            ACPI_NOTIFY_BATTERY_STATUS,
            ACPI_NOTIFY_BATTERY_INFORMATION,
            ACPI_NOTIFY_BATTERY_DEVICE_CHECK,
            ACPI_NOTIFY_THERMAL_TEMP_CHANGED,
            ACPI_NOTIFY_THERMAL_TRIP_POINTS_CHANGED,
            ACPI_NOTIFY_THERMAL_DEVICE_LISTS_CHANGED,
            ACPI_NOTIFY_THERMAL_RELATIONSHIP_CHANGED,
            ACPI_NOTIFY_VIDEO_CYCLE_OUTPUT,
            ACPI_NOTIFY_VIDEO_NEXT_OUTPUT,
            ACPI_NOTIFY_VIDEO_PREV_OUTPUT,
            ACPI_NOTIFY_VIDEO_BRIGHTNESS_CYCLE,
            ACPI_NOTIFY_VIDEO_BRIGHTNESS_INC,
            ACPI_NOTIFY_VIDEO_BRIGHTNESS_DEC,
            ACPI_NOTIFY_VIDEO_BRIGHTNESS_ZERO,
        ];
        for v in d {
            assert!(v >= ACPI_NOTIFY_DEVICE_FIRST);
            assert!(v <= ACPI_NOTIFY_DEVICE_LAST);
        }
    }

    #[test]
    fn test_gpe_register_blocks_dense() {
        assert_eq!(ACPI_GPE_REG_GPE0, 0);
        assert_eq!(ACPI_GPE_REG_GPE1, 1);
    }

    #[test]
    fn test_gpe_type_flags_combine() {
        // WAKE and RUNTIME are single bits; WAKE_RUN is their OR.
        assert!(ACPI_GPE_TYPE_WAKE.is_power_of_two());
        assert!(ACPI_GPE_TYPE_RUNTIME.is_power_of_two());
        assert_eq!(
            ACPI_GPE_TYPE_WAKE_RUN,
            ACPI_GPE_TYPE_WAKE | ACPI_GPE_TYPE_RUNTIME
        );
        assert_eq!(ACPI_GPE_TYPE_WAKE & ACPI_GPE_TYPE_RUNTIME, 0);
    }
}
