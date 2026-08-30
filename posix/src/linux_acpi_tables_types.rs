//! `<acpi/actbl.h>` — ACPI table signature and structure constants.
//!
//! ACPI firmware provides hardware descriptions via tables in memory.
//! The RSDP (Root System Description Pointer) points to RSDT/XSDT
//! which list all other tables. Key tables include MADT (interrupt
//! controllers), FADT (fixed hardware addresses), DSDT/SSDT (AML
//! bytecode for device methods), HPET (high-precision timer), MCFG
//! (PCI ECAM base), and many others.

// ---------------------------------------------------------------------------
// ACPI table signatures (4-char ASCII, packed as u32)
// ---------------------------------------------------------------------------

/// RSDP signature "RSD PTR " (8 bytes, special case).
pub const ACPI_SIG_RSDP_LO: u32 = 0x2052_5344; // "RSD "
/// RSDT (Root System Description Table).
pub const ACPI_SIG_RSDT: u32 = 0x5444_5352; // "RSDT"
/// XSDT (Extended System Description Table, 64-bit addresses).
pub const ACPI_SIG_XSDT: u32 = 0x5444_5358; // "XSDT"
/// FADT (Fixed ACPI Description Table).
pub const ACPI_SIG_FADT: u32 = 0x5043_4146; // "FACP"
/// MADT (Multiple APIC Description Table).
pub const ACPI_SIG_MADT: u32 = 0x4349_5041; // "APIC"
/// DSDT (Differentiated System Description Table).
pub const ACPI_SIG_DSDT: u32 = 0x5444_5344; // "DSDT"
/// SSDT (Secondary System Description Table).
pub const ACPI_SIG_SSDT: u32 = 0x5444_5353; // "SSDT"
/// HPET (High Precision Event Timer).
pub const ACPI_SIG_HPET: u32 = 0x5445_5048; // "HPET"
/// MCFG (PCI Express ECAM configuration).
pub const ACPI_SIG_MCFG: u32 = 0x4746_434D; // "MCFG"
/// SRAT (System Resource Affinity Table — NUMA).
pub const ACPI_SIG_SRAT: u32 = 0x5441_5253; // "SRAT"
/// SLIT (System Locality Information Table — NUMA distances).
pub const ACPI_SIG_SLIT: u32 = 0x5449_4C53; // "SLIT"
/// BGRT (Boot Graphics Resource Table).
pub const ACPI_SIG_BGRT: u32 = 0x5452_4742; // "BGRT"

// ---------------------------------------------------------------------------
// ACPI revision
// ---------------------------------------------------------------------------

/// ACPI 1.0 revision.
pub const ACPI_REVISION_1: u32 = 1;
/// ACPI 2.0+ revision (XSDT, 64-bit).
pub const ACPI_REVISION_2: u32 = 2;

// ---------------------------------------------------------------------------
// MADT entry types
// ---------------------------------------------------------------------------

/// Local APIC (per-CPU interrupt controller).
pub const MADT_TYPE_LOCAL_APIC: u32 = 0;
/// I/O APIC (interrupt routing).
pub const MADT_TYPE_IO_APIC: u32 = 1;
/// Interrupt Source Override.
pub const MADT_TYPE_INT_SRC_OVERRIDE: u32 = 2;
/// NMI Source.
pub const MADT_TYPE_NMI_SOURCE: u32 = 3;
/// Local APIC NMI.
pub const MADT_TYPE_LOCAL_APIC_NMI: u32 = 4;
/// Local APIC Address Override (64-bit).
pub const MADT_TYPE_LOCAL_APIC_OVERRIDE: u32 = 5;
/// I/O SAPIC.
pub const MADT_TYPE_IO_SAPIC: u32 = 6;
/// Local x2APIC.
pub const MADT_TYPE_LOCAL_X2APIC: u32 = 9;
/// Local x2APIC NMI.
pub const MADT_TYPE_LOCAL_X2APIC_NMI: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_sigs_distinct() {
        let sigs = [
            ACPI_SIG_RSDT,
            ACPI_SIG_XSDT,
            ACPI_SIG_FADT,
            ACPI_SIG_MADT,
            ACPI_SIG_DSDT,
            ACPI_SIG_SSDT,
            ACPI_SIG_HPET,
            ACPI_SIG_MCFG,
            ACPI_SIG_SRAT,
            ACPI_SIG_SLIT,
            ACPI_SIG_BGRT,
        ];
        for i in 0..sigs.len() {
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }

    #[test]
    fn test_madt_types_distinct() {
        let types = [
            MADT_TYPE_LOCAL_APIC,
            MADT_TYPE_IO_APIC,
            MADT_TYPE_INT_SRC_OVERRIDE,
            MADT_TYPE_NMI_SOURCE,
            MADT_TYPE_LOCAL_APIC_NMI,
            MADT_TYPE_LOCAL_APIC_OVERRIDE,
            MADT_TYPE_IO_SAPIC,
            MADT_TYPE_LOCAL_X2APIC,
            MADT_TYPE_LOCAL_X2APIC_NMI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_revisions() {
        assert!(ACPI_REVISION_1 < ACPI_REVISION_2);
    }
}
