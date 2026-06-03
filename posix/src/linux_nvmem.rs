//! `<linux/nvmem-consumer.h>` / `<linux/nvmem-provider.h>` — NVMEM constants.
//!
//! The NVMEM (Non-Volatile Memory) framework provides a uniform
//! interface for reading/writing small non-volatile storage:
//! EEPROM, OTP fuses, battery-backed SRAM, EFUSE, etc. Consumers
//! access named cells; providers register the backing storage.

// ---------------------------------------------------------------------------
// NVMEM types
// ---------------------------------------------------------------------------

/// Unknown NVMEM type.
pub const NVMEM_TYPE_UNKNOWN: u32 = 0;
/// EEPROM.
pub const NVMEM_TYPE_EEPROM: u32 = 1;
/// One-Time Programmable (fuses).
pub const NVMEM_TYPE_OTP: u32 = 2;
/// Battery-backed RAM.
pub const NVMEM_TYPE_BATTERY_BACKED: u32 = 3;
/// FRAM (Ferroelectric RAM).
pub const NVMEM_TYPE_FRAM: u32 = 4;

// ---------------------------------------------------------------------------
// NVMEM flags
// ---------------------------------------------------------------------------

/// Read-only NVMEM.
pub const NVMEM_FLAG_READ_ONLY: u32 = 1 << 0;
/// Root-only access.
pub const NVMEM_FLAG_ROOT_ONLY: u32 = 1 << 1;
/// In-band ECC.
pub const NVMEM_FLAG_IN_BAND_ECC: u32 = 1 << 2;
/// Keep power on during standby.
pub const NVMEM_FLAG_KEEP_POWER: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Cell flags
// ---------------------------------------------------------------------------

/// Cell contains a MAC address.
pub const NVMEM_CELL_FLAG_MAC: u32 = 1 << 0;
/// Cell is post-processed (transformed after read).
pub const NVMEM_CELL_FLAG_POST_PROCESS: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Layout types
// ---------------------------------------------------------------------------

/// ONIE TLV layout.
pub const NVMEM_LAYOUT_ONIE_TLV: &str = "onie-tlv";
/// Device tree-based layout.
pub const NVMEM_LAYOUT_DT: &str = "device-tree";
/// SL28 layout (Kontron boards).
pub const NVMEM_LAYOUT_SL28_VPD: &str = "sl28-vpd";

// ---------------------------------------------------------------------------
// Common cell names
// ---------------------------------------------------------------------------

/// MAC address cell.
pub const NVMEM_CELL_MAC_ADDRESS: &str = "mac-address";
/// Serial number cell.
pub const NVMEM_CELL_SERIAL_NUMBER: &str = "serial-number";
/// Product name cell.
pub const NVMEM_CELL_PRODUCT_NAME: &str = "product-name";
/// Calibration data cell.
pub const NVMEM_CELL_CALIBRATION: &str = "calibration";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            NVMEM_TYPE_UNKNOWN,
            NVMEM_TYPE_EEPROM,
            NVMEM_TYPE_OTP,
            NVMEM_TYPE_BATTERY_BACKED,
            NVMEM_TYPE_FRAM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            NVMEM_FLAG_READ_ONLY,
            NVMEM_FLAG_ROOT_ONLY,
            NVMEM_FLAG_IN_BAND_ECC,
            NVMEM_FLAG_KEEP_POWER,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            NVMEM_FLAG_READ_ONLY,
            NVMEM_FLAG_ROOT_ONLY,
            NVMEM_FLAG_IN_BAND_ECC,
            NVMEM_FLAG_KEEP_POWER,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cell_flags_powers_of_two() {
        assert!(NVMEM_CELL_FLAG_MAC.is_power_of_two());
        assert!(NVMEM_CELL_FLAG_POST_PROCESS.is_power_of_two());
    }

    #[test]
    fn test_cell_flags_no_overlap() {
        assert_eq!(NVMEM_CELL_FLAG_MAC & NVMEM_CELL_FLAG_POST_PROCESS, 0);
    }

    #[test]
    fn test_layout_names_distinct() {
        let layouts = [
            NVMEM_LAYOUT_ONIE_TLV,
            NVMEM_LAYOUT_DT,
            NVMEM_LAYOUT_SL28_VPD,
        ];
        for i in 0..layouts.len() {
            for j in (i + 1)..layouts.len() {
                assert_ne!(layouts[i], layouts[j]);
            }
        }
    }

    #[test]
    fn test_cell_names_distinct() {
        let names = [
            NVMEM_CELL_MAC_ADDRESS,
            NVMEM_CELL_SERIAL_NUMBER,
            NVMEM_CELL_PRODUCT_NAME,
            NVMEM_CELL_CALIBRATION,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }
}
