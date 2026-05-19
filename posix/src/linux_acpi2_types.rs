//! `<linux/acpi.h>` — Additional ACPI constants.
//!
//! Supplementary ACPI constants covering power states,
//! device types, and table signatures.

// ---------------------------------------------------------------------------
// ACPI power states (D-states)
// ---------------------------------------------------------------------------

/// D0: fully on.
pub const ACPI_STATE_D0: u32 = 0;
/// D1: light sleep.
pub const ACPI_STATE_D1: u32 = 1;
/// D2: deeper sleep.
pub const ACPI_STATE_D2: u32 = 2;
/// D3hot: device power removed but bus powered.
pub const ACPI_STATE_D3_HOT: u32 = 3;
/// D3cold: fully off.
pub const ACPI_STATE_D3_COLD: u32 = 4;

// ---------------------------------------------------------------------------
// ACPI system sleep states (S-states)
// ---------------------------------------------------------------------------

/// S0: working.
pub const ACPI_STATE_S0: u32 = 0;
/// S1: standby.
pub const ACPI_STATE_S1: u32 = 1;
/// S2: CPU off.
pub const ACPI_STATE_S2: u32 = 2;
/// S3: suspend to RAM.
pub const ACPI_STATE_S3: u32 = 3;
/// S4: suspend to disk (hibernate).
pub const ACPI_STATE_S4: u32 = 4;
/// S5: soft off.
pub const ACPI_STATE_S5: u32 = 5;

// ---------------------------------------------------------------------------
// ACPI table signatures (as u32 for matching)
// ---------------------------------------------------------------------------

/// RSDP signature ("RSD PTR ").
pub const ACPI_SIG_RSDP_LO: u32 = 0x20445352;
/// DSDT table.
pub const ACPI_SIG_DSDT: u32 = 0x54445344;
/// FADT (Fixed ACPI Description Table).
pub const ACPI_SIG_FADT: u32 = 0x50434146;
/// MADT (Multiple APIC Description Table).
pub const ACPI_SIG_MADT: u32 = 0x43495041;
/// SSDT (Secondary System Description Table).
pub const ACPI_SIG_SSDT: u32 = 0x54445353;
/// HPET table.
pub const ACPI_SIG_HPET: u32 = 0x54455048;
/// MCFG (PCI Express memory-mapped config).
pub const ACPI_SIG_MCFG: u32 = 0x4746434D;
/// BGRT (Boot Graphics Resource Table).
pub const ACPI_SIG_BGRT: u32 = 0x54524742;

// ---------------------------------------------------------------------------
// ACPI generic address space IDs
// ---------------------------------------------------------------------------

/// System memory.
pub const ACPI_ADR_SPACE_SYSTEM_MEMORY: u32 = 0;
/// System I/O.
pub const ACPI_ADR_SPACE_SYSTEM_IO: u32 = 1;
/// PCI config space.
pub const ACPI_ADR_SPACE_PCI_CONFIG: u32 = 2;
/// Embedded controller.
pub const ACPI_ADR_SPACE_EC: u32 = 3;
/// SMBus.
pub const ACPI_ADR_SPACE_SMBUS: u32 = 4;
/// CMOS.
pub const ACPI_ADR_SPACE_CMOS: u32 = 5;
/// PCI BAR target.
pub const ACPI_ADR_SPACE_PCI_BAR_TARGET: u32 = 6;
/// IPMI.
pub const ACPI_ADR_SPACE_IPMI: u32 = 7;
/// GPIO.
pub const ACPI_ADR_SPACE_GPIO: u32 = 8;
/// Generic serial bus.
pub const ACPI_ADR_SPACE_GSBUS: u32 = 9;
/// Platform comm channel.
pub const ACPI_ADR_SPACE_PCC: u32 = 10;
/// Platform runtime mechanism.
pub const ACPI_ADR_SPACE_PRM: u32 = 11;
/// Functional fixed hardware.
pub const ACPI_ADR_SPACE_FIXED_HARDWARE: u32 = 0x7F;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d_states_distinct() {
        let states = [
            ACPI_STATE_D0, ACPI_STATE_D1, ACPI_STATE_D2,
            ACPI_STATE_D3_HOT, ACPI_STATE_D3_COLD,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_s_states_distinct() {
        let states = [
            ACPI_STATE_S0, ACPI_STATE_S1, ACPI_STATE_S2,
            ACPI_STATE_S3, ACPI_STATE_S4, ACPI_STATE_S5,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_table_sigs_distinct() {
        let sigs = [
            ACPI_SIG_DSDT, ACPI_SIG_FADT, ACPI_SIG_MADT,
            ACPI_SIG_SSDT, ACPI_SIG_HPET, ACPI_SIG_MCFG,
            ACPI_SIG_BGRT,
        ];
        for i in 0..sigs.len() {
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }

    #[test]
    fn test_addr_spaces_distinct() {
        let spaces = [
            ACPI_ADR_SPACE_SYSTEM_MEMORY, ACPI_ADR_SPACE_SYSTEM_IO,
            ACPI_ADR_SPACE_PCI_CONFIG, ACPI_ADR_SPACE_EC,
            ACPI_ADR_SPACE_SMBUS, ACPI_ADR_SPACE_CMOS,
            ACPI_ADR_SPACE_PCI_BAR_TARGET, ACPI_ADR_SPACE_IPMI,
            ACPI_ADR_SPACE_GPIO, ACPI_ADR_SPACE_GSBUS,
            ACPI_ADR_SPACE_PCC, ACPI_ADR_SPACE_PRM,
            ACPI_ADR_SPACE_FIXED_HARDWARE,
        ];
        for i in 0..spaces.len() {
            for j in (i + 1)..spaces.len() {
                assert_ne!(spaces[i], spaces[j]);
            }
        }
    }

    #[test]
    fn test_d0_is_zero() {
        assert_eq!(ACPI_STATE_D0, 0);
    }

    #[test]
    fn test_s0_is_zero() {
        assert_eq!(ACPI_STATE_S0, 0);
    }
}
