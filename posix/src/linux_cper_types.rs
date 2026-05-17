//! `<linux/cper.h>` — Common Platform Error Record (CPER) constants.
//!
//! CPER is the UEFI/ACPI standard format for reporting hardware errors.
//! Firmware creates CPER records for errors detected by platform
//! hardware (memory, PCIe, processor, etc.) and delivers them to
//! the OS via GHES, BERT (Boot Error Record Table), or EINJ (Error
//! Injection). Each CPER record contains a header, one or more
//! error sections typed by GUID, and optional FRU (Field Replaceable
//! Unit) identification for service actions.

// ---------------------------------------------------------------------------
// CPER error severity (matches UEFI spec)
// ---------------------------------------------------------------------------

/// Recoverable error.
pub const CPER_SEV_RECOVERABLE: u32 = 0;
/// Fatal error.
pub const CPER_SEV_FATAL: u32 = 1;
/// Corrected error.
pub const CPER_SEV_CORRECTED: u32 = 2;
/// Informational.
pub const CPER_SEV_INFORMATIONAL: u32 = 3;

// ---------------------------------------------------------------------------
// CPER section types (error section GUIDs represented as IDs)
// ---------------------------------------------------------------------------

/// Processor Generic error section.
pub const CPER_SECTION_PROC_GENERIC: u32 = 0;
/// IA32/X64 processor error section.
pub const CPER_SECTION_PROC_IA32X64: u32 = 1;
/// ARM processor error section.
pub const CPER_SECTION_PROC_ARM: u32 = 2;
/// Platform memory error section.
pub const CPER_SECTION_MEMORY: u32 = 3;
/// Platform memory error section v2.
pub const CPER_SECTION_MEMORY2: u32 = 4;
/// PCIe error section.
pub const CPER_SECTION_PCIE: u32 = 5;
/// Firmware error record reference.
pub const CPER_SECTION_FW_ERROR: u32 = 6;
/// PCI bus error section.
pub const CPER_SECTION_PCI_BUS: u32 = 7;
/// PCI component error section.
pub const CPER_SECTION_PCI_DEV: u32 = 8;
/// DMAr (IOMMU) error section.
pub const CPER_SECTION_DMAR: u32 = 9;
/// CXL protocol error section.
pub const CPER_SECTION_CXL_PROTOCOL: u32 = 10;
/// CXL component error section.
pub const CPER_SECTION_CXL_EVENT: u32 = 11;

// ---------------------------------------------------------------------------
// CPER memory error types
// ---------------------------------------------------------------------------

/// Unknown memory error.
pub const CPER_MEM_ERROR_UNKNOWN: u32 = 0;
/// No error (test injection).
pub const CPER_MEM_ERROR_NONE: u32 = 1;
/// Single-bit ECC.
pub const CPER_MEM_ERROR_SINGLEBIT: u32 = 2;
/// Multi-bit ECC.
pub const CPER_MEM_ERROR_MULTIBIT: u32 = 3;
/// Single-symbol ChipKill ECC.
pub const CPER_MEM_ERROR_SINGLESYM: u32 = 4;
/// Multi-symbol ChipKill ECC.
pub const CPER_MEM_ERROR_MULTISYM: u32 = 5;
/// Master abort.
pub const CPER_MEM_ERROR_MASTER_ABORT: u32 = 6;
/// Target abort.
pub const CPER_MEM_ERROR_TARGET_ABORT: u32 = 7;
/// Parity error.
pub const CPER_MEM_ERROR_PARITY: u32 = 8;
/// Scrub corrected error.
pub const CPER_MEM_ERROR_SCRUB_CORRECTED: u32 = 12;
/// Scrub uncorrected error.
pub const CPER_MEM_ERROR_SCRUB_UNCORRECTED: u32 = 13;

// ---------------------------------------------------------------------------
// CPER validation bits (which fields are valid)
// ---------------------------------------------------------------------------

/// Physical address is valid.
pub const CPER_MEM_VALID_PHYS_ADDR: u32 = 1 << 0;
/// Node is valid.
pub const CPER_MEM_VALID_NODE: u32 = 1 << 1;
/// Card is valid.
pub const CPER_MEM_VALID_CARD: u32 = 1 << 2;
/// Module is valid.
pub const CPER_MEM_VALID_MODULE: u32 = 1 << 3;
/// Bank is valid.
pub const CPER_MEM_VALID_BANK: u32 = 1 << 4;
/// Row is valid.
pub const CPER_MEM_VALID_ROW: u32 = 1 << 5;
/// Column is valid.
pub const CPER_MEM_VALID_COLUMN: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_distinct() {
        let sevs = [
            CPER_SEV_RECOVERABLE, CPER_SEV_FATAL,
            CPER_SEV_CORRECTED, CPER_SEV_INFORMATIONAL,
        ];
        for i in 0..sevs.len() {
            for j in (i + 1)..sevs.len() {
                assert_ne!(sevs[i], sevs[j]);
            }
        }
    }

    #[test]
    fn test_sections_distinct() {
        let sections = [
            CPER_SECTION_PROC_GENERIC, CPER_SECTION_PROC_IA32X64,
            CPER_SECTION_PROC_ARM, CPER_SECTION_MEMORY,
            CPER_SECTION_MEMORY2, CPER_SECTION_PCIE,
            CPER_SECTION_FW_ERROR, CPER_SECTION_PCI_BUS,
            CPER_SECTION_PCI_DEV, CPER_SECTION_DMAR,
            CPER_SECTION_CXL_PROTOCOL, CPER_SECTION_CXL_EVENT,
        ];
        for i in 0..sections.len() {
            for j in (i + 1)..sections.len() {
                assert_ne!(sections[i], sections[j]);
            }
        }
    }

    #[test]
    fn test_mem_error_types_distinct() {
        let types = [
            CPER_MEM_ERROR_UNKNOWN, CPER_MEM_ERROR_NONE,
            CPER_MEM_ERROR_SINGLEBIT, CPER_MEM_ERROR_MULTIBIT,
            CPER_MEM_ERROR_SINGLESYM, CPER_MEM_ERROR_MULTISYM,
            CPER_MEM_ERROR_MASTER_ABORT, CPER_MEM_ERROR_TARGET_ABORT,
            CPER_MEM_ERROR_PARITY, CPER_MEM_ERROR_SCRUB_CORRECTED,
            CPER_MEM_ERROR_SCRUB_UNCORRECTED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_validation_bits_no_overlap() {
        let bits = [
            CPER_MEM_VALID_PHYS_ADDR, CPER_MEM_VALID_NODE,
            CPER_MEM_VALID_CARD, CPER_MEM_VALID_MODULE,
            CPER_MEM_VALID_BANK, CPER_MEM_VALID_ROW,
            CPER_MEM_VALID_COLUMN,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }
}
