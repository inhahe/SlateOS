//! `<linux/device/bus.h>` — Bus subsystem constants.
//!
//! A bus represents a communication channel between the processor and
//! devices (PCI, USB, I2C, SPI, platform, etc.). Each bus type defines
//! how devices are discovered (enumeration), how drivers are matched
//! to devices, and how device resources (interrupts, I/O regions, DMA)
//! are configured. The bus subsystem handles hotplug notification,
//! power management coordination, and device/driver lifecycle.

// ---------------------------------------------------------------------------
// Bus types (well-known buses)
// ---------------------------------------------------------------------------

/// PCI / PCIe bus.
pub const BUS_TYPE_PCI: u32 = 0;
/// USB bus.
pub const BUS_TYPE_USB: u32 = 1;
/// I2C bus.
pub const BUS_TYPE_I2C: u32 = 2;
/// SPI bus.
pub const BUS_TYPE_SPI: u32 = 3;
/// Platform bus (memory-mapped, non-discoverable).
pub const BUS_TYPE_PLATFORM: u32 = 4;
/// ACPI bus.
pub const BUS_TYPE_ACPI: u32 = 5;
/// Device tree (OF) bus.
pub const BUS_TYPE_OF: u32 = 6;
/// Auxiliary bus (sub-function of a device).
pub const BUS_TYPE_AUXILIARY: u32 = 7;
/// Virtual bus (for virtual/software devices).
pub const BUS_TYPE_VIRTUAL: u32 = 8;
/// SDIO bus.
pub const BUS_TYPE_SDIO: u32 = 9;
/// MDIO bus (Ethernet PHY management).
pub const BUS_TYPE_MDIO: u32 = 10;
/// HID bus (Human Interface Devices).
pub const BUS_TYPE_HID: u32 = 11;

// ---------------------------------------------------------------------------
// Bus PM states
// ---------------------------------------------------------------------------

/// Bus is active (all devices accessible).
pub const BUS_PM_ACTIVE: u32 = 0;
/// Bus is in low-power state.
pub const BUS_PM_SUSPENDED: u32 = 1;
/// Bus is powered off.
pub const BUS_PM_OFF: u32 = 2;

// ---------------------------------------------------------------------------
// Bus flags
// ---------------------------------------------------------------------------

/// Bus supports hotplug.
pub const BUS_FLAG_HOTPLUG: u32 = 0x01;
/// Bus supports runtime PM.
pub const BUS_FLAG_PM_RUNTIME: u32 = 0x02;
/// Bus auto-probes new devices.
pub const BUS_FLAG_AUTO_PROBE: u32 = 0x04;
/// Bus supports DMA.
pub const BUS_FLAG_DMA: u32 = 0x08;
/// Bus is enumerated at boot only (no hotplug).
pub const BUS_FLAG_STATIC: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_types_distinct() {
        let types = [
            BUS_TYPE_PCI, BUS_TYPE_USB, BUS_TYPE_I2C, BUS_TYPE_SPI,
            BUS_TYPE_PLATFORM, BUS_TYPE_ACPI, BUS_TYPE_OF,
            BUS_TYPE_AUXILIARY, BUS_TYPE_VIRTUAL, BUS_TYPE_SDIO,
            BUS_TYPE_MDIO, BUS_TYPE_HID,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pm_states_distinct() {
        let states = [BUS_PM_ACTIVE, BUS_PM_SUSPENDED, BUS_PM_OFF];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            BUS_FLAG_HOTPLUG, BUS_FLAG_PM_RUNTIME,
            BUS_FLAG_AUTO_PROBE, BUS_FLAG_DMA, BUS_FLAG_STATIC,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
