//! `<linux/device/driver.h>` — Driver model constants.
//!
//! The Linux driver model organizes the relationship between devices
//! (physical/virtual hardware), drivers (code that operates hardware),
//! and buses (communication channels between CPU and devices). A driver
//! registers with a bus, the bus matches devices to drivers via match
//! tables (PCI IDs, ACPI IDs, device tree compatible strings, etc.),
//! and calls the driver's probe() function when a match is found.

// ---------------------------------------------------------------------------
// Driver probe return values
// ---------------------------------------------------------------------------

/// Probe succeeded.
pub const DRIVER_PROBE_OK: i32 = 0;
/// Probe deferred (dependency not yet available).
pub const DRIVER_PROBE_DEFER: i32 = -517; // -EPROBE_DEFER
/// Probe failed, don't retry.
pub const DRIVER_PROBE_FAIL: i32 = -1;

// ---------------------------------------------------------------------------
// Driver binding states
// ---------------------------------------------------------------------------

/// Driver is not bound to any device.
pub const DRIVER_STATE_UNBOUND: u32 = 0;
/// Driver is being probed.
pub const DRIVER_STATE_PROBING: u32 = 1;
/// Driver is bound to a device.
pub const DRIVER_STATE_BOUND: u32 = 2;
/// Driver is being unbound.
pub const DRIVER_STATE_UNBINDING: u32 = 3;

// ---------------------------------------------------------------------------
// Device match types
// ---------------------------------------------------------------------------

/// Match by name string.
pub const MATCH_TYPE_NAME: u32 = 0;
/// Match by PCI vendor/device ID.
pub const MATCH_TYPE_PCI_ID: u32 = 1;
/// Match by USB vendor/product ID.
pub const MATCH_TYPE_USB_ID: u32 = 2;
/// Match by ACPI hardware ID.
pub const MATCH_TYPE_ACPI_ID: u32 = 3;
/// Match by device tree compatible string.
pub const MATCH_TYPE_OF_COMPATIBLE: u32 = 4;
/// Match by platform device name.
pub const MATCH_TYPE_PLATFORM: u32 = 5;
/// Match by I2C device address/name.
pub const MATCH_TYPE_I2C: u32 = 6;
/// Match by SPI chip select/name.
pub const MATCH_TYPE_SPI: u32 = 7;

// ---------------------------------------------------------------------------
// Driver flags
// ---------------------------------------------------------------------------

/// Driver supports runtime PM.
pub const DRIVER_FLAG_PM_RUNTIME: u32 = 0x01;
/// Driver can be unbound at runtime.
pub const DRIVER_FLAG_ALLOW_UNBIND: u32 = 0x02;
/// Driver does not support suspend/resume.
pub const DRIVER_FLAG_NO_PM: u32 = 0x04;
/// Driver probe can be deferred.
pub const DRIVER_FLAG_PROBE_DEFER: u32 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_returns_distinct() {
        assert_ne!(DRIVER_PROBE_OK, DRIVER_PROBE_DEFER);
        assert_ne!(DRIVER_PROBE_OK, DRIVER_PROBE_FAIL);
        assert_ne!(DRIVER_PROBE_DEFER, DRIVER_PROBE_FAIL);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            DRIVER_STATE_UNBOUND,
            DRIVER_STATE_PROBING,
            DRIVER_STATE_BOUND,
            DRIVER_STATE_UNBINDING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_match_types_distinct() {
        let types = [
            MATCH_TYPE_NAME,
            MATCH_TYPE_PCI_ID,
            MATCH_TYPE_USB_ID,
            MATCH_TYPE_ACPI_ID,
            MATCH_TYPE_OF_COMPATIBLE,
            MATCH_TYPE_PLATFORM,
            MATCH_TYPE_I2C,
            MATCH_TYPE_SPI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            DRIVER_FLAG_PM_RUNTIME,
            DRIVER_FLAG_ALLOW_UNBIND,
            DRIVER_FLAG_NO_PM,
            DRIVER_FLAG_PROBE_DEFER,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
