//! `<linux/platform_device.h>` — Platform device/driver constants.
//!
//! Platform devices are non-discoverable devices described by
//! firmware (device tree, ACPI) rather than bus enumeration. They
//! include SoC peripherals, memory-mapped I/O controllers, and
//! interrupt controllers that lack self-identification.

// ---------------------------------------------------------------------------
// Platform resource types (struct resource.flags)
// ---------------------------------------------------------------------------

/// I/O port resource.
pub const IORESOURCE_IO: u32 = 0x0000_0100;
/// Memory-mapped I/O resource.
pub const IORESOURCE_MEM: u32 = 0x0000_0200;
/// IRQ resource.
pub const IORESOURCE_IRQ: u32 = 0x0000_0400;
/// DMA channel resource.
pub const IORESOURCE_DMA: u32 = 0x0000_0800;
/// Bus number resource (PCI).
pub const IORESOURCE_BUS: u32 = 0x0000_1000;

// ---------------------------------------------------------------------------
// Resource flags (combined with type)
// ---------------------------------------------------------------------------

/// Resource is prefetchable.
pub const IORESOURCE_PREFETCH: u32 = 0x0000_2000;
/// Resource is read-only.
pub const IORESOURCE_READONLY: u32 = 0x0000_4000;
/// Resource is cacheable.
pub const IORESOURCE_CACHEABLE: u32 = 0x0000_8000;
/// Resource is currently busy/in-use.
pub const IORESOURCE_BUSY: u32 = 0x8000_0000;
/// Resource disabled.
pub const IORESOURCE_DISABLED: u32 = 0x1000_0000;
/// Resource is unset/unknown.
pub const IORESOURCE_UNSET: u32 = 0x2000_0000;
/// Auto-size resource.
pub const IORESOURCE_AUTO: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// Platform device IDs
// ---------------------------------------------------------------------------

/// Auto-assigned platform device ID.
pub const PLATFORM_DEVID_AUTO: i32 = -2;
/// No device ID (single instance).
pub const PLATFORM_DEVID_NONE: i32 = -1;

// ---------------------------------------------------------------------------
// Driver probe types
// ---------------------------------------------------------------------------

/// Synchronous probe (default).
pub const PROBE_DEFAULT_STRATEGY: u32 = 0;
/// Prefer async probe for this driver.
pub const PROBE_PREFER_ASYNCHRONOUS: u32 = 1;
/// Force sync probe.
pub const PROBE_FORCE_SYNCHRONOUS: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_types_no_overlap() {
        let types = [
            IORESOURCE_IO, IORESOURCE_MEM, IORESOURCE_IRQ,
            IORESOURCE_DMA, IORESOURCE_BUS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_resource_flags_distinct() {
        let flags = [
            IORESOURCE_PREFETCH, IORESOURCE_READONLY,
            IORESOURCE_CACHEABLE, IORESOURCE_BUSY,
            IORESOURCE_DISABLED, IORESOURCE_UNSET, IORESOURCE_AUTO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_platform_devid() {
        assert_ne!(PLATFORM_DEVID_AUTO, PLATFORM_DEVID_NONE);
        assert!(PLATFORM_DEVID_AUTO < 0);
        assert!(PLATFORM_DEVID_NONE < 0);
    }

    #[test]
    fn test_probe_types_distinct() {
        assert_ne!(PROBE_DEFAULT_STRATEGY, PROBE_PREFER_ASYNCHRONOUS);
        assert_ne!(PROBE_PREFER_ASYNCHRONOUS, PROBE_FORCE_SYNCHRONOUS);
    }
}
