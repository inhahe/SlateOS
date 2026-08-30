//! `<linux/platform_device.h>` — Platform device constants.
//!
//! Platform devices represent non-discoverable hardware that is
//! described by firmware (device tree, ACPI) rather than by bus
//! enumeration. Common on SoCs where peripherals are memory-mapped
//! at fixed addresses (UART, GPIO, SPI controllers, etc.).

// ---------------------------------------------------------------------------
// Resource types (from <linux/ioport.h> but used by platform devices)
// ---------------------------------------------------------------------------

/// I/O port resource.
pub const IORESOURCE_IO: u32 = 0x0000_0100;
/// Memory-mapped I/O resource.
pub const IORESOURCE_MEM: u32 = 0x0000_0200;
/// IRQ resource.
pub const IORESOURCE_IRQ: u32 = 0x0000_0400;
/// DMA resource.
pub const IORESOURCE_DMA: u32 = 0x0000_0800;
/// Bus number resource.
pub const IORESOURCE_BUS: u32 = 0x0000_1000;

// ---------------------------------------------------------------------------
// Resource flags
// ---------------------------------------------------------------------------

/// Resource is disabled.
pub const IORESOURCE_DISABLED: u32 = 0x1000_0000;
/// Resource auto-detected (not firmware-specified).
pub const IORESOURCE_AUTO: u32 = 0x4000_0000;
/// Resource is busy.
pub const IORESOURCE_BUSY: u32 = 0x8000_0000;
/// Read-only resource.
pub const IORESOURCE_READONLY: u32 = 0x0008_0000;
/// Cacheable resource.
pub const IORESOURCE_CACHEABLE: u32 = 0x0004_0000;
/// Prefetchable resource.
pub const IORESOURCE_PREFETCH: u32 = 0x0010_0000;

// ---------------------------------------------------------------------------
// Platform device ID
// ---------------------------------------------------------------------------

/// Auto-assign device ID.
pub const PLATFORM_DEVID_AUTO: i32 = -2;
/// No device ID.
pub const PLATFORM_DEVID_NONE: i32 = -1;

// ---------------------------------------------------------------------------
// Platform device flags
// ---------------------------------------------------------------------------

/// Device data is reference-counted.
pub const PLATFORM_FLAG_DMA_COHERENT: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// IRQ resource flags (trigger types)
// ---------------------------------------------------------------------------

/// IRQ is rising-edge triggered.
pub const IORESOURCE_IRQ_HIGHEDGE: u32 = 1 << 0;
/// IRQ is falling-edge triggered.
pub const IORESOURCE_IRQ_LOWEDGE: u32 = 1 << 1;
/// IRQ is active-high level triggered.
pub const IORESOURCE_IRQ_HIGHLEVEL: u32 = 1 << 2;
/// IRQ is active-low level triggered.
pub const IORESOURCE_IRQ_LOWLEVEL: u32 = 1 << 3;
/// Shareable IRQ.
pub const IORESOURCE_IRQ_SHAREABLE: u32 = 1 << 4;
/// Optional IRQ.
pub const IORESOURCE_IRQ_OPTIONAL: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_types_distinct() {
        let types = [
            IORESOURCE_IO,
            IORESOURCE_MEM,
            IORESOURCE_IRQ,
            IORESOURCE_DMA,
            IORESOURCE_BUS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_resource_types_no_overlap() {
        let types = [
            IORESOURCE_IO,
            IORESOURCE_MEM,
            IORESOURCE_IRQ,
            IORESOURCE_DMA,
            IORESOURCE_BUS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_devid_values() {
        assert_eq!(PLATFORM_DEVID_AUTO, -2);
        assert_eq!(PLATFORM_DEVID_NONE, -1);
        assert_ne!(PLATFORM_DEVID_AUTO, PLATFORM_DEVID_NONE);
    }

    #[test]
    fn test_irq_flags_powers_of_two() {
        let flags = [
            IORESOURCE_IRQ_HIGHEDGE,
            IORESOURCE_IRQ_LOWEDGE,
            IORESOURCE_IRQ_HIGHLEVEL,
            IORESOURCE_IRQ_LOWLEVEL,
            IORESOURCE_IRQ_SHAREABLE,
            IORESOURCE_IRQ_OPTIONAL,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_irq_flags_no_overlap() {
        let flags = [
            IORESOURCE_IRQ_HIGHEDGE,
            IORESOURCE_IRQ_LOWEDGE,
            IORESOURCE_IRQ_HIGHLEVEL,
            IORESOURCE_IRQ_LOWLEVEL,
            IORESOURCE_IRQ_SHAREABLE,
            IORESOURCE_IRQ_OPTIONAL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_resource_flags_distinct() {
        let flags = [
            IORESOURCE_DISABLED,
            IORESOURCE_AUTO,
            IORESOURCE_BUSY,
            IORESOURCE_READONLY,
            IORESOURCE_CACHEABLE,
            IORESOURCE_PREFETCH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
