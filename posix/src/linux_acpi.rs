//! `<linux/acpi.h>` — ACPI (Advanced Configuration and Power Interface) constants.
//!
//! ACPI provides a standard interface for hardware discovery, power
//! management, and platform configuration. These constants are used
//! by the kernel's ACPI subsystem, device drivers, and tools like
//! acpid and powertop.

// ---------------------------------------------------------------------------
// ACPI power states
// ---------------------------------------------------------------------------

/// Working state.
pub const ACPI_STATE_S0: u8 = 0;
/// Standby (CPU stops, memory refreshed).
pub const ACPI_STATE_S1: u8 = 1;
/// Standby (CPU off, memory refreshed).
pub const ACPI_STATE_S2: u8 = 2;
/// Suspend to RAM.
pub const ACPI_STATE_S3: u8 = 3;
/// Hibernate (suspend to disk).
pub const ACPI_STATE_S4: u8 = 4;
/// Soft off.
pub const ACPI_STATE_S5: u8 = 5;

// ---------------------------------------------------------------------------
// ACPI device states (D-states)
// ---------------------------------------------------------------------------

/// Fully on.
pub const ACPI_STATE_D0: u8 = 0;
/// Intermediate power state.
pub const ACPI_STATE_D1: u8 = 1;
/// Intermediate power state.
pub const ACPI_STATE_D2: u8 = 2;
/// Off (no power, context lost).
pub const ACPI_STATE_D3_HOT: u8 = 3;
/// Off (no power at all).
pub const ACPI_STATE_D3_COLD: u8 = 4;

// ---------------------------------------------------------------------------
// ACPI table signatures (4-byte ASCII)
// ---------------------------------------------------------------------------

/// Root System Description Pointer.
pub const ACPI_SIG_RSDP: &str = "RSD PTR ";
/// Extended System Description Table.
pub const ACPI_SIG_XSDT: &str = "XSDT";
/// Fixed ACPI Description Table.
pub const ACPI_SIG_FADT: &str = "FACP";
/// Differentiated System Description Table.
pub const ACPI_SIG_DSDT: &str = "DSDT";
/// Secondary System Description Table.
pub const ACPI_SIG_SSDT: &str = "SSDT";
/// Multiple APIC Description Table.
pub const ACPI_SIG_MADT: &str = "APIC";
/// High Precision Event Timer.
pub const ACPI_SIG_HPET: &str = "HPET";
/// Memory-Mapped Configuration Space.
pub const ACPI_SIG_MCFG: &str = "MCFG";
/// Boot Graphics Resource Table.
pub const ACPI_SIG_BGRT: &str = "BGRT";
/// NUMA System Resource Affinity Table.
pub const ACPI_SIG_SRAT: &str = "SRAT";
/// System Locality Information Table.
pub const ACPI_SIG_SLIT: &str = "SLIT";
/// Trusted Platform Module 2.0 Table.
pub const ACPI_SIG_TPM2: &str = "TPM2";

// ---------------------------------------------------------------------------
// ACPI generic address structure (GAS) address space IDs
// ---------------------------------------------------------------------------

/// System memory.
pub const ACPI_ADR_SPACE_SYSTEM_MEMORY: u8 = 0;
/// System I/O.
pub const ACPI_ADR_SPACE_SYSTEM_IO: u8 = 1;
/// PCI configuration space.
pub const ACPI_ADR_SPACE_PCI_CONFIG: u8 = 2;
/// Embedded controller.
pub const ACPI_ADR_SPACE_EC: u8 = 3;
/// SMBus.
pub const ACPI_ADR_SPACE_SMBUS: u8 = 4;
/// Functional fixed hardware.
pub const ACPI_ADR_SPACE_FIXED_HARDWARE: u8 = 0x7F;

// ---------------------------------------------------------------------------
// ACPI MADT entry types
// ---------------------------------------------------------------------------

/// Processor Local APIC.
pub const ACPI_MADT_TYPE_LOCAL_APIC: u8 = 0;
/// I/O APIC.
pub const ACPI_MADT_TYPE_IO_APIC: u8 = 1;
/// Interrupt Source Override.
pub const ACPI_MADT_TYPE_INTERRUPT_OVERRIDE: u8 = 2;
/// NMI Source.
pub const ACPI_MADT_TYPE_NMI_SOURCE: u8 = 3;
/// Local APIC NMI.
pub const ACPI_MADT_TYPE_LOCAL_APIC_NMI: u8 = 4;
/// Local APIC Address Override.
pub const ACPI_MADT_TYPE_LOCAL_APIC_OVERRIDE: u8 = 5;
/// Processor Local x2APIC.
pub const ACPI_MADT_TYPE_LOCAL_X2APIC: u8 = 9;
/// Local x2APIC NMI.
pub const ACPI_MADT_TYPE_LOCAL_X2APIC_NMI: u8 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep_states() {
        assert_eq!(ACPI_STATE_S0, 0);
        assert_eq!(ACPI_STATE_S3, 3);
        assert_eq!(ACPI_STATE_S5, 5);
    }

    #[test]
    fn test_device_states() {
        assert_eq!(ACPI_STATE_D0, 0);
        assert_eq!(ACPI_STATE_D3_HOT, 3);
        assert_eq!(ACPI_STATE_D3_COLD, 4);
    }

    #[test]
    fn test_table_sigs() {
        assert_eq!(ACPI_SIG_FADT, "FACP");
        assert_eq!(ACPI_SIG_MADT, "APIC");
        assert_eq!(ACPI_SIG_HPET, "HPET");
        assert_eq!(ACPI_SIG_MCFG, "MCFG");
    }

    #[test]
    fn test_address_spaces_distinct() {
        let spaces = [
            ACPI_ADR_SPACE_SYSTEM_MEMORY,
            ACPI_ADR_SPACE_SYSTEM_IO,
            ACPI_ADR_SPACE_PCI_CONFIG,
            ACPI_ADR_SPACE_EC,
            ACPI_ADR_SPACE_SMBUS,
            ACPI_ADR_SPACE_FIXED_HARDWARE,
        ];
        for i in 0..spaces.len() {
            for j in (i + 1)..spaces.len() {
                assert_ne!(spaces[i], spaces[j]);
            }
        }
    }

    #[test]
    fn test_madt_types_distinct() {
        let types = [
            ACPI_MADT_TYPE_LOCAL_APIC,
            ACPI_MADT_TYPE_IO_APIC,
            ACPI_MADT_TYPE_INTERRUPT_OVERRIDE,
            ACPI_MADT_TYPE_NMI_SOURCE,
            ACPI_MADT_TYPE_LOCAL_APIC_NMI,
            ACPI_MADT_TYPE_LOCAL_APIC_OVERRIDE,
            ACPI_MADT_TYPE_LOCAL_X2APIC,
            ACPI_MADT_TYPE_LOCAL_X2APIC_NMI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
