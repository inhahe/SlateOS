//! ACPI table override and dynamic loading interfaces.
//!
//! Linux lets users override or supplement firmware ACPI tables at
//! boot time via the initrd (`acpi_table_override`) and at runtime via
//! `/sys/firmware/acpi/tables/dynamic/`. Useful for adding SSDTs that
//! patch buggy firmware (laptops, embedded boards).

// ---------------------------------------------------------------------------
// Initrd override path (relative to cpio root)
// ---------------------------------------------------------------------------

pub const ACPI_INITRD_OVERRIDE_PREFIX: &str = "kernel/firmware/acpi";

// ---------------------------------------------------------------------------
// Sysfs dynamic-table loading
// ---------------------------------------------------------------------------

pub const SYS_FIRMWARE_ACPI_TABLES_DYNAMIC: &str = "/sys/firmware/acpi/tables/dynamic";

// ---------------------------------------------------------------------------
// `acpi_table_*` kernel command-line parameters
// ---------------------------------------------------------------------------

pub const CMDLINE_ACPI_NO_TABLE_OVERRIDE: &str = "acpi=no_table_override";
pub const CMDLINE_ACPI_RSDP: &str = "acpi_rsdp=";
pub const CMDLINE_ACPI_TABLE_PARSE_DEBUG: &str = "acpi_table_parse_debug";
pub const CMDLINE_ACPI_OVERRIDE: &str = "acpi_override";

// ---------------------------------------------------------------------------
// Table override modes (`enum`)
// ---------------------------------------------------------------------------

pub const ACPI_TABLE_OVERRIDE_NONE: u8 = 0;
pub const ACPI_TABLE_OVERRIDE_INITRD: u8 = 1;
pub const ACPI_TABLE_OVERRIDE_OEMID: u8 = 2;

// ---------------------------------------------------------------------------
// ACPICA error codes returned by table-parsing helpers (`ACPI_STATUS`)
// ---------------------------------------------------------------------------

pub const ACPI_AE_OK: u32 = 0x0000;
pub const ACPI_AE_ERROR: u32 = 0x0001;
pub const ACPI_AE_NO_ACPI_TABLES: u32 = 0x0002;
pub const ACPI_AE_NO_NAMESPACE: u32 = 0x0003;
pub const ACPI_AE_NO_MEMORY: u32 = 0x0004;
pub const ACPI_AE_NOT_FOUND: u32 = 0x0005;
pub const ACPI_AE_BAD_SIGNATURE: u32 = 0x0006;
pub const ACPI_AE_BAD_HEADER: u32 = 0x0007;
pub const ACPI_AE_BAD_CHECKSUM: u32 = 0x0008;
pub const ACPI_AE_BAD_PARAMETER: u32 = 0x0009;

// ---------------------------------------------------------------------------
// Limits enforced by the kernel parser
// ---------------------------------------------------------------------------

/// `MAX_ACPI_TABLES` historically — the table-tree array bound.
pub const ACPI_MAX_TABLES: usize = 128;

/// Largest individual table the kernel accepts (4 MiB safety cap).
pub const ACPI_MAX_TABLE_BYTES: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initrd_prefix() {
        assert_eq!(ACPI_INITRD_OVERRIDE_PREFIX, "kernel/firmware/acpi");
        assert!(!ACPI_INITRD_OVERRIDE_PREFIX.starts_with('/'));
    }

    #[test]
    fn test_dynamic_sysfs_path() {
        assert_eq!(
            SYS_FIRMWARE_ACPI_TABLES_DYNAMIC,
            "/sys/firmware/acpi/tables/dynamic"
        );
        assert!(SYS_FIRMWARE_ACPI_TABLES_DYNAMIC.starts_with("/sys/firmware/acpi/"));
    }

    #[test]
    fn test_cmdline_params_acpi_prefix() {
        let c = [
            CMDLINE_ACPI_NO_TABLE_OVERRIDE,
            CMDLINE_ACPI_RSDP,
            CMDLINE_ACPI_TABLE_PARSE_DEBUG,
            CMDLINE_ACPI_OVERRIDE,
        ];
        for s in c {
            assert!(s.starts_with("acpi"));
        }
    }

    #[test]
    fn test_override_modes_dense_0_to_2() {
        let m = [
            ACPI_TABLE_OVERRIDE_NONE,
            ACPI_TABLE_OVERRIDE_INITRD,
            ACPI_TABLE_OVERRIDE_OEMID,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_acpica_status_codes_dense() {
        let s = [
            ACPI_AE_OK,
            ACPI_AE_ERROR,
            ACPI_AE_NO_ACPI_TABLES,
            ACPI_AE_NO_NAMESPACE,
            ACPI_AE_NO_MEMORY,
            ACPI_AE_NOT_FOUND,
            ACPI_AE_BAD_SIGNATURE,
            ACPI_AE_BAD_HEADER,
            ACPI_AE_BAD_CHECKSUM,
            ACPI_AE_BAD_PARAMETER,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // OK is the only success code; everything else is non-zero.
        assert_eq!(ACPI_AE_OK, 0);
    }

    #[test]
    fn test_kernel_limits() {
        // 128 entries — historical MAX_ACPI_TABLES cap.
        assert_eq!(ACPI_MAX_TABLES, 128);
        assert!(ACPI_MAX_TABLES.is_power_of_two());
        // 4 MiB max per table.
        assert_eq!(ACPI_MAX_TABLE_BYTES, 4 * 1024 * 1024);
        assert!(ACPI_MAX_TABLE_BYTES.is_power_of_two());
    }
}
