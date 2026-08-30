//! `<linux/efi.h>` — EFI/UEFI firmware interface constants.
//!
//! UEFI (Unified Extensible Firmware Interface) is the modern firmware
//! interface replacing legacy BIOS. It provides boot services (memory
//! allocation, protocol access, image loading) and runtime services
//! (variable storage, time, reset) that persist after the OS takes
//! over. EFI variables in NVRAM store boot configuration, Secure Boot
//! keys, and OEM data.

// ---------------------------------------------------------------------------
// EFI memory types
// ---------------------------------------------------------------------------

/// Reserved memory (firmware use).
pub const EFI_RESERVED_TYPE: u32 = 0;
/// EFI loader code.
pub const EFI_LOADER_CODE: u32 = 1;
/// EFI loader data.
pub const EFI_LOADER_DATA: u32 = 2;
/// EFI boot services code (reclaimable after ExitBootServices).
pub const EFI_BOOT_SERVICES_CODE: u32 = 3;
/// EFI boot services data (reclaimable after ExitBootServices).
pub const EFI_BOOT_SERVICES_DATA: u32 = 4;
/// EFI runtime services code (must be preserved).
pub const EFI_RUNTIME_SERVICES_CODE: u32 = 5;
/// EFI runtime services data (must be preserved).
pub const EFI_RUNTIME_SERVICES_DATA: u32 = 6;
/// Conventional memory (available for OS use).
pub const EFI_CONVENTIONAL_MEMORY: u32 = 7;
/// Unusable memory (hardware errors).
pub const EFI_UNUSABLE_MEMORY: u32 = 8;
/// ACPI reclaim memory (tables, reclaimable after parsing).
pub const EFI_ACPI_RECLAIM_MEMORY: u32 = 9;
/// ACPI NVS memory (firmware state, must be preserved).
pub const EFI_ACPI_MEMORY_NVS: u32 = 10;
/// Memory-mapped I/O.
pub const EFI_MEMORY_MAPPED_IO: u32 = 11;
/// Memory-mapped I/O port space.
pub const EFI_MEMORY_MAPPED_IO_PORT_SPACE: u32 = 12;
/// Processor-reserved (PAL code on Itanium).
pub const EFI_PAL_CODE: u32 = 13;
/// Persistent memory (NVDIMM).
pub const EFI_PERSISTENT_MEMORY: u32 = 14;

// ---------------------------------------------------------------------------
// EFI memory attribute bits
// ---------------------------------------------------------------------------

/// Uncacheable.
pub const EFI_MEMORY_UC: u64 = 1 << 0;
/// Write-combining.
pub const EFI_MEMORY_WC: u64 = 1 << 1;
/// Write-through.
pub const EFI_MEMORY_WT: u64 = 1 << 2;
/// Write-back.
pub const EFI_MEMORY_WB: u64 = 1 << 3;
/// Uncacheable, exported (non-posted).
pub const EFI_MEMORY_UCE: u64 = 1 << 4;
/// Write-protected.
pub const EFI_MEMORY_WP: u64 = 1 << 12;
/// Read-protected.
pub const EFI_MEMORY_RP: u64 = 1 << 13;
/// Execute-protected.
pub const EFI_MEMORY_XP: u64 = 1 << 14;
/// Non-volatile (persistent memory).
pub const EFI_MEMORY_NV: u64 = 1 << 15;
/// Runtime (accessible after ExitBootServices).
pub const EFI_MEMORY_RUNTIME: u64 = 1 << 63;

// ---------------------------------------------------------------------------
// EFI variable attributes
// ---------------------------------------------------------------------------

/// Variable is non-volatile (persists across reboot).
pub const EFI_VARIABLE_NON_VOLATILE: u32 = 0x01;
/// Variable accessible at boot services time.
pub const EFI_VARIABLE_BOOTSERVICE_ACCESS: u32 = 0x02;
/// Variable accessible at runtime.
pub const EFI_VARIABLE_RUNTIME_ACCESS: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_memory_attrs_no_overlap() {
        let attrs: [u64; 5] = [
            EFI_MEMORY_UC,
            EFI_MEMORY_WC,
            EFI_MEMORY_WT,
            EFI_MEMORY_WB,
            EFI_MEMORY_UCE,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_variable_attrs_no_overlap() {
        let attrs = [
            EFI_VARIABLE_NON_VOLATILE,
            EFI_VARIABLE_BOOTSERVICE_ACCESS,
            EFI_VARIABLE_RUNTIME_ACCESS,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }
}
