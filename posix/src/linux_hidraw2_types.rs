//! `<linux/hidraw.h>` — Additional HID raw constants.
//!
//! Supplementary HID raw constants covering report types,
//! device info flags, and ioctl commands.

// ---------------------------------------------------------------------------
// HID report types
// ---------------------------------------------------------------------------

/// Input report.
pub const HID_INPUT_REPORT: u8 = 0;
/// Output report.
pub const HID_OUTPUT_REPORT: u8 = 1;
/// Feature report.
pub const HID_FEATURE_REPORT: u8 = 2;

// ---------------------------------------------------------------------------
// HID bus types
// ---------------------------------------------------------------------------

/// USB bus.
pub const BUS_USB: u32 = 0x03;
/// HIL bus.
pub const BUS_HIL: u32 = 0x04;
/// Bluetooth bus.
pub const BUS_BLUETOOTH: u32 = 0x05;
/// Virtual bus.
pub const BUS_VIRTUAL: u32 = 0x06;
/// I2C bus.
pub const BUS_I2C: u32 = 0x18;
/// SPI bus.
pub const BUS_SPI: u32 = 0x1C;

// ---------------------------------------------------------------------------
// HID country codes
// ---------------------------------------------------------------------------

/// Not supported.
pub const HID_COUNTRY_NOT_SUPPORTED: u8 = 0;
/// US.
pub const HID_COUNTRY_US: u8 = 33;

// ---------------------------------------------------------------------------
// HIDRAW ioctl commands
// ---------------------------------------------------------------------------

/// Get raw device info.
pub const HIDIOCGRAWINFO: u32 = 0x80084803;
/// Get raw device name.
pub const HIDIOCGRAWNAME: u32 = 0x80044804;
/// Get physical address.
pub const HIDIOCGRAWPHYS: u32 = 0x80044805;
/// Get feature report.
pub const HIDIOCGFEATURE: u32 = 0xC0044807;
/// Set feature report.
pub const HIDIOCSFEATURE: u32 = 0xC0044806;
/// Get raw unique ID.
pub const HIDIOCGRAWUNIQ: u32 = 0x80044808;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_types_distinct() {
        let types = [HID_INPUT_REPORT, HID_OUTPUT_REPORT, HID_FEATURE_REPORT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_bus_types_distinct() {
        let buses = [
            BUS_USB,
            BUS_HIL,
            BUS_BLUETOOTH,
            BUS_VIRTUAL,
            BUS_I2C,
            BUS_SPI,
        ];
        for i in 0..buses.len() {
            for j in (i + 1)..buses.len() {
                assert_ne!(buses[i], buses[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            HIDIOCGRAWINFO,
            HIDIOCGRAWNAME,
            HIDIOCGRAWPHYS,
            HIDIOCGFEATURE,
            HIDIOCSFEATURE,
            HIDIOCGRAWUNIQ,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
