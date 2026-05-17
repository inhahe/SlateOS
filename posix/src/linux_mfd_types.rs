//! `<linux/mfd/core.h>` — Multi-Function Device (MFD) framework constants.
//!
//! MFD devices are single physical chips that contain multiple
//! independent functional blocks. For example, a PMIC (Power
//! Management IC) might contain voltage regulators, an ADC, GPIO
//! pins, an RTC, and a charger controller — all on one chip behind
//! a single I2C/SPI bus address. The MFD framework creates child
//! platform devices for each function, each getting its own driver.

// ---------------------------------------------------------------------------
// MFD cell flags
// ---------------------------------------------------------------------------

/// Cell can be suspended independently.
pub const MFD_CELL_SUSPEND_LATE: u32 = 1 << 0;
/// Cell should be ignored (not instantiated).
pub const MFD_CELL_IGNORE: u32 = 1 << 1;
/// Cell DMA mask should be set.
pub const MFD_CELL_DMA_MASK: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// MFD device types (common MFD chip categories)
// ---------------------------------------------------------------------------

/// PMIC (Power Management IC).
pub const MFD_TYPE_PMIC: u32 = 0;
/// Audio codec (with other functions).
pub const MFD_TYPE_AUDIO_CODEC: u32 = 1;
/// Touch screen controller (with other functions).
pub const MFD_TYPE_TOUCHSCREEN: u32 = 2;
/// RTC + watchdog + GPIO combo.
pub const MFD_TYPE_RTC_COMBO: u32 = 3;
/// Connectivity hub (WiFi + BT + FM).
pub const MFD_TYPE_CONNECTIVITY: u32 = 4;

// ---------------------------------------------------------------------------
// MFD resource sharing modes
// ---------------------------------------------------------------------------

/// Resource is exclusive to this cell.
pub const MFD_SHARED_EXCLUSIVE: u32 = 0;
/// Resource is shared among cells (read-only sharing).
pub const MFD_SHARED_RO: u32 = 1;
/// Resource is shared with concurrent write access.
pub const MFD_SHARED_RW: u32 = 2;

// ---------------------------------------------------------------------------
// MFD add cell flags (for mfd_add_devices)
// ---------------------------------------------------------------------------

/// No platform data.
pub const MFD_ADD_NO_PDATA: u32 = 0;
/// Use parent's IRQ domain.
pub const MFD_ADD_PARENT_IRQ: u32 = 1;
/// Use device tree for resources.
pub const MFD_ADD_OF_NODE: u32 = 2;
/// Use ACPI for resources.
pub const MFD_ADD_ACPI: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_flags_no_overlap() {
        let flags = [MFD_CELL_SUSPEND_LATE, MFD_CELL_IGNORE, MFD_CELL_DMA_MASK];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_device_types_distinct() {
        let types = [
            MFD_TYPE_PMIC, MFD_TYPE_AUDIO_CODEC,
            MFD_TYPE_TOUCHSCREEN, MFD_TYPE_RTC_COMBO,
            MFD_TYPE_CONNECTIVITY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sharing_modes_distinct() {
        let modes = [MFD_SHARED_EXCLUSIVE, MFD_SHARED_RO, MFD_SHARED_RW];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_add_flags_distinct() {
        let flags = [
            MFD_ADD_NO_PDATA, MFD_ADD_PARENT_IRQ,
            MFD_ADD_OF_NODE, MFD_ADD_ACPI,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
