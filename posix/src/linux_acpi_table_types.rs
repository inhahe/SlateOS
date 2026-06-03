//! `<acpi/actbl.h>` — ACPI table signature and revision constants.
//!
//! ACPI firmware tables are identified by 4-character signatures
//! and version numbers. The OS locates these tables via the RSDP
//! (Root System Description Pointer), which points to the RSDT/XSDT
//! containing pointers to all other tables.

// ---------------------------------------------------------------------------
// ACPI table signatures (as u32 from 4 ASCII bytes, little-endian)
// ---------------------------------------------------------------------------

/// "APIC" — Multiple APIC Description Table (MADT).
pub const ACPI_SIG_MADT: u32 = 0x4349_5041;
/// "FACP" — Fixed ACPI Description Table (FADT).
pub const ACPI_SIG_FADT: u32 = 0x5043_4146;
/// "HPET" — High Precision Event Timer table.
pub const ACPI_SIG_HPET: u32 = 0x5445_5048;
/// "MCFG" — PCI Express Memory-mapped Configuration table.
pub const ACPI_SIG_MCFG: u32 = 0x4746_434D;
/// "SSDT" — Secondary System Description Table.
pub const ACPI_SIG_SSDT: u32 = 0x5444_5353;
/// "DSDT" — Differentiated System Description Table.
pub const ACPI_SIG_DSDT: u32 = 0x5444_5344;
/// "BGRT" — Boot Graphics Resource Table.
pub const ACPI_SIG_BGRT: u32 = 0x5452_4742;
/// "SRAT" — System Resource Affinity Table (NUMA).
pub const ACPI_SIG_SRAT: u32 = 0x5441_5253;
/// "SLIT" — System Locality Information Table (NUMA distances).
pub const ACPI_SIG_SLIT: u32 = 0x5449_4C53;
/// "DMAR" — DMA Remapping Table (Intel VT-d).
pub const ACPI_SIG_DMAR: u32 = 0x5241_4D44;

// ---------------------------------------------------------------------------
// ACPI revision constants
// ---------------------------------------------------------------------------

/// ACPI 1.0 revision.
pub const ACPI_REVISION_1: u8 = 1;
/// ACPI 2.0 revision (introduced XSDT, 64-bit addresses).
pub const ACPI_REVISION_2: u8 = 2;
/// ACPI 3.0 revision.
pub const ACPI_REVISION_3: u8 = 3;
/// ACPI 4.0 revision.
pub const ACPI_REVISION_4: u8 = 4;
/// ACPI 5.0 revision.
pub const ACPI_REVISION_5: u8 = 5;
/// ACPI 6.0 revision (current major revision family).
pub const ACPI_REVISION_6: u8 = 6;

// ---------------------------------------------------------------------------
// RSDP signature bytes
// ---------------------------------------------------------------------------

/// RSDP signature: "RSD PTR " (8 bytes as u64, little-endian).
pub const ACPI_RSDP_SIGNATURE: u64 = 0x2052_5450_2044_5352;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_signatures_distinct() {
        let sigs = [
            ACPI_SIG_MADT,
            ACPI_SIG_FADT,
            ACPI_SIG_HPET,
            ACPI_SIG_MCFG,
            ACPI_SIG_SSDT,
            ACPI_SIG_DSDT,
            ACPI_SIG_BGRT,
            ACPI_SIG_SRAT,
            ACPI_SIG_SLIT,
            ACPI_SIG_DMAR,
        ];
        for i in 0..sigs.len() {
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }

    #[test]
    fn test_revisions_sequential() {
        assert_eq!(ACPI_REVISION_1, 1);
        assert_eq!(ACPI_REVISION_2, 2);
        assert_eq!(ACPI_REVISION_3, 3);
        assert_eq!(ACPI_REVISION_4, 4);
        assert_eq!(ACPI_REVISION_5, 5);
        assert_eq!(ACPI_REVISION_6, 6);
    }

    #[test]
    fn test_rsdp_signature() {
        // "RSD PTR " in ASCII bytes
        let bytes = b"RSD PTR ";
        let val = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        assert_eq!(val, ACPI_RSDP_SIGNATURE);
    }

    #[test]
    fn test_madt_signature_bytes() {
        // "APIC" as little-endian u32
        let bytes = b"APIC";
        let val = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(val, ACPI_SIG_MADT);
    }
}
