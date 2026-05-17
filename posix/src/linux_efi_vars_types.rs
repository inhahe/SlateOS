//! `<linux/efi.h>` — EFI variable access constants.
//!
//! EFI (UEFI) variables are stored in firmware NVRAM and accessed via
//! /sys/firmware/efi/efivars/ or the efivarfs filesystem. They store
//! boot configuration, Secure Boot keys, and platform-specific data.
//! These constants define variable attributes and well-known GUIDs.

// ---------------------------------------------------------------------------
// EFI variable attributes
// ---------------------------------------------------------------------------

/// Variable is accessible at boot services time.
pub const EFI_VARIABLE_NON_VOLATILE: u32 = 0x0000_0001;
/// Variable is accessible at boot services time.
pub const EFI_VARIABLE_BOOTSERVICE_ACCESS: u32 = 0x0000_0002;
/// Variable is accessible at runtime.
pub const EFI_VARIABLE_RUNTIME_ACCESS: u32 = 0x0000_0004;
/// Hardware error record.
pub const EFI_VARIABLE_HARDWARE_ERROR_RECORD: u32 = 0x0000_0008;
/// Authenticated write access (time-based, signing required).
pub const EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS: u32 = 0x0000_0020;
/// Append write (append to existing data, don't replace).
pub const EFI_VARIABLE_APPEND_WRITE: u32 = 0x0000_0040;
/// Enhanced authenticated access.
pub const EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// Common attribute combinations
// ---------------------------------------------------------------------------

/// Typical NV+BS+RT attributes (most boot variables).
pub const EFI_VARIABLE_DEFAULT_ATTRS: u32 = EFI_VARIABLE_NON_VOLATILE
    | EFI_VARIABLE_BOOTSERVICE_ACCESS
    | EFI_VARIABLE_RUNTIME_ACCESS;

// ---------------------------------------------------------------------------
// Well-known EFI variable names
// ---------------------------------------------------------------------------

/// Boot order variable name.
pub const EFI_VAR_BOOT_ORDER: &str = "BootOrder";
/// Boot current variable name.
pub const EFI_VAR_BOOT_CURRENT: &str = "BootCurrent";
/// Boot next variable name.
pub const EFI_VAR_BOOT_NEXT: &str = "BootNext";
/// Secure Boot enable flag.
pub const EFI_VAR_SECURE_BOOT: &str = "SecureBoot";
/// Setup mode flag.
pub const EFI_VAR_SETUP_MODE: &str = "SetupMode";

// ---------------------------------------------------------------------------
// efivarfs ioctl
// ---------------------------------------------------------------------------

/// Delete an EFI variable.
pub const EFI_VAR_DELETE: u32 = 0;

// ---------------------------------------------------------------------------
// EFI GUID size
// ---------------------------------------------------------------------------

/// Size of an EFI GUID in bytes.
pub const EFI_GUID_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attributes_no_overlap() {
        let attrs = [
            EFI_VARIABLE_NON_VOLATILE,
            EFI_VARIABLE_BOOTSERVICE_ACCESS,
            EFI_VARIABLE_RUNTIME_ACCESS,
            EFI_VARIABLE_HARDWARE_ERROR_RECORD,
            EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS,
            EFI_VARIABLE_APPEND_WRITE,
            EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_default_attrs() {
        assert_eq!(
            EFI_VARIABLE_DEFAULT_ATTRS,
            EFI_VARIABLE_NON_VOLATILE
                | EFI_VARIABLE_BOOTSERVICE_ACCESS
                | EFI_VARIABLE_RUNTIME_ACCESS
        );
    }

    #[test]
    fn test_var_names_distinct() {
        let names = [
            EFI_VAR_BOOT_ORDER, EFI_VAR_BOOT_CURRENT,
            EFI_VAR_BOOT_NEXT, EFI_VAR_SECURE_BOOT,
            EFI_VAR_SETUP_MODE,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_guid_size() {
        assert_eq!(EFI_GUID_SIZE, 16);
    }
}
