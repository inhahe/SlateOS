//! `<linux/hid.h>` — HID bus and device type constants.
//!
//! HID devices can connect over multiple transports (USB, Bluetooth,
//! I2C, Intel ISH). The HID bus type identifies the underlying
//! transport, affecting how the driver communicates with the device
//! and which features are available (e.g., power management).

// ---------------------------------------------------------------------------
// HID bus types (hid_device.bus)
// ---------------------------------------------------------------------------

/// USB HID transport.
pub const BUS_USB: u16 = 0x03;
/// Bluetooth HID transport.
pub const BUS_BLUETOOTH: u16 = 0x05;
/// I2C HID transport (common on laptops).
pub const BUS_I2C: u16 = 0x18;
/// SPI HID transport.
pub const BUS_SPI: u16 = 0x1C;
/// Virtual/software HID device (uhid).
pub const BUS_VIRTUAL: u16 = 0x06;
/// Intel Integrated Sensor Hub.
pub const BUS_INTEL_ISHTP: u16 = 0x44;

// ---------------------------------------------------------------------------
// HID device quirk flags
// ---------------------------------------------------------------------------

/// Device has broken descriptor.
pub const HID_QUIRK_BADPAD: u32 = 1 << 0;
/// Device sends extra reports.
pub const HID_QUIRK_MULTI_INPUT: u32 = 1 << 1;
/// Skip output reports.
pub const HID_QUIRK_NOGET: u32 = 1 << 2;
/// Device has inverted logic.
pub const HID_QUIRK_INVERT: u32 = 1 << 3;
/// Skip redundant HID init.
pub const HID_QUIRK_SKIP_OUTPUT_REPORTS: u32 = 1 << 4;
/// Device needs HID output reports sent as feature reports.
pub const HID_QUIRK_SKIP_OUTPUT_REPORT_ID: u32 = 1 << 5;
/// Device has non-unique report IDs.
pub const HID_QUIRK_NO_OUTPUT_REPORTS_ON_INTR_EP: u32 = 1 << 6;
/// Don't decode this device's reports.
pub const HID_QUIRK_NO_INIT_REPORTS: u32 = 1 << 7;
/// Always poll the device.
pub const HID_QUIRK_ALWAYS_POLL: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// HID report types (report ID prefix)
// ---------------------------------------------------------------------------

/// Input report (device → host).
pub const HID_INPUT_REPORT: u8 = 0;
/// Output report (host → device).
pub const HID_OUTPUT_REPORT: u8 = 1;
/// Feature report (bidirectional configuration).
pub const HID_FEATURE_REPORT: u8 = 2;

// ---------------------------------------------------------------------------
// HID connect flags (hid_hw_start mask)
// ---------------------------------------------------------------------------

/// Connect input subsystem (evdev).
pub const HID_CONNECT_HIDINPUT: u32 = 1 << 0;
/// Connect hiddev (raw HID userspace interface).
pub const HID_CONNECT_HIDDEV: u32 = 1 << 1;
/// Connect hidraw (raw reports).
pub const HID_CONNECT_HIDRAW: u32 = 1 << 2;
/// Connect force feedback.
pub const HID_CONNECT_FF: u32 = 1 << 3;
/// Connect all interfaces.
pub const HID_CONNECT_DEFAULT: u32 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_types_distinct() {
        let buses = [
            BUS_USB, BUS_BLUETOOTH, BUS_I2C,
            BUS_SPI, BUS_VIRTUAL, BUS_INTEL_ISHTP,
        ];
        for i in 0..buses.len() {
            for j in (i + 1)..buses.len() {
                assert_ne!(buses[i], buses[j]);
            }
        }
    }

    #[test]
    fn test_quirk_flags_are_bits() {
        let quirks = [
            HID_QUIRK_BADPAD, HID_QUIRK_MULTI_INPUT,
            HID_QUIRK_NOGET, HID_QUIRK_INVERT,
            HID_QUIRK_SKIP_OUTPUT_REPORTS,
            HID_QUIRK_SKIP_OUTPUT_REPORT_ID,
            HID_QUIRK_NO_OUTPUT_REPORTS_ON_INTR_EP,
            HID_QUIRK_NO_INIT_REPORTS, HID_QUIRK_ALWAYS_POLL,
        ];
        for i in 0..quirks.len() {
            assert!(quirks[i].is_power_of_two());
            for j in (i + 1)..quirks.len() {
                assert_eq!(quirks[i] & quirks[j], 0);
            }
        }
    }

    #[test]
    fn test_report_types_distinct() {
        assert_ne!(HID_INPUT_REPORT, HID_OUTPUT_REPORT);
        assert_ne!(HID_OUTPUT_REPORT, HID_FEATURE_REPORT);
        assert_ne!(HID_INPUT_REPORT, HID_FEATURE_REPORT);
    }

    #[test]
    fn test_connect_flags() {
        let flags = [
            HID_CONNECT_HIDINPUT, HID_CONNECT_HIDDEV,
            HID_CONNECT_HIDRAW, HID_CONNECT_FF,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
        }
        let combined = HID_CONNECT_HIDINPUT | HID_CONNECT_HIDDEV
            | HID_CONNECT_HIDRAW | HID_CONNECT_FF;
        assert_eq!(HID_CONNECT_DEFAULT, combined);
    }
}
