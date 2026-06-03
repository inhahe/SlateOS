//! `<linux/mfd/core.h>` — Multi-Function Device constants.
//!
//! MFD (Multi-Function Device) drivers manage integrated circuits
//! that contain multiple distinct functional blocks (e.g., a PMIC
//! with regulators, GPIO, and RTC; or an audio codec with ADC, DAC,
//! and GPIO). The MFD core provides cell registration and resource
//! sharing between sub-devices.

// ---------------------------------------------------------------------------
// MFD cell capabilities
// ---------------------------------------------------------------------------

/// Cell can be suspended independently.
pub const MFD_CELL_SUSPEND: u32 = 1 << 0;
/// Cell shares an IRQ domain with parent.
pub const MFD_CELL_SHARED_IRQ: u32 = 1 << 1;
/// Cell has ACPI companion device.
pub const MFD_CELL_ACPI_MATCH: u32 = 1 << 2;
/// Cell has OF (device tree) companion.
pub const MFD_CELL_OF_MATCH: u32 = 1 << 3;
/// Cell does not require platform data.
pub const MFD_CELL_NO_PD: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// MFD resource types
// ---------------------------------------------------------------------------

/// Memory-mapped I/O resource.
pub const MFD_RES_MEM: u8 = 0;
/// I/O port resource.
pub const MFD_RES_IO: u8 = 1;
/// IRQ resource.
pub const MFD_RES_IRQ: u8 = 2;
/// DMA channel resource.
pub const MFD_RES_DMA: u8 = 3;
/// Bus (I2C/SPI) resource.
pub const MFD_RES_BUS: u8 = 4;

// ---------------------------------------------------------------------------
// Common MFD device classes
// ---------------------------------------------------------------------------

/// Power Management IC (PMIC).
pub const MFD_CLASS_PMIC: u16 = 0x0001;
/// Audio codec.
pub const MFD_CLASS_CODEC: u16 = 0x0002;
/// Touchscreen controller.
pub const MFD_CLASS_TOUCH: u16 = 0x0003;
/// GPIO expander.
pub const MFD_CLASS_GPIO: u16 = 0x0004;
/// Real-time clock.
pub const MFD_CLASS_RTC: u16 = 0x0005;
/// Backlight controller.
pub const MFD_CLASS_BACKLIGHT: u16 = 0x0006;
/// LED controller.
pub const MFD_CLASS_LED: u16 = 0x0007;
/// Watchdog timer.
pub const MFD_CLASS_WDT: u16 = 0x0008;
/// ADC (Analog-to-Digital Converter).
pub const MFD_CLASS_ADC: u16 = 0x0009;
/// PWM controller.
pub const MFD_CLASS_PWM: u16 = 0x000A;

// ---------------------------------------------------------------------------
// MFD add flags
// ---------------------------------------------------------------------------

/// Add cells during device probe.
pub const MFD_ADD_ON_PROBE: u32 = 1 << 0;
/// Remove cells during device remove.
pub const MFD_DEL_ON_REMOVE: u32 = 1 << 1;
/// Share parent's IRQ chip.
pub const MFD_SHARE_IRQ_CHIP: u32 = 1 << 2;
/// Use platform device IDs from parent.
pub const MFD_USE_PARENT_ID: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_caps_no_overlap() {
        let caps = [
            MFD_CELL_SUSPEND,
            MFD_CELL_SHARED_IRQ,
            MFD_CELL_ACPI_MATCH,
            MFD_CELL_OF_MATCH,
            MFD_CELL_NO_PD,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_cell_caps_power_of_two() {
        let caps = [
            MFD_CELL_SUSPEND,
            MFD_CELL_SHARED_IRQ,
            MFD_CELL_ACPI_MATCH,
            MFD_CELL_OF_MATCH,
            MFD_CELL_NO_PD,
        ];
        for c in &caps {
            assert!(c.is_power_of_two());
        }
    }

    #[test]
    fn test_resource_types_distinct() {
        let types = [
            MFD_RES_MEM,
            MFD_RES_IO,
            MFD_RES_IRQ,
            MFD_RES_DMA,
            MFD_RES_BUS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_device_classes_distinct() {
        let classes = [
            MFD_CLASS_PMIC,
            MFD_CLASS_CODEC,
            MFD_CLASS_TOUCH,
            MFD_CLASS_GPIO,
            MFD_CLASS_RTC,
            MFD_CLASS_BACKLIGHT,
            MFD_CLASS_LED,
            MFD_CLASS_WDT,
            MFD_CLASS_ADC,
            MFD_CLASS_PWM,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_add_flags_no_overlap() {
        let flags = [
            MFD_ADD_ON_PROBE,
            MFD_DEL_ON_REMOVE,
            MFD_SHARE_IRQ_CHIP,
            MFD_USE_PARENT_ID,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
