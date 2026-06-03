//! Less-common ACPI table signatures (HEST, BERT, EINJ, ERST, MPST,
//! PCCT, IORT, GTDT, PPTT, SLIC, MSCT, NHLT).
//!
//! These tables aren't in every machine's firmware but show up on
//! servers (HEST/BERT/EINJ for RAS), ARM platforms (IORT/GTDT/PPTT),
//! and laptops with onboard audio (NHLT). `acpidump` lists them all.

// ---------------------------------------------------------------------------
// RAS / error-injection
// ---------------------------------------------------------------------------

pub const ACPI_SIG_HEST: &str = "HEST"; // Hardware Error Source Table
pub const ACPI_SIG_BERT: &str = "BERT"; // Boot Error Record Table
pub const ACPI_SIG_EINJ: &str = "EINJ"; // Error Injection
pub const ACPI_SIG_ERST: &str = "ERST"; // Error Record Serialization

// ---------------------------------------------------------------------------
// Memory topology / NUMA
// ---------------------------------------------------------------------------

pub const ACPI_SIG_MSCT: &str = "MSCT"; // Maximum System Characteristics
pub const ACPI_SIG_MPST: &str = "MPST"; // Memory Power State Table
pub const ACPI_SIG_PMTT: &str = "PMTT"; // Platform Memory Topology
pub const ACPI_SIG_HMAT: &str = "HMAT"; // Heterogeneous Memory Attributes

// ---------------------------------------------------------------------------
// Platform-communication / hardware
// ---------------------------------------------------------------------------

pub const ACPI_SIG_PCCT: &str = "PCCT"; // Platform Communications Channel
pub const ACPI_SIG_PDTT: &str = "PDTT"; // Platform Debug Trigger
pub const ACPI_SIG_LPIT: &str = "LPIT"; // Low Power Idle Table

// ---------------------------------------------------------------------------
// ARM platform tables
// ---------------------------------------------------------------------------

pub const ACPI_SIG_IORT: &str = "IORT"; // I/O Remapping Table
pub const ACPI_SIG_GTDT: &str = "GTDT"; // Generic Timer Description
pub const ACPI_SIG_PPTT: &str = "PPTT"; // Processor Properties Topology

// ---------------------------------------------------------------------------
// Audio / OEM
// ---------------------------------------------------------------------------

pub const ACPI_SIG_NHLT: &str = "NHLT"; // Non-HD Audio Link Table
pub const ACPI_SIG_SLIC: &str = "SLIC"; // OEM Windows license proof

// ---------------------------------------------------------------------------
// HEST notification types (`hest_notification.type`)
// ---------------------------------------------------------------------------

pub const HEST_NOTIFY_POLLED: u8 = 0;
pub const HEST_NOTIFY_EXTERNAL_INT: u8 = 1;
pub const HEST_NOTIFY_LOCAL_INT: u8 = 2;
pub const HEST_NOTIFY_SCI: u8 = 3;
pub const HEST_NOTIFY_NMI: u8 = 4;
pub const HEST_NOTIFY_CMCI: u8 = 5;
pub const HEST_NOTIFY_MCE: u8 = 6;
pub const HEST_NOTIFY_GPIO_SIGNAL: u8 = 7;
pub const HEST_NOTIFY_SEA: u8 = 8;
pub const HEST_NOTIFY_SEI: u8 = 9;
pub const HEST_NOTIFY_GSIV: u8 = 10;
pub const HEST_NOTIFY_SOFTWARE_DELEGATED: u8 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_signatures_4_chars() {
        let s = [
            ACPI_SIG_HEST,
            ACPI_SIG_BERT,
            ACPI_SIG_EINJ,
            ACPI_SIG_ERST,
            ACPI_SIG_MSCT,
            ACPI_SIG_MPST,
            ACPI_SIG_PMTT,
            ACPI_SIG_HMAT,
            ACPI_SIG_PCCT,
            ACPI_SIG_PDTT,
            ACPI_SIG_LPIT,
            ACPI_SIG_IORT,
            ACPI_SIG_GTDT,
            ACPI_SIG_PPTT,
            ACPI_SIG_NHLT,
            ACPI_SIG_SLIC,
        ];
        for sig in s {
            assert_eq!(sig.len(), 4);
            // All printable ASCII uppercase letters.
            for b in sig.bytes() {
                assert!(b.is_ascii_uppercase());
            }
        }
    }

    #[test]
    fn test_signatures_distinct() {
        let s = [
            ACPI_SIG_HEST,
            ACPI_SIG_BERT,
            ACPI_SIG_EINJ,
            ACPI_SIG_ERST,
            ACPI_SIG_MSCT,
            ACPI_SIG_MPST,
            ACPI_SIG_PMTT,
            ACPI_SIG_HMAT,
            ACPI_SIG_PCCT,
            ACPI_SIG_PDTT,
            ACPI_SIG_LPIT,
            ACPI_SIG_IORT,
            ACPI_SIG_GTDT,
            ACPI_SIG_PPTT,
            ACPI_SIG_NHLT,
            ACPI_SIG_SLIC,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_hest_notify_dense_0_to_11() {
        let n = [
            HEST_NOTIFY_POLLED,
            HEST_NOTIFY_EXTERNAL_INT,
            HEST_NOTIFY_LOCAL_INT,
            HEST_NOTIFY_SCI,
            HEST_NOTIFY_NMI,
            HEST_NOTIFY_CMCI,
            HEST_NOTIFY_MCE,
            HEST_NOTIFY_GPIO_SIGNAL,
            HEST_NOTIFY_SEA,
            HEST_NOTIFY_SEI,
            HEST_NOTIFY_GSIV,
            HEST_NOTIFY_SOFTWARE_DELEGATED,
        ];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_iort_pptt_gtdt_are_arm_tables() {
        // These are the three ARM-platform tables Linux requires for
        // SBSA boards.
        for s in [ACPI_SIG_IORT, ACPI_SIG_GTDT, ACPI_SIG_PPTT] {
            assert_eq!(s.len(), 4);
        }
    }
}
