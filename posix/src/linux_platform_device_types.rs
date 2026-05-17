//! `<linux/platform_device.h>` — Platform device/driver constants.
//!
//! Platform devices represent non-discoverable devices that are
//! described by firmware (device tree, ACPI) or board files rather
//! than being enumerated on a bus (like PCI or USB). Examples include
//! SoC peripherals (GPIO controllers, I2C adapters, SPI controllers,
//! DMA engines, timers) that are memory-mapped and described in the
//! device tree. The platform bus matches devices to drivers by name
//! or compatible string.

// ---------------------------------------------------------------------------
// Platform device resource types
// ---------------------------------------------------------------------------

/// I/O memory (MMIO) resource.
pub const PLATFORM_RES_MEM: u32 = 0;
/// IRQ resource.
pub const PLATFORM_RES_IRQ: u32 = 1;
/// DMA channel resource.
pub const PLATFORM_RES_DMA: u32 = 2;
/// I/O port resource.
pub const PLATFORM_RES_IO: u32 = 3;
/// Bus resource.
pub const PLATFORM_RES_BUS: u32 = 4;

// ---------------------------------------------------------------------------
// Platform device flags
// ---------------------------------------------------------------------------

/// Device was added by board code (not firmware).
pub const PLATFORM_DEVFLAG_BOARD: u32 = 1 << 0;
/// Device was added dynamically (at runtime).
pub const PLATFORM_DEVFLAG_DYNAMIC: u32 = 1 << 1;
/// Device's IRQ resources have been overridden.
pub const PLATFORM_DEVFLAG_IRQ_OVERRIDE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Platform driver probe types
// ---------------------------------------------------------------------------

/// Normal probe (during driver registration or device add).
pub const PLATFORM_PROBE_NORMAL: u32 = 0;
/// Deferred probe (dependency not yet available, try later).
pub const PLATFORM_PROBE_DEFERRED: u32 = 1;
/// Probe skipped (device disabled or blacklisted).
pub const PLATFORM_PROBE_SKIPPED: u32 = 2;

// ---------------------------------------------------------------------------
// Platform device ID modes
// ---------------------------------------------------------------------------

/// Match by name only.
pub const PLATFORM_ID_NAME: u32 = 0;
/// Match by device tree compatible.
pub const PLATFORM_ID_OF: u32 = 1;
/// Match by ACPI ID.
pub const PLATFORM_ID_ACPI: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_types_distinct() {
        let types = [
            PLATFORM_RES_MEM, PLATFORM_RES_IRQ, PLATFORM_RES_DMA,
            PLATFORM_RES_IO, PLATFORM_RES_BUS,
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
            PLATFORM_DEVFLAG_BOARD, PLATFORM_DEVFLAG_DYNAMIC,
            PLATFORM_DEVFLAG_IRQ_OVERRIDE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_probe_types_distinct() {
        let types = [
            PLATFORM_PROBE_NORMAL, PLATFORM_PROBE_DEFERRED,
            PLATFORM_PROBE_SKIPPED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_id_modes_distinct() {
        let modes = [PLATFORM_ID_NAME, PLATFORM_ID_OF, PLATFORM_ID_ACPI];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
