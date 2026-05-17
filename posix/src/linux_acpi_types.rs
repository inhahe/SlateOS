//! `<acpi/acpi.h>` — ACPI (Advanced Configuration and Power Interface) constants.
//!
//! ACPI provides an open standard for device configuration, power
//! management, and thermal control. The kernel ACPI subsystem
//! parses ACPI tables (DSDT, SSDT, FADT, etc.) and manages device
//! enumeration, power states, and platform events.

// ---------------------------------------------------------------------------
// ACPI power states (device D-states)
// ---------------------------------------------------------------------------

/// D0: Fully on.
pub const ACPI_STATE_D0: u8 = 0;
/// D1: Light sleep.
pub const ACPI_STATE_D1: u8 = 1;
/// D2: Deeper sleep.
pub const ACPI_STATE_D2: u8 = 2;
/// D3hot: Software-off, power available.
pub const ACPI_STATE_D3_HOT: u8 = 3;
/// D3cold: Power removed.
pub const ACPI_STATE_D3_COLD: u8 = 4;

// ---------------------------------------------------------------------------
// ACPI system sleep states (S-states)
// ---------------------------------------------------------------------------

/// S0: Working.
pub const ACPI_STATE_S0: u8 = 0;
/// S1: Standby (CPU stopped, RAM refreshed).
pub const ACPI_STATE_S1: u8 = 1;
/// S2: Deeper standby.
pub const ACPI_STATE_S2: u8 = 2;
/// S3: Suspend to RAM.
pub const ACPI_STATE_S3: u8 = 3;
/// S4: Suspend to disk (hibernate).
pub const ACPI_STATE_S4: u8 = 4;
/// S5: Soft off.
pub const ACPI_STATE_S5: u8 = 5;

// ---------------------------------------------------------------------------
// ACPI table signatures (4-byte ASCII)
// ---------------------------------------------------------------------------

/// Root System Description Pointer.
pub const ACPI_SIG_RSDP: &str = "RSD PTR ";
/// Root System Description Table.
pub const ACPI_SIG_RSDT: &str = "RSDT";
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
/// Memory Configuration Table (NFIT).
pub const ACPI_SIG_NFIT: &str = "NFIT";
/// PCI Express Memory-mapped Configuration.
pub const ACPI_SIG_MCFG: &str = "MCFG";

// ---------------------------------------------------------------------------
// ACPI address space types
// ---------------------------------------------------------------------------

/// System memory.
pub const ACPI_ADR_SPACE_SYSTEM_MEMORY: u8 = 0;
/// System I/O.
pub const ACPI_ADR_SPACE_SYSTEM_IO: u8 = 1;
/// PCI Configuration.
pub const ACPI_ADR_SPACE_PCI_CONFIG: u8 = 2;
/// Embedded Controller.
pub const ACPI_ADR_SPACE_EC: u8 = 3;
/// SMBus.
pub const ACPI_ADR_SPACE_SMBUS: u8 = 4;
/// CMOS.
pub const ACPI_ADR_SPACE_CMOS: u8 = 5;
/// PCI BAR target.
pub const ACPI_ADR_SPACE_PCI_BAR_TARGET: u8 = 6;
/// Fixed hardware.
pub const ACPI_ADR_SPACE_FIXED_HARDWARE: u8 = 0x7F;

// ---------------------------------------------------------------------------
// ACPI event types
// ---------------------------------------------------------------------------

/// Power button pressed.
pub const ACPI_EVENT_POWER_BUTTON: u8 = 0;
/// Sleep button pressed.
pub const ACPI_EVENT_SLEEP_BUTTON: u8 = 1;
/// Lid opened/closed.
pub const ACPI_EVENT_LID: u8 = 2;
/// AC adapter connected/disconnected.
pub const ACPI_EVENT_AC_ADAPTER: u8 = 3;
/// Battery status changed.
pub const ACPI_EVENT_BATTERY: u8 = 4;
/// Thermal zone event.
pub const ACPI_EVENT_THERMAL: u8 = 5;

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
    fn test_table_signatures_distinct() {
        let sigs = [
            ACPI_SIG_RSDT, ACPI_SIG_XSDT, ACPI_SIG_FADT,
            ACPI_SIG_DSDT, ACPI_SIG_SSDT, ACPI_SIG_MADT,
            ACPI_SIG_HPET, ACPI_SIG_NFIT, ACPI_SIG_MCFG,
        ];
        for i in 0..sigs.len() {
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }

    #[test]
    fn test_address_spaces_distinct() {
        let spaces = [
            ACPI_ADR_SPACE_SYSTEM_MEMORY, ACPI_ADR_SPACE_SYSTEM_IO,
            ACPI_ADR_SPACE_PCI_CONFIG, ACPI_ADR_SPACE_EC,
            ACPI_ADR_SPACE_SMBUS, ACPI_ADR_SPACE_CMOS,
            ACPI_ADR_SPACE_PCI_BAR_TARGET, ACPI_ADR_SPACE_FIXED_HARDWARE,
        ];
        for i in 0..spaces.len() {
            for j in (i + 1)..spaces.len() {
                assert_ne!(spaces[i], spaces[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            ACPI_EVENT_POWER_BUTTON, ACPI_EVENT_SLEEP_BUTTON,
            ACPI_EVENT_LID, ACPI_EVENT_AC_ADAPTER,
            ACPI_EVENT_BATTERY, ACPI_EVENT_THERMAL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
