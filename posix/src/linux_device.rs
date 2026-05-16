//! `<linux/device.h>` — Device model constants.
//!
//! The device model represents hardware as a hierarchy of buses,
//! devices, and drivers. This module defines device types, power
//! states, and driver binding constants used throughout the kernel
//! driver framework.

// ---------------------------------------------------------------------------
// Device power states (dev_pm_info)
// ---------------------------------------------------------------------------

/// D0 — fully on.
pub const DEV_PM_STATE_D0: u32 = 0;
/// D1 — light sleep.
pub const DEV_PM_STATE_D1: u32 = 1;
/// D2 — deeper sleep.
pub const DEV_PM_STATE_D2: u32 = 2;
/// D3hot — device off but can wake.
pub const DEV_PM_STATE_D3HOT: u32 = 3;
/// D3cold — device and bus power off.
pub const DEV_PM_STATE_D3COLD: u32 = 4;

// ---------------------------------------------------------------------------
// Device link flags (DL_FLAG_*)
// ---------------------------------------------------------------------------

/// Stateless link (no state tracking).
pub const DL_FLAG_STATELESS: u32 = 1 << 0;
/// Auto-remove consumer (when consumer is unbound).
pub const DL_FLAG_AUTOREMOVE_CONSUMER: u32 = 1 << 1;
/// PM runtime link.
pub const DL_FLAG_PM_RUNTIME: u32 = 1 << 2;
/// RPM active at link time.
pub const DL_FLAG_RPM_ACTIVE: u32 = 1 << 3;
/// Auto-remove supplier.
pub const DL_FLAG_AUTOREMOVE_SUPPLIER: u32 = 1 << 4;
/// Auto-probe consumer.
pub const DL_FLAG_AUTOPROBE_CONSUMER: u32 = 1 << 5;
/// Managed link.
pub const DL_FLAG_MANAGED: u32 = 1 << 6;
/// Sync state only link.
pub const DL_FLAG_SYNC_STATE_ONLY: u32 = 1 << 7;
/// Inferred link (from firmware).
pub const DL_FLAG_INFERRED: u32 = 1 << 8;
/// Cycle detection.
pub const DL_FLAG_CYCLE: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Device link states
// ---------------------------------------------------------------------------

/// Not tracked.
pub const DL_STATE_NONE: u32 = 0;
/// Dormant.
pub const DL_STATE_DORMANT: u32 = 1;
/// Available.
pub const DL_STATE_AVAILABLE: u32 = 2;
/// Consumer probe.
pub const DL_STATE_CONSUMER_PROBE: u32 = 3;
/// Active.
pub const DL_STATE_ACTIVE: u32 = 4;
/// Supplier unbind.
pub const DL_STATE_SUPPLIER_UNBIND: u32 = 5;

// ---------------------------------------------------------------------------
// Bus types
// ---------------------------------------------------------------------------

/// PCI bus.
pub const BUS_TYPE_PCI: &str = "pci";
/// USB bus.
pub const BUS_TYPE_USB: &str = "usb";
/// Platform bus.
pub const BUS_TYPE_PLATFORM: &str = "platform";
/// I2C bus.
pub const BUS_TYPE_I2C: &str = "i2c";
/// SPI bus.
pub const BUS_TYPE_SPI: &str = "spi";
/// ACPI bus.
pub const BUS_TYPE_ACPI: &str = "acpi";
/// Virtual bus.
pub const BUS_TYPE_VIRTUAL: &str = "virtual";

// ---------------------------------------------------------------------------
// Driver probe return codes
// ---------------------------------------------------------------------------

/// Probe succeeded.
pub const PROBE_OK: i32 = 0;
/// Defer probing (dependency not ready yet).
pub const PROBE_DEFER: i32 = -517; // -EPROBE_DEFER

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pm_states_distinct() {
        let states = [
            DEV_PM_STATE_D0, DEV_PM_STATE_D1, DEV_PM_STATE_D2,
            DEV_PM_STATE_D3HOT, DEV_PM_STATE_D3COLD,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_pm_states_ordered() {
        assert!(DEV_PM_STATE_D0 < DEV_PM_STATE_D1);
        assert!(DEV_PM_STATE_D1 < DEV_PM_STATE_D2);
        assert!(DEV_PM_STATE_D2 < DEV_PM_STATE_D3HOT);
        assert!(DEV_PM_STATE_D3HOT < DEV_PM_STATE_D3COLD);
    }

    #[test]
    fn test_dl_flags_powers_of_two() {
        let flags = [
            DL_FLAG_STATELESS, DL_FLAG_AUTOREMOVE_CONSUMER,
            DL_FLAG_PM_RUNTIME, DL_FLAG_RPM_ACTIVE,
            DL_FLAG_AUTOREMOVE_SUPPLIER, DL_FLAG_AUTOPROBE_CONSUMER,
            DL_FLAG_MANAGED, DL_FLAG_SYNC_STATE_ONLY,
            DL_FLAG_INFERRED, DL_FLAG_CYCLE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_dl_flags_no_overlap() {
        let flags = [
            DL_FLAG_STATELESS, DL_FLAG_AUTOREMOVE_CONSUMER,
            DL_FLAG_PM_RUNTIME, DL_FLAG_RPM_ACTIVE,
            DL_FLAG_AUTOREMOVE_SUPPLIER, DL_FLAG_AUTOPROBE_CONSUMER,
            DL_FLAG_MANAGED, DL_FLAG_SYNC_STATE_ONLY,
            DL_FLAG_INFERRED, DL_FLAG_CYCLE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dl_states_distinct() {
        let states = [
            DL_STATE_NONE, DL_STATE_DORMANT, DL_STATE_AVAILABLE,
            DL_STATE_CONSUMER_PROBE, DL_STATE_ACTIVE,
            DL_STATE_SUPPLIER_UNBIND,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_bus_types_distinct() {
        let types = [
            BUS_TYPE_PCI, BUS_TYPE_USB, BUS_TYPE_PLATFORM,
            BUS_TYPE_I2C, BUS_TYPE_SPI, BUS_TYPE_ACPI,
            BUS_TYPE_VIRTUAL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_probe_codes() {
        assert_eq!(PROBE_OK, 0);
        assert!(PROBE_DEFER < 0);
    }
}
