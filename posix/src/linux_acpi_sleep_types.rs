//! `<linux/acpi.h>` — ACPI sleep state and power management constants.
//!
//! ACPI defines system sleep states (S0-S5) and device power states
//! (D0-D3). The OS orchestrates transitions between these states
//! for system suspend, hibernate, and per-device power management.
//! Each state has different latency, power, and wake characteristics.

// ---------------------------------------------------------------------------
// System sleep states (Sx)
// ---------------------------------------------------------------------------

/// S0: Working (fully on).
pub const ACPI_STATE_S0: u8 = 0;
/// S1: Power on suspend (CPU stops, RAM refreshed).
pub const ACPI_STATE_S1: u8 = 1;
/// S2: CPU powered off, RAM refreshed.
pub const ACPI_STATE_S2: u8 = 2;
/// S3: Suspend to RAM (STR) — only RAM powered.
pub const ACPI_STATE_S3: u8 = 3;
/// S4: Suspend to disk (hibernate) — all power off, state on disk.
pub const ACPI_STATE_S4: u8 = 4;
/// S5: Soft off (mechanical off equivalent).
pub const ACPI_STATE_S5: u8 = 5;

// ---------------------------------------------------------------------------
// Device power states (Dx)
// ---------------------------------------------------------------------------

/// D0: Fully on (device operational).
pub const ACPI_STATE_D0: u8 = 0;
/// D1: Light sleep (device-specific, low latency).
pub const ACPI_STATE_D1: u8 = 1;
/// D2: Deeper sleep (more power savings).
pub const ACPI_STATE_D2: u8 = 2;
/// D3hot: Device off but power applied (can restore quickly).
pub const ACPI_STATE_D3_HOT: u8 = 3;
/// D3cold: Device fully off (power removed).
pub const ACPI_STATE_D3_COLD: u8 = 4;

// ---------------------------------------------------------------------------
// Sleep type register values (SLP_TYP)
// ---------------------------------------------------------------------------

/// SLP_TYP field mask in PM1_CNT register.
pub const ACPI_SLP_TYP_MASK: u16 = 0x1C00;
/// SLP_TYP field shift in PM1_CNT register.
pub const ACPI_SLP_TYP_SHIFT: u16 = 10;
/// SLP_EN bit in PM1_CNT (trigger sleep).
pub const ACPI_SLP_EN: u16 = 0x2000;

// ---------------------------------------------------------------------------
// Wake capabilities
// ---------------------------------------------------------------------------

/// Device can wake from S1.
pub const ACPI_WAKE_S1: u32 = 1 << 0;
/// Device can wake from S2.
pub const ACPI_WAKE_S2: u32 = 1 << 1;
/// Device can wake from S3.
pub const ACPI_WAKE_S3: u32 = 1 << 2;
/// Device can wake from S4.
pub const ACPI_WAKE_S4: u32 = 1 << 3;
/// Device can wake from S5.
pub const ACPI_WAKE_S5: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// PM1 control register bits
// ---------------------------------------------------------------------------

/// SCI enable (route events to SCI, not SMI).
pub const ACPI_PM1_SCI_EN: u16 = 0x0001;
/// Bus master reload (restart arbiter).
pub const ACPI_PM1_BM_RLD: u16 = 0x0002;
/// Global lock release.
pub const ACPI_PM1_GBL_RLS: u16 = 0x0004;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep_states_sequential() {
        assert_eq!(ACPI_STATE_S0, 0);
        assert_eq!(ACPI_STATE_S1, 1);
        assert_eq!(ACPI_STATE_S2, 2);
        assert_eq!(ACPI_STATE_S3, 3);
        assert_eq!(ACPI_STATE_S4, 4);
        assert_eq!(ACPI_STATE_S5, 5);
    }

    #[test]
    fn test_device_states_sequential() {
        assert_eq!(ACPI_STATE_D0, 0);
        assert_eq!(ACPI_STATE_D1, 1);
        assert_eq!(ACPI_STATE_D2, 2);
        assert_eq!(ACPI_STATE_D3_HOT, 3);
        assert_eq!(ACPI_STATE_D3_COLD, 4);
    }

    #[test]
    fn test_slp_en_bit() {
        assert_eq!(ACPI_SLP_EN, 0x2000);
        assert!(ACPI_SLP_EN & ACPI_SLP_TYP_MASK == 0);
    }

    #[test]
    fn test_wake_caps_no_overlap() {
        let caps = [
            ACPI_WAKE_S1,
            ACPI_WAKE_S2,
            ACPI_WAKE_S3,
            ACPI_WAKE_S4,
            ACPI_WAKE_S5,
        ];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_pm1_bits_no_overlap() {
        assert_eq!(ACPI_PM1_SCI_EN & ACPI_PM1_BM_RLD, 0);
        assert_eq!(ACPI_PM1_BM_RLD & ACPI_PM1_GBL_RLS, 0);
        assert_eq!(ACPI_PM1_SCI_EN & ACPI_PM1_GBL_RLS, 0);
    }
}
