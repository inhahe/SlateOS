//! `<acpi/ghes.h>` — Generic Hardware Error Source (GHES) constants.
//!
//! GHES is the ACPI mechanism for firmware to report hardware errors
//! to the OS. The firmware populates a CPER record in a shared memory
//! region and notifies the OS via SCI, NMI, or SEA/SEI (ARM). GHES
//! handles errors from all platform components: memory, PCIe, CPU,
//! firmware, CXL, etc. It's the primary error reporting path on
//! modern UEFI servers, replacing older platform-specific mechanisms.

// ---------------------------------------------------------------------------
// GHES notification types (how firmware signals an error)
// ---------------------------------------------------------------------------

/// Polled (OS periodically checks error status).
pub const GHES_NOTIFY_POLLED: u32 = 0;
/// External interrupt (SCI on x86).
pub const GHES_NOTIFY_SCI: u32 = 1;
/// NMI (Non-Maskable Interrupt).
pub const GHES_NOTIFY_NMI: u32 = 2;
/// Local interrupt (per-CPU).
pub const GHES_NOTIFY_LOCAL: u32 = 3;
/// SEA (Synchronous External Abort, ARM).
pub const GHES_NOTIFY_SEA: u32 = 4;
/// SEI (System Error Interrupt, ARM).
pub const GHES_NOTIFY_SEI: u32 = 5;
/// GPIO (General Purpose I/O signal).
pub const GHES_NOTIFY_GPIO: u32 = 6;
/// Software delegated exception (ARM SDEI).
pub const GHES_NOTIFY_SOFTWARE_DELEGATED: u32 = 7;

// ---------------------------------------------------------------------------
// GHES error source types (HEST table entries)
// ---------------------------------------------------------------------------

/// IA-32 Machine Check Exception.
pub const GHES_SOURCE_IA32_MCE: u32 = 0;
/// IA-32 Corrected Machine Check.
pub const GHES_SOURCE_IA32_CMC: u32 = 1;
/// IA-32 NMI.
pub const GHES_SOURCE_IA32_NMI: u32 = 2;
/// Generic Hardware Error Source (v1).
pub const GHES_SOURCE_GENERIC: u32 = 9;
/// Generic Hardware Error Source v2 (with read-ack).
pub const GHES_SOURCE_GENERIC_V2: u32 = 10;
/// IA-32 Deferred Machine Check.
pub const GHES_SOURCE_IA32_DEFERRED_MCE: u32 = 11;

// ---------------------------------------------------------------------------
// GHES status block flags
// ---------------------------------------------------------------------------

/// Error data is valid.
pub const GHES_STATUS_VALID: u32 = 1 << 0;
/// Error source has been acknowledged.
pub const GHES_STATUS_ACKED: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// GHES read-ack values (GHESv2)
// ---------------------------------------------------------------------------

/// Acknowledge error record (firmware can reuse buffer).
pub const GHES_ACK_VALUE: u64 = 0;
/// Error record not yet acknowledged.
pub const GHES_NACK_VALUE: u64 = 1;

// ---------------------------------------------------------------------------
// GHES error block severity (matches CPER severity)
// ---------------------------------------------------------------------------

/// Corrected error.
pub const GHES_BLOCK_SEV_CORRECTED: u32 = 0;
/// Recoverable error.
pub const GHES_BLOCK_SEV_RECOVERABLE: u32 = 1;
/// Fatal error.
pub const GHES_BLOCK_SEV_FATAL: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_types_distinct() {
        let types = [
            GHES_NOTIFY_POLLED,
            GHES_NOTIFY_SCI,
            GHES_NOTIFY_NMI,
            GHES_NOTIFY_LOCAL,
            GHES_NOTIFY_SEA,
            GHES_NOTIFY_SEI,
            GHES_NOTIFY_GPIO,
            GHES_NOTIFY_SOFTWARE_DELEGATED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_source_types_distinct() {
        let sources = [
            GHES_SOURCE_IA32_MCE,
            GHES_SOURCE_IA32_CMC,
            GHES_SOURCE_IA32_NMI,
            GHES_SOURCE_GENERIC,
            GHES_SOURCE_GENERIC_V2,
            GHES_SOURCE_IA32_DEFERRED_MCE,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }

    #[test]
    fn test_status_flags_no_overlap() {
        let flags = [GHES_STATUS_VALID, GHES_STATUS_ACKED];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_ack_values_distinct() {
        assert_ne!(GHES_ACK_VALUE, GHES_NACK_VALUE);
    }

    #[test]
    fn test_severity_distinct() {
        let sevs = [
            GHES_BLOCK_SEV_CORRECTED,
            GHES_BLOCK_SEV_RECOVERABLE,
            GHES_BLOCK_SEV_FATAL,
        ];
        for i in 0..sevs.len() {
            for j in (i + 1)..sevs.len() {
                assert_ne!(sevs[i], sevs[j]);
            }
        }
    }
}
