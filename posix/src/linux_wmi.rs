//! `<linux/wmi.h>` — Windows Management Instrumentation (ACPI WMI).
//!
//! ACPI WMI provides a standardized interface for BIOS/firmware
//! features on x86 platforms. OEM-specific functionality (keyboard
//! backlight, thermal profiles, battery thresholds) is exposed
//! through WMI GUIDs. Linux maps these to sysfs attributes.

// ---------------------------------------------------------------------------
// WMI event types
// ---------------------------------------------------------------------------

/// Data block query.
pub const WMI_METHOD_DATA_QUERY: u32 = 0;
/// Data block set.
pub const WMI_METHOD_DATA_SET: u32 = 1;
/// Method call.
pub const WMI_METHOD_METHOD: u32 = 2;
/// Event notification.
pub const WMI_METHOD_EVENT: u32 = 3;

// ---------------------------------------------------------------------------
// WMI block flags
// ---------------------------------------------------------------------------

/// Block is expensive to read (cache results).
pub const WMI_ACPI_FLAG_EXPENSIVE: u32 = 1 << 0;
/// Block provides event notifications.
pub const WMI_ACPI_FLAG_EVENT: u32 = 1 << 1;
/// Block is a method.
pub const WMI_ACPI_FLAG_METHOD: u32 = 1 << 2;
/// Block is a string.
pub const WMI_ACPI_FLAG_STRING: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// WMI MOF (Managed Object Format) data types
// ---------------------------------------------------------------------------

/// Boolean.
pub const WMI_MOF_BOOLEAN: u32 = 0;
/// Signed 8-bit.
pub const WMI_MOF_SINT8: u32 = 1;
/// Unsigned 8-bit.
pub const WMI_MOF_UINT8: u32 = 2;
/// Signed 16-bit.
pub const WMI_MOF_SINT16: u32 = 3;
/// Unsigned 16-bit.
pub const WMI_MOF_UINT16: u32 = 4;
/// Signed 32-bit.
pub const WMI_MOF_SINT32: u32 = 5;
/// Unsigned 32-bit.
pub const WMI_MOF_UINT32: u32 = 6;
/// Signed 64-bit.
pub const WMI_MOF_SINT64: u32 = 7;
/// Unsigned 64-bit.
pub const WMI_MOF_UINT64: u32 = 8;
/// String.
pub const WMI_MOF_STRING: u32 = 9;

// ---------------------------------------------------------------------------
// GUID size
// ---------------------------------------------------------------------------

/// WMI GUID size in bytes.
pub const WMI_GUID_SIZE: usize = 16;
/// WMI GUID string length (with hyphens, no braces).
pub const WMI_GUID_STRING_LEN: usize = 36;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_methods_distinct() {
        let methods = [
            WMI_METHOD_DATA_QUERY,
            WMI_METHOD_DATA_SET,
            WMI_METHOD_METHOD,
            WMI_METHOD_EVENT,
        ];
        for i in 0..methods.len() {
            for j in (i + 1)..methods.len() {
                assert_ne!(methods[i], methods[j]);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            WMI_ACPI_FLAG_EXPENSIVE,
            WMI_ACPI_FLAG_EVENT,
            WMI_ACPI_FLAG_METHOD,
            WMI_ACPI_FLAG_STRING,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            WMI_ACPI_FLAG_EXPENSIVE,
            WMI_ACPI_FLAG_EVENT,
            WMI_ACPI_FLAG_METHOD,
            WMI_ACPI_FLAG_STRING,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mof_types_distinct() {
        let types = [
            WMI_MOF_BOOLEAN,
            WMI_MOF_SINT8,
            WMI_MOF_UINT8,
            WMI_MOF_SINT16,
            WMI_MOF_UINT16,
            WMI_MOF_SINT32,
            WMI_MOF_UINT32,
            WMI_MOF_SINT64,
            WMI_MOF_UINT64,
            WMI_MOF_STRING,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_guid_size() {
        assert_eq!(WMI_GUID_SIZE, 16);
        assert_eq!(WMI_GUID_STRING_LEN, 36);
    }
}
