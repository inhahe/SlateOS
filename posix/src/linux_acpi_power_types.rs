//! `<acpi/actypes.h>` (power subset) — ACPI power management constants.
//!
//! ACPI (Advanced Configuration and Power Interface) defines standard
//! power states for the system and individual devices. The OS uses
//! ACPI methods (AML bytecode in firmware tables) to transition
//! between power states, discover hardware topology, and manage
//! thermal zones. Power states range from S0 (working) through S5
//! (mechanical off), while device power states range from D0 (full on)
//! through D3 (off).

// ---------------------------------------------------------------------------
// System power states (Sx)
// ---------------------------------------------------------------------------

/// S0: Working (system fully on).
pub const ACPI_STATE_S0: u32 = 0;
/// S1: Sleep (CPU stops, RAM refreshed, fast resume).
pub const ACPI_STATE_S1: u32 = 1;
/// S2: Sleep (CPU powered off, RAM refreshed).
pub const ACPI_STATE_S2: u32 = 2;
/// S3: Suspend to RAM (most hardware off, RAM refreshed).
pub const ACPI_STATE_S3: u32 = 3;
/// S4: Hibernate (state saved to disk, power off).
pub const ACPI_STATE_S4: u32 = 4;
/// S5: Soft off (power fully off, only power button wakes).
pub const ACPI_STATE_S5: u32 = 5;

// ---------------------------------------------------------------------------
// Device power states (Dx)
// ---------------------------------------------------------------------------

/// D0: Fully on (device operational).
pub const ACPI_DEVICE_D0: u32 = 0;
/// D1: Light sleep (device-specific low power).
pub const ACPI_DEVICE_D1: u32 = 1;
/// D2: Deeper sleep (more power savings, longer resume).
pub const ACPI_DEVICE_D2: u32 = 2;
/// D3hot: Off but still powered (can be enumerated).
pub const ACPI_DEVICE_D3_HOT: u32 = 3;
/// D3cold: Fully off (power removed, not on bus).
pub const ACPI_DEVICE_D3_COLD: u32 = 4;

// ---------------------------------------------------------------------------
// ACPI GPE (General Purpose Event) types
// ---------------------------------------------------------------------------

/// Edge-triggered GPE.
pub const ACPI_GPE_EDGE_TRIGGERED: u32 = 0;
/// Level-triggered GPE.
pub const ACPI_GPE_LEVEL_TRIGGERED: u32 = 1;

// ---------------------------------------------------------------------------
// ACPI notify types
// ---------------------------------------------------------------------------

/// Bus check (device enumeration changed).
pub const ACPI_NOTIFY_BUS_CHECK: u32 = 0x00;
/// Device check (device insertion/removal).
pub const ACPI_NOTIFY_DEVICE_CHECK: u32 = 0x01;
/// Device wake (device woke the system).
pub const ACPI_NOTIFY_DEVICE_WAKE: u32 = 0x02;
/// Eject request.
pub const ACPI_NOTIFY_EJECT_REQUEST: u32 = 0x03;
/// Device check light (presence change).
pub const ACPI_NOTIFY_DEVICE_CHECK_LIGHT: u32 = 0x04;
/// Frequency mismatch.
pub const ACPI_NOTIFY_FREQUENCY_MISMATCH: u32 = 0x05;
/// Bus mode mismatch.
pub const ACPI_NOTIFY_BUS_MODE_MISMATCH: u32 = 0x06;
/// Power fault.
pub const ACPI_NOTIFY_POWER_FAULT: u32 = 0x07;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_states_ordered() {
        assert!(ACPI_STATE_S0 < ACPI_STATE_S1);
        assert!(ACPI_STATE_S1 < ACPI_STATE_S2);
        assert!(ACPI_STATE_S2 < ACPI_STATE_S3);
        assert!(ACPI_STATE_S3 < ACPI_STATE_S4);
        assert!(ACPI_STATE_S4 < ACPI_STATE_S5);
    }

    #[test]
    fn test_device_states_ordered() {
        assert!(ACPI_DEVICE_D0 < ACPI_DEVICE_D1);
        assert!(ACPI_DEVICE_D1 < ACPI_DEVICE_D2);
        assert!(ACPI_DEVICE_D2 < ACPI_DEVICE_D3_HOT);
        assert!(ACPI_DEVICE_D3_HOT < ACPI_DEVICE_D3_COLD);
    }

    #[test]
    fn test_gpe_types_distinct() {
        assert_ne!(ACPI_GPE_EDGE_TRIGGERED, ACPI_GPE_LEVEL_TRIGGERED);
    }

    #[test]
    fn test_notify_types_distinct() {
        let types = [
            ACPI_NOTIFY_BUS_CHECK,
            ACPI_NOTIFY_DEVICE_CHECK,
            ACPI_NOTIFY_DEVICE_WAKE,
            ACPI_NOTIFY_EJECT_REQUEST,
            ACPI_NOTIFY_DEVICE_CHECK_LIGHT,
            ACPI_NOTIFY_FREQUENCY_MISMATCH,
            ACPI_NOTIFY_BUS_MODE_MISMATCH,
            ACPI_NOTIFY_POWER_FAULT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
