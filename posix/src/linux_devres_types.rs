//! `<linux/device.h>` (devres subset) — Device resource management constants.
//!
//! devres (device resource management) provides automatic cleanup of
//! resources when a device is unbound from its driver. Resources
//! (memory, IRQs, I/O regions, clocks, regulators) are tracked in a
//! per-device list and released in reverse order when the driver
//! detaches. This eliminates resource leaks in error paths and makes
//! driver probe/remove functions simpler and more reliable.

// ---------------------------------------------------------------------------
// devres resource types
// ---------------------------------------------------------------------------

/// Managed memory allocation (devm_kmalloc).
pub const DEVRES_TYPE_MEM: u32 = 0;
/// Managed IRQ (devm_request_irq).
pub const DEVRES_TYPE_IRQ: u32 = 1;
/// Managed I/O memory region (devm_ioremap).
pub const DEVRES_TYPE_IOMEM: u32 = 2;
/// Managed I/O port region (devm_request_region).
pub const DEVRES_TYPE_IOPORT: u32 = 3;
/// Managed DMA allocation (devm_kmalloc + dma_alloc).
pub const DEVRES_TYPE_DMA: u32 = 4;
/// Managed clock (devm_clk_get).
pub const DEVRES_TYPE_CLK: u32 = 5;
/// Managed regulator (devm_regulator_get).
pub const DEVRES_TYPE_REGULATOR: u32 = 6;
/// Managed GPIO (devm_gpio_request).
pub const DEVRES_TYPE_GPIO: u32 = 7;
/// Managed reset control (devm_reset_control_get).
pub const DEVRES_TYPE_RESET: u32 = 8;
/// Managed PHY (devm_phy_get).
pub const DEVRES_TYPE_PHY: u32 = 9;
/// Managed pinctrl (devm_pinctrl_get).
pub const DEVRES_TYPE_PINCTRL: u32 = 10;
/// Custom action (devm_add_action).
pub const DEVRES_TYPE_ACTION: u32 = 11;

// ---------------------------------------------------------------------------
// devres group flags
// ---------------------------------------------------------------------------

/// Resource belongs to a group.
pub const DEVRES_FLAG_GROUP: u32 = 0x01;
/// Resource group is open (accepting new members).
pub const DEVRES_FLAG_GROUP_OPEN: u32 = 0x02;
/// Resource marked for release on next detach.
pub const DEVRES_FLAG_RELEASE: u32 = 0x04;

// ---------------------------------------------------------------------------
// devres limits
// ---------------------------------------------------------------------------

/// Maximum number of managed resources per device.
pub const DEVRES_MAX_PER_DEVICE: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            DEVRES_TYPE_MEM, DEVRES_TYPE_IRQ, DEVRES_TYPE_IOMEM,
            DEVRES_TYPE_IOPORT, DEVRES_TYPE_DMA, DEVRES_TYPE_CLK,
            DEVRES_TYPE_REGULATOR, DEVRES_TYPE_GPIO, DEVRES_TYPE_RESET,
            DEVRES_TYPE_PHY, DEVRES_TYPE_PINCTRL, DEVRES_TYPE_ACTION,
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
            DEVRES_FLAG_GROUP, DEVRES_FLAG_GROUP_OPEN,
            DEVRES_FLAG_RELEASE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert!(DEVRES_MAX_PER_DEVICE > 0);
    }
}
