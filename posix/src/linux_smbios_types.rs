//! `<linux/dmi.h>` (SMBIOS subset) — SMBIOS structure types.
//!
//! SMBIOS (System Management BIOS) is the standard for DMI data
//! encoding. Each SMBIOS structure has a type, length, and handle.
//! The entry point (found by scanning memory or via EFI configuration
//! table) provides the SMBIOS version and table location. Structure
//! types are standardized by the DMTF (Distributed Management Task
//! Force) specification.

// ---------------------------------------------------------------------------
// SMBIOS structure types (well-known)
// ---------------------------------------------------------------------------

/// Type 0: BIOS Information.
pub const SMBIOS_TYPE_BIOS: u32 = 0;
/// Type 1: System Information.
pub const SMBIOS_TYPE_SYSTEM: u32 = 1;
/// Type 2: Baseboard (Motherboard).
pub const SMBIOS_TYPE_BASEBOARD: u32 = 2;
/// Type 3: System Enclosure / Chassis.
pub const SMBIOS_TYPE_CHASSIS: u32 = 3;
/// Type 4: Processor Information.
pub const SMBIOS_TYPE_PROCESSOR: u32 = 4;
/// Type 7: Cache Information.
pub const SMBIOS_TYPE_CACHE: u32 = 7;
/// Type 8: Port Connector.
pub const SMBIOS_TYPE_PORT: u32 = 8;
/// Type 9: System Slot.
pub const SMBIOS_TYPE_SLOT: u32 = 9;
/// Type 16: Physical Memory Array.
pub const SMBIOS_TYPE_MEMARRAY: u32 = 16;
/// Type 17: Memory Device (individual DIMM).
pub const SMBIOS_TYPE_MEMDEVICE: u32 = 17;
/// Type 19: Memory Array Mapped Address.
pub const SMBIOS_TYPE_MEMMAPPED: u32 = 19;
/// Type 32: System Boot Information.
pub const SMBIOS_TYPE_BOOT: u32 = 32;
/// Type 127: End of Table.
pub const SMBIOS_TYPE_END: u32 = 127;

// ---------------------------------------------------------------------------
// SMBIOS entry point signatures
// ---------------------------------------------------------------------------

/// SMBIOS 2.x entry point signature "_SM_".
pub const SMBIOS2_ANCHOR: u32 = 0x5F4D_535F; // "_SM_"
/// SMBIOS 3.x entry point signature "_SM3_".
pub const SMBIOS3_ANCHOR: u32 = 0x5F33_4D53; // "_SM3" (first 4 bytes)

// ---------------------------------------------------------------------------
// SMBIOS versions
// ---------------------------------------------------------------------------

/// SMBIOS 2.0.
pub const SMBIOS_VERSION_2_0: u32 = 0x0200;
/// SMBIOS 2.8.
pub const SMBIOS_VERSION_2_8: u32 = 0x0208;
/// SMBIOS 3.0.
pub const SMBIOS_VERSION_3_0: u32 = 0x0300;
/// SMBIOS 3.5 (latest as of 2024).
pub const SMBIOS_VERSION_3_5: u32 = 0x0305;

// ---------------------------------------------------------------------------
// SMBIOS memory form factors (Type 17)
// ---------------------------------------------------------------------------

/// DIMM.
pub const SMBIOS_MEMFORM_DIMM: u32 = 0x09;
/// SO-DIMM (laptop).
pub const SMBIOS_MEMFORM_SODIMM: u32 = 0x0D;
/// NVDIMM (persistent memory).
pub const SMBIOS_MEMFORM_NVDIMM: u32 = 0x18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            SMBIOS_TYPE_BIOS, SMBIOS_TYPE_SYSTEM, SMBIOS_TYPE_BASEBOARD,
            SMBIOS_TYPE_CHASSIS, SMBIOS_TYPE_PROCESSOR, SMBIOS_TYPE_CACHE,
            SMBIOS_TYPE_PORT, SMBIOS_TYPE_SLOT, SMBIOS_TYPE_MEMARRAY,
            SMBIOS_TYPE_MEMDEVICE, SMBIOS_TYPE_MEMMAPPED,
            SMBIOS_TYPE_BOOT, SMBIOS_TYPE_END,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_versions_ordered() {
        assert!(SMBIOS_VERSION_2_0 < SMBIOS_VERSION_2_8);
        assert!(SMBIOS_VERSION_2_8 < SMBIOS_VERSION_3_0);
        assert!(SMBIOS_VERSION_3_0 < SMBIOS_VERSION_3_5);
    }

    #[test]
    fn test_form_factors_distinct() {
        let forms = [
            SMBIOS_MEMFORM_DIMM, SMBIOS_MEMFORM_SODIMM,
            SMBIOS_MEMFORM_NVDIMM,
        ];
        for i in 0..forms.len() {
            for j in (i + 1)..forms.len() {
                assert_ne!(forms[i], forms[j]);
            }
        }
    }
}
