//! `<linux/acpi.h>` — ACPI tables, sysfs surface, and ACPICA constants.
//!
//! This module covers the parts of Linux's ACPI subsystem visible to
//! userspace: the 4-character table signatures shown by `acpidump`,
//! the `/sys/firmware/acpi/` paths, and the `/dev/acpi*` device nodes
//! used by `acpid` and `lm-sensors`.

// ---------------------------------------------------------------------------
// Sysfs and procfs paths
// ---------------------------------------------------------------------------

pub const SYS_FIRMWARE_ACPI: &str = "/sys/firmware/acpi";
pub const SYS_FIRMWARE_ACPI_TABLES: &str = "/sys/firmware/acpi/tables";
pub const PROC_ACPI: &str = "/proc/acpi";
pub const DEV_ACPI_EVENT: &str = "/proc/acpi/event"; // legacy
pub const DEV_INPUT_BY_PATH_ACPI: &str = "/dev/input/by-path";

// ---------------------------------------------------------------------------
// ACPI table signatures (4 ASCII bytes, little-endian on disk)
// ---------------------------------------------------------------------------

pub const ACPI_SIG_RSDP: &str = "RSD PTR ";
pub const ACPI_SIG_RSDT: &str = "RSDT";
pub const ACPI_SIG_XSDT: &str = "XSDT";
pub const ACPI_SIG_FADT: &str = "FACP";
pub const ACPI_SIG_FACS: &str = "FACS";
pub const ACPI_SIG_DSDT: &str = "DSDT";
pub const ACPI_SIG_SSDT: &str = "SSDT";
pub const ACPI_SIG_MADT: &str = "APIC";
pub const ACPI_SIG_MCFG: &str = "MCFG";
pub const ACPI_SIG_HPET: &str = "HPET";
pub const ACPI_SIG_SRAT: &str = "SRAT";
pub const ACPI_SIG_SLIT: &str = "SLIT";
pub const ACPI_SIG_DMAR: &str = "DMAR";
pub const ACPI_SIG_IVRS: &str = "IVRS";
pub const ACPI_SIG_TPM2: &str = "TPM2";
pub const ACPI_SIG_BGRT: &str = "BGRT";

// ---------------------------------------------------------------------------
// ACPI header byte layout
// ---------------------------------------------------------------------------

/// `struct acpi_table_header` is 36 bytes (signature 4 + length 4 +
/// revision 1 + checksum 1 + OEMID 6 + OEM table id 8 + OEM rev 4 +
/// creator 4 + creator rev 4).
pub const ACPI_TABLE_HEADER_SIZE: usize = 36;

pub const ACPI_OEM_ID_LEN: usize = 6;
pub const ACPI_OEM_TABLE_ID_LEN: usize = 8;
pub const ACPI_SIG_LEN: usize = 4;

// ---------------------------------------------------------------------------
// PM1 control-register sleep-type bits (`SLP_TYPx`)
// ---------------------------------------------------------------------------

pub const ACPI_PM1_SLP_EN: u16 = 1 << 13;
pub const ACPI_PM1_SLP_TYP_MASK: u16 = 0x1C00;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_under_sys_firmware_or_proc() {
        assert!(SYS_FIRMWARE_ACPI_TABLES.starts_with(SYS_FIRMWARE_ACPI));
        assert!(PROC_ACPI.starts_with("/proc/"));
        assert!(DEV_ACPI_EVENT.starts_with("/proc/acpi/"));
    }

    #[test]
    fn test_signatures_are_4_chars_except_rsdp() {
        // Everything except RSDP signature ("RSD PTR " is 8 bytes —
        // the only table with an 8-byte signature in the spec).
        for sig in [
            ACPI_SIG_RSDT,
            ACPI_SIG_XSDT,
            ACPI_SIG_FADT,
            ACPI_SIG_FACS,
            ACPI_SIG_DSDT,
            ACPI_SIG_SSDT,
            ACPI_SIG_MADT,
            ACPI_SIG_MCFG,
            ACPI_SIG_HPET,
            ACPI_SIG_SRAT,
            ACPI_SIG_SLIT,
            ACPI_SIG_DMAR,
            ACPI_SIG_IVRS,
            ACPI_SIG_TPM2,
            ACPI_SIG_BGRT,
        ] {
            assert_eq!(sig.len(), 4);
        }
        // RSDP is uniquely 8 bytes.
        assert_eq!(ACPI_SIG_RSDP.len(), 8);
    }

    #[test]
    fn test_fadt_is_facp_not_fadt() {
        // FADT's on-disk signature is "FACP" — a famous trap when
        // searching for "FADT".
        assert_eq!(ACPI_SIG_FADT, "FACP");
        // MADT's on-disk signature is "APIC", not "MADT".
        assert_eq!(ACPI_SIG_MADT, "APIC");
    }

    #[test]
    fn test_signatures_distinct() {
        let s = [
            ACPI_SIG_RSDT,
            ACPI_SIG_XSDT,
            ACPI_SIG_FADT,
            ACPI_SIG_FACS,
            ACPI_SIG_DSDT,
            ACPI_SIG_SSDT,
            ACPI_SIG_MADT,
            ACPI_SIG_MCFG,
            ACPI_SIG_HPET,
            ACPI_SIG_SRAT,
            ACPI_SIG_SLIT,
            ACPI_SIG_DMAR,
            ACPI_SIG_IVRS,
            ACPI_SIG_TPM2,
            ACPI_SIG_BGRT,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_header_byte_layout_sums_to_36() {
        // Sum every field width per the ACPI 6.5 spec:
        // sig(4)+len(4)+rev(1)+csum(1)+oem(6)+oem_tbl(8)+oem_rev(4)+
        // creator(4)+creator_rev(4) = 36.
        let sum = ACPI_SIG_LEN + 4 + 1 + 1 + ACPI_OEM_ID_LEN + ACPI_OEM_TABLE_ID_LEN + 4 + 4 + 4;
        assert_eq!(sum, ACPI_TABLE_HEADER_SIZE);
        assert_eq!(ACPI_TABLE_HEADER_SIZE, 36);
    }

    #[test]
    fn test_pm1_slp_bits_disjoint() {
        // SLP_EN is bit 13, SLP_TYP occupies bits 10..12.
        assert_eq!(ACPI_PM1_SLP_EN, 1 << 13);
        assert_eq!(ACPI_PM1_SLP_TYP_MASK, 0b111 << 10);
        assert_eq!(ACPI_PM1_SLP_EN & ACPI_PM1_SLP_TYP_MASK, 0);
    }
}
