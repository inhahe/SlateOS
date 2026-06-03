//! `<linux/efi.h>` — EFI/UEFI constants and GUIDs.
//!
//! The Extensible Firmware Interface (EFI/UEFI) provides the boot
//! firmware interface on modern x86_64 and ARM systems. This module
//! defines EFI variable attributes, memory types, table GUIDs, and
//! status codes used by the kernel's EFI runtime services interface.

// ---------------------------------------------------------------------------
// EFI variable attributes
// ---------------------------------------------------------------------------

/// Non-volatile variable.
pub const EFI_VARIABLE_NON_VOLATILE: u32 = 0x0000_0001;
/// Accessible at boot services.
pub const EFI_VARIABLE_BOOTSERVICE_ACCESS: u32 = 0x0000_0002;
/// Accessible at runtime services.
pub const EFI_VARIABLE_RUNTIME_ACCESS: u32 = 0x0000_0004;
/// Hardware error record.
pub const EFI_VARIABLE_HARDWARE_ERROR_RECORD: u32 = 0x0000_0008;
/// Authenticated write access (deprecated).
pub const EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS: u32 = 0x0000_0010;
/// Time-based authenticated write.
pub const EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS: u32 = 0x0000_0020;
/// Append write.
pub const EFI_VARIABLE_APPEND_WRITE: u32 = 0x0000_0040;
/// Enhanced authenticated access.
pub const EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS: u32 = 0x0000_0080;

/// Common mask for boot+runtime variables.
pub const EFI_VARIABLE_MASK: u32 = EFI_VARIABLE_NON_VOLATILE
    | EFI_VARIABLE_BOOTSERVICE_ACCESS
    | EFI_VARIABLE_RUNTIME_ACCESS
    | EFI_VARIABLE_HARDWARE_ERROR_RECORD
    | EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS
    | EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS
    | EFI_VARIABLE_APPEND_WRITE
    | EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS;

// ---------------------------------------------------------------------------
// EFI memory types
// ---------------------------------------------------------------------------

/// Reserved memory.
pub const EFI_RESERVED_TYPE: u32 = 0;
/// Loader code.
pub const EFI_LOADER_CODE: u32 = 1;
/// Loader data.
pub const EFI_LOADER_DATA: u32 = 2;
/// Boot services code.
pub const EFI_BOOT_SERVICES_CODE: u32 = 3;
/// Boot services data.
pub const EFI_BOOT_SERVICES_DATA: u32 = 4;
/// Runtime services code.
pub const EFI_RUNTIME_SERVICES_CODE: u32 = 5;
/// Runtime services data.
pub const EFI_RUNTIME_SERVICES_DATA: u32 = 6;
/// Conventional (usable) memory.
pub const EFI_CONVENTIONAL_MEMORY: u32 = 7;
/// Unusable memory.
pub const EFI_UNUSABLE_MEMORY: u32 = 8;
/// ACPI reclaim memory.
pub const EFI_ACPI_RECLAIM_MEMORY: u32 = 9;
/// ACPI memory NVS.
pub const EFI_ACPI_MEMORY_NVS: u32 = 10;
/// Memory-mapped I/O.
pub const EFI_MEMORY_MAPPED_IO: u32 = 11;
/// Memory-mapped I/O port space.
pub const EFI_MEMORY_MAPPED_IO_PORT_SPACE: u32 = 12;
/// PAL code.
pub const EFI_PAL_CODE: u32 = 13;
/// Persistent memory.
pub const EFI_PERSISTENT_MEMORY: u32 = 14;
/// Unaccepted memory (TDX/SEV-SNP).
pub const EFI_UNACCEPTED_MEMORY: u32 = 15;
/// Maximum memory type.
pub const EFI_MAX_MEMORY_TYPE: u32 = 16;

// ---------------------------------------------------------------------------
// EFI memory attribute flags
// ---------------------------------------------------------------------------

/// Uncacheable.
pub const EFI_MEMORY_UC: u64 = 1 << 0;
/// Write-combining.
pub const EFI_MEMORY_WC: u64 = 1 << 1;
/// Write-through.
pub const EFI_MEMORY_WT: u64 = 1 << 2;
/// Write-back.
pub const EFI_MEMORY_WB: u64 = 1 << 3;
/// Uncacheable, exported.
pub const EFI_MEMORY_UCE: u64 = 1 << 4;
/// Write-protected.
pub const EFI_MEMORY_WP: u64 = 1 << 12;
/// Read-protected.
pub const EFI_MEMORY_RP: u64 = 1 << 13;
/// Execute-protected.
pub const EFI_MEMORY_XP: u64 = 1 << 14;
/// Non-volatile.
pub const EFI_MEMORY_NV: u64 = 1 << 15;
/// Higher reliability.
pub const EFI_MEMORY_MORE_RELIABLE: u64 = 1 << 16;
/// Read-only.
pub const EFI_MEMORY_RO: u64 = 1 << 17;
/// Specific-purpose memory.
pub const EFI_MEMORY_SP: u64 = 1 << 18;
/// Crypto-capable.
pub const EFI_MEMORY_CPU_CRYPTO: u64 = 1 << 19;
/// Runtime mapping required.
pub const EFI_MEMORY_RUNTIME: u64 = 1 << 63;

// ---------------------------------------------------------------------------
// EFI status codes
// ---------------------------------------------------------------------------

/// Success.
pub const EFI_SUCCESS: u64 = 0;
/// Invalid parameter.
pub const EFI_INVALID_PARAMETER: u64 = (1 << 63) | 2;
/// Unsupported.
pub const EFI_UNSUPPORTED: u64 = (1 << 63) | 3;
/// Buffer too small.
pub const EFI_BUFFER_TOO_SMALL: u64 = (1 << 63) | 5;
/// Not ready.
pub const EFI_NOT_READY: u64 = (1 << 63) | 6;
/// Device error.
pub const EFI_DEVICE_ERROR: u64 = (1 << 63) | 7;
/// Write protected.
pub const EFI_WRITE_PROTECTED: u64 = (1 << 63) | 8;
/// Out of resources.
pub const EFI_OUT_OF_RESOURCES: u64 = (1 << 63) | 9;
/// Not found.
pub const EFI_NOT_FOUND: u64 = (1 << 63) | 14;
/// Security violation.
pub const EFI_SECURITY_VIOLATION: u64 = (1 << 63) | 26;

// ---------------------------------------------------------------------------
// EFI reset types
// ---------------------------------------------------------------------------

/// Cold reset.
pub const EFI_RESET_COLD: u32 = 0;
/// Warm reset.
pub const EFI_RESET_WARM: u32 = 1;
/// Shutdown.
pub const EFI_RESET_SHUTDOWN: u32 = 2;
/// Platform-specific reset.
pub const EFI_RESET_PLATFORM_SPECIFIC: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_attrs_powers_of_two() {
        let attrs = [
            EFI_VARIABLE_NON_VOLATILE,
            EFI_VARIABLE_BOOTSERVICE_ACCESS,
            EFI_VARIABLE_RUNTIME_ACCESS,
            EFI_VARIABLE_HARDWARE_ERROR_RECORD,
            EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS,
            EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS,
            EFI_VARIABLE_APPEND_WRITE,
            EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS,
        ];
        for attr in &attrs {
            assert!(attr.is_power_of_two(), "0x{:x}", attr);
        }
    }

    #[test]
    fn test_variable_attrs_no_overlap() {
        let attrs = [
            EFI_VARIABLE_NON_VOLATILE,
            EFI_VARIABLE_BOOTSERVICE_ACCESS,
            EFI_VARIABLE_RUNTIME_ACCESS,
            EFI_VARIABLE_HARDWARE_ERROR_RECORD,
            EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS,
            EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS,
            EFI_VARIABLE_APPEND_WRITE,
            EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_variable_mask() {
        assert_eq!(EFI_VARIABLE_MASK, 0xFF);
    }

    #[test]
    fn test_memory_types_distinct() {
        let types = [
            EFI_RESERVED_TYPE,
            EFI_LOADER_CODE,
            EFI_LOADER_DATA,
            EFI_BOOT_SERVICES_CODE,
            EFI_BOOT_SERVICES_DATA,
            EFI_RUNTIME_SERVICES_CODE,
            EFI_RUNTIME_SERVICES_DATA,
            EFI_CONVENTIONAL_MEMORY,
            EFI_UNUSABLE_MEMORY,
            EFI_ACPI_RECLAIM_MEMORY,
            EFI_ACPI_MEMORY_NVS,
            EFI_MEMORY_MAPPED_IO,
            EFI_MEMORY_MAPPED_IO_PORT_SPACE,
            EFI_PAL_CODE,
            EFI_PERSISTENT_MEMORY,
            EFI_UNACCEPTED_MEMORY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_max_memory_type() {
        assert_eq!(EFI_MAX_MEMORY_TYPE, 16);
    }

    #[test]
    fn test_memory_cache_attrs_powers_of_two() {
        let attrs: [u64; 5] = [
            EFI_MEMORY_UC,
            EFI_MEMORY_WC,
            EFI_MEMORY_WT,
            EFI_MEMORY_WB,
            EFI_MEMORY_UCE,
        ];
        for attr in &attrs {
            assert!(attr.is_power_of_two(), "0x{:x}", attr);
        }
    }

    #[test]
    fn test_memory_prot_attrs_powers_of_two() {
        let attrs: [u64; 5] = [
            EFI_MEMORY_WP,
            EFI_MEMORY_RP,
            EFI_MEMORY_XP,
            EFI_MEMORY_NV,
            EFI_MEMORY_MORE_RELIABLE,
        ];
        for attr in &attrs {
            assert!(attr.is_power_of_two(), "0x{:x}", attr);
        }
    }

    #[test]
    fn test_memory_runtime_is_high_bit() {
        assert_eq!(EFI_MEMORY_RUNTIME, 1u64 << 63);
    }

    #[test]
    fn test_status_success() {
        assert_eq!(EFI_SUCCESS, 0);
    }

    #[test]
    fn test_error_codes_have_high_bit() {
        let errors = [
            EFI_INVALID_PARAMETER,
            EFI_UNSUPPORTED,
            EFI_BUFFER_TOO_SMALL,
            EFI_NOT_READY,
            EFI_DEVICE_ERROR,
            EFI_WRITE_PROTECTED,
            EFI_OUT_OF_RESOURCES,
            EFI_NOT_FOUND,
            EFI_SECURITY_VIOLATION,
        ];
        for err in &errors {
            assert_ne!(*err & (1u64 << 63), 0, "0x{:x}", err);
        }
    }

    #[test]
    fn test_error_codes_distinct() {
        let errors = [
            EFI_INVALID_PARAMETER,
            EFI_UNSUPPORTED,
            EFI_BUFFER_TOO_SMALL,
            EFI_NOT_READY,
            EFI_DEVICE_ERROR,
            EFI_WRITE_PROTECTED,
            EFI_OUT_OF_RESOURCES,
            EFI_NOT_FOUND,
            EFI_SECURITY_VIOLATION,
        ];
        for i in 0..errors.len() {
            for j in (i + 1)..errors.len() {
                assert_ne!(errors[i], errors[j]);
            }
        }
    }

    #[test]
    fn test_reset_types_distinct() {
        let types = [
            EFI_RESET_COLD,
            EFI_RESET_WARM,
            EFI_RESET_SHUTDOWN,
            EFI_RESET_PLATFORM_SPECIFIC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
