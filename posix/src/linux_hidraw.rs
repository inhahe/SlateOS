//! `<linux/hidraw.h>` — HID raw device access.
//!
//! Provides ioctl constants for interacting with HID devices via
//! `/dev/hidrawN`. Used by libusb, Steam Input, and many other
//! programs that need direct HID access.

// ---------------------------------------------------------------------------
// HIDRAW ioctl commands
// ---------------------------------------------------------------------------

/// Get raw device descriptor.
pub const HIDIOCGRDESCSIZE: u64 = 0x80044801;
/// Get raw report descriptor.
pub const HIDIOCGRDESC: u64 = 0x90044802;
/// Get raw device info.
pub const HIDIOCGRAWINFO: u64 = 0x80084803;
/// Get device name string.
pub const HIDIOCGRAWNAME_BASE: u64 = 0x80004804;
/// Get physical address string.
pub const HIDIOCGRAWPHYS_BASE: u64 = 0x80004805;
/// Get unique ID string.
pub const HIDIOCGRAWUNIQ_BASE: u64 = 0x80004808;

/// Send a feature report.
pub const HIDIOCSFEATURE_BASE: u64 = 0xC0004806;
/// Get a feature report.
pub const HIDIOCGFEATURE_BASE: u64 = 0xC0004807;

/// Send an output report.
pub const HIDIOCSINPUT_BASE: u64 = 0xC0004809;
/// Get an input report.
pub const HIDIOCGINPUT_BASE: u64 = 0xC000480A;

// ---------------------------------------------------------------------------
// HID bus types (from struct hidraw_devinfo)
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
// Structures
// ---------------------------------------------------------------------------

/// HID raw device info.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HidrawDevinfo {
    /// Bus type.
    pub bustype: u32,
    /// Vendor ID.
    pub vendor: i16,
    /// Product ID.
    pub product: i16,
}

impl HidrawDevinfo {
    /// Create a zeroed device info.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Maximum report descriptor size.
pub const HID_MAX_DESCRIPTOR_SIZE: usize = 4096;

/// HID report descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HidrawReportDescriptor {
    /// Size of the descriptor.
    pub size: u32,
    /// Descriptor data.
    pub value: [u8; HID_MAX_DESCRIPTOR_SIZE],
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_devinfo_size() {
        assert_eq!(core::mem::size_of::<HidrawDevinfo>(), 8);
    }

    #[test]
    fn test_devinfo_zeroed() {
        let info = HidrawDevinfo::zeroed();
        assert_eq!(info.bustype, 0);
        assert_eq!(info.vendor, 0);
        assert_eq!(info.product, 0);
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
    fn test_ioctl_commands_distinct() {
        let cmds = [
            HIDIOCGRDESCSIZE,
            HIDIOCGRDESC,
            HIDIOCGRAWINFO,
            HIDIOCGRAWNAME_BASE,
            HIDIOCGRAWPHYS_BASE,
            HIDIOCGRAWUNIQ_BASE,
            HIDIOCSFEATURE_BASE,
            HIDIOCGFEATURE_BASE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_max_descriptor_size() {
        assert_eq!(HID_MAX_DESCRIPTOR_SIZE, 4096);
    }
}
