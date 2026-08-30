//! `<linux/hidraw.h>` — raw HID userspace ioctls and limits.
//!
//! The hidraw driver exposes `/dev/hidrawN` so userspace tools
//! (libusb-driven device configuration, gaming-mouse software,
//! Steam Deck input remapping) can send raw HID reports without
//! a kernel input driver. Constants below cover the ioctls and
//! the report-descriptor / device-info structures.

// ---------------------------------------------------------------------------
// hidraw_report_descriptor / device-info limits
// ---------------------------------------------------------------------------

/// Maximum size of a raw report descriptor in bytes.
pub const HID_MAX_DESCRIPTOR_SIZE: u32 = 4096;
/// Maximum length of the bus-info string returned by HIDIOCGRAWINFO
/// helpers (vendor:product:version is fixed-size).
pub const HID_NAME_SIZE: u32 = 256;
/// Maximum length of the physical-location string.
pub const HID_PHYS_SIZE: u32 = 64;
/// Maximum length of the unique-id string.
pub const HID_UNIQ_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Bus types (struct hidraw_devinfo.bustype)
// ---------------------------------------------------------------------------

/// PCI bus.
pub const BUS_PCI: u16 = 0x01;
/// ISA-PNP bus.
pub const BUS_ISAPNP: u16 = 0x02;
/// USB bus.
pub const BUS_USB: u16 = 0x03;
/// HIL (HP Human Interface Loop).
pub const BUS_HIL: u16 = 0x04;
/// Bluetooth.
pub const BUS_BLUETOOTH: u16 = 0x05;
/// Virtual bus (e.g. uinput).
pub const BUS_VIRTUAL: u16 = 0x06;
/// I2C bus.
pub const BUS_I2C: u16 = 0x18;
/// SPI bus.
pub const BUS_SPI: u16 = 0x1c;

// ---------------------------------------------------------------------------
// ioctl numbers
// ---------------------------------------------------------------------------

/// `HIDIOCGRDESCSIZE` — get report descriptor size (int*).
pub const HIDIOCGRDESCSIZE: u32 = 0x8004_4801;
/// `HIDIOCGRDESC` — get report descriptor (struct hidraw_report_descriptor).
pub const HIDIOCGRDESC: u32 = 0x9020_4802;
/// `HIDIOCGRAWINFO` — get raw device info (struct hidraw_devinfo).
pub const HIDIOCGRAWINFO: u32 = 0x8008_4803;
/// `HIDIOCGRAWNAME(len)` base — get device name (variable).
pub const HIDIOCGRAWNAME_BASE: u32 = 0x8100_4804;
/// `HIDIOCGRAWPHYS(len)` base — get physical-location string.
pub const HIDIOCGRAWPHYS_BASE: u32 = 0x8100_4805;
/// `HIDIOCGRAWUNIQ(len)` base — get unique-id string.
pub const HIDIOCGRAWUNIQ_BASE: u32 = 0x8100_4808;
/// `HIDIOCSFEATURE(len)` base — set feature report.
pub const HIDIOCSFEATURE_BASE: u32 = 0xc000_4806;
/// `HIDIOCGFEATURE(len)` base — get feature report.
pub const HIDIOCGFEATURE_BASE: u32 = 0xc000_4807;
/// `HIDIOCSINPUT(len)` base — send input report.
pub const HIDIOCSINPUT_BASE: u32 = 0xc000_4809;
/// `HIDIOCGINPUT(len)` base — fetch latest input report.
pub const HIDIOCGINPUT_BASE: u32 = 0xc000_480a;
/// `HIDIOCSOUTPUT(len)` base — send output report.
pub const HIDIOCSOUTPUT_BASE: u32 = 0xc000_480b;
/// `HIDIOCGOUTPUT(len)` base — fetch latest output report.
pub const HIDIOCGOUTPUT_BASE: u32 = 0xc000_480c;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor_limit_pow2() {
        // 4 KiB matches the kernel's static buffer for HID descriptors.
        assert_eq!(HID_MAX_DESCRIPTOR_SIZE, 4096);
        assert!(HID_MAX_DESCRIPTOR_SIZE.is_power_of_two());
        assert!(HID_NAME_SIZE.is_power_of_two());
        assert!(HID_PHYS_SIZE.is_power_of_two());
        assert!(HID_UNIQ_SIZE.is_power_of_two());
        // Name field is the largest of the three string fields.
        assert!(HID_NAME_SIZE > HID_PHYS_SIZE);
        assert!(HID_NAME_SIZE > HID_UNIQ_SIZE);
    }

    #[test]
    fn test_bus_types_distinct_and_usb_known() {
        let b = [
            BUS_PCI,
            BUS_ISAPNP,
            BUS_USB,
            BUS_HIL,
            BUS_BLUETOOTH,
            BUS_VIRTUAL,
            BUS_I2C,
            BUS_SPI,
        ];
        for i in 0..b.len() {
            for j in (i + 1)..b.len() {
                assert_ne!(b[i], b[j]);
            }
        }
        // USB is the most common HID transport — well-known value 0x3.
        assert_eq!(BUS_USB, 3);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ops = [
            HIDIOCGRDESCSIZE,
            HIDIOCGRDESC,
            HIDIOCGRAWINFO,
            HIDIOCGRAWNAME_BASE,
            HIDIOCGRAWPHYS_BASE,
            HIDIOCGRAWUNIQ_BASE,
            HIDIOCSFEATURE_BASE,
            HIDIOCGFEATURE_BASE,
            HIDIOCSINPUT_BASE,
            HIDIOCGINPUT_BASE,
            HIDIOCSOUTPUT_BASE,
            HIDIOCGOUTPUT_BASE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_type_byte_is_h() {
        // All hidraw ioctls share type byte 'H' (0x48).
        for &n in &[
            HIDIOCGRDESCSIZE,
            HIDIOCGRDESC,
            HIDIOCGRAWINFO,
            HIDIOCGRAWNAME_BASE,
            HIDIOCGRAWPHYS_BASE,
            HIDIOCGRAWUNIQ_BASE,
            HIDIOCSFEATURE_BASE,
            HIDIOCGFEATURE_BASE,
        ] {
            // Type byte is in bits 8..15 of the ioctl number.
            assert_eq!((n >> 8) & 0xff, b'H' as u32);
        }
    }
}
