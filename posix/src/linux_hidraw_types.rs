//! `<linux/hidraw.h>` — HID (Human Interface Device) raw access constants.
//!
//! hidraw provides raw access to HID devices (keyboards, mice,
//! gamepads, sensors, etc.) without going through the input subsystem.
//! Applications can read raw HID reports and send feature/output
//! reports directly. Used by game controllers, drawing tablets,
//! and custom HID devices that need protocol-level access.

// ---------------------------------------------------------------------------
// hidraw ioctl commands
// ---------------------------------------------------------------------------

/// Get raw HID report descriptor.
pub const HIDIOCGRDESCSIZE: u32 = 0x8004_4801;
/// Get raw report descriptor content.
pub const HIDIOCGRDESC: u32 = 0x9000_4802;
/// Get device info (bus, vendor, product).
pub const HIDIOCGRAWINFO: u32 = 0x8008_4803;
/// Get device name string.
pub const HIDIOCGRAWNAME: u32 = 0x9100_4804;
/// Get physical device path.
pub const HIDIOCGRAWPHYS: u32 = 0x9100_4805;
/// Send a feature report.
pub const HIDIOCSFEATURE: u32 = 0xC100_4806;
/// Get a feature report.
pub const HIDIOCGFEATURE: u32 = 0xC100_4807;
/// Get unique device ID string.
pub const HIDIOCGRAWUNIQ: u32 = 0x9100_4808;

// ---------------------------------------------------------------------------
// HID bus types
// ---------------------------------------------------------------------------

/// USB bus.
pub const BUS_USB: u32 = 0x03;
/// Bluetooth bus.
pub const BUS_BLUETOOTH: u32 = 0x05;
/// I2C bus (common for laptop touchpads).
pub const BUS_I2C: u32 = 0x18;
/// Virtual/software device.
pub const BUS_VIRTUAL: u32 = 0x06;

// ---------------------------------------------------------------------------
// HID report types
// ---------------------------------------------------------------------------

/// Input report (device → host).
pub const HID_INPUT_REPORT: u32 = 0;
/// Output report (host → device).
pub const HID_OUTPUT_REPORT: u32 = 1;
/// Feature report (bidirectional configuration).
pub const HID_FEATURE_REPORT: u32 = 2;

// ---------------------------------------------------------------------------
// HID descriptor limits
// ---------------------------------------------------------------------------

/// Maximum HID report descriptor size.
pub const HID_MAX_DESCRIPTOR_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            HIDIOCGRDESCSIZE,
            HIDIOCGRDESC,
            HIDIOCGRAWINFO,
            HIDIOCGRAWNAME,
            HIDIOCGRAWPHYS,
            HIDIOCSFEATURE,
            HIDIOCGFEATURE,
            HIDIOCGRAWUNIQ,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_bus_types_distinct() {
        let buses = [BUS_USB, BUS_BLUETOOTH, BUS_I2C, BUS_VIRTUAL];
        for i in 0..buses.len() {
            for j in (i + 1)..buses.len() {
                assert_ne!(buses[i], buses[j]);
            }
        }
    }

    #[test]
    fn test_report_types_distinct() {
        let reports = [HID_INPUT_REPORT, HID_OUTPUT_REPORT, HID_FEATURE_REPORT];
        for i in 0..reports.len() {
            for j in (i + 1)..reports.len() {
                assert_ne!(reports[i], reports[j]);
            }
        }
    }

    #[test]
    fn test_max_descriptor_size() {
        assert_eq!(HID_MAX_DESCRIPTOR_SIZE, 4096);
        assert!(HID_MAX_DESCRIPTOR_SIZE.is_power_of_two());
    }
}
