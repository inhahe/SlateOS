//! `<linux/uhid.h>` — UHID (User-space HID) constants.
//!
//! UHID allows creating virtual HID devices from userspace.
//! These constants define event types, device flags,
//! and protocol parameters.

// ---------------------------------------------------------------------------
// UHID event types
// ---------------------------------------------------------------------------

/// Create device (deprecated v1).
pub const UHID_CREATE: u32 = 0;
/// Destroy device.
pub const UHID_DESTROY: u32 = 1;
/// Start device.
pub const UHID_START: u32 = 2;
/// Stop device.
pub const UHID_STOP: u32 = 3;
/// Open device.
pub const UHID_OPEN: u32 = 4;
/// Close device.
pub const UHID_CLOSE: u32 = 5;
/// Output report.
pub const UHID_OUTPUT: u32 = 6;
/// Input report (from kernel, deprecated).
pub const UHID_INPUT: u32 = 9;
/// Get report (from kernel).
pub const UHID_GET_REPORT: u32 = 10;
/// Get report reply (to kernel).
pub const UHID_GET_REPORT_REPLY: u32 = 11;
/// Create device v2.
pub const UHID_CREATE2: u32 = 12;
/// Input report v2.
pub const UHID_INPUT2: u32 = 13;
/// Set report (from kernel).
pub const UHID_SET_REPORT: u32 = 14;
/// Set report reply (to kernel).
pub const UHID_SET_REPORT_REPLY: u32 = 15;

// ---------------------------------------------------------------------------
// UHID device flags
// ---------------------------------------------------------------------------

/// Numbered feature reports.
pub const UHID_DEV_NUMBERED_FEATURE_REPORTS: u32 = 1 << 0;
/// Numbered output reports.
pub const UHID_DEV_NUMBERED_OUTPUT_REPORTS: u32 = 1 << 1;
/// Numbered input reports.
pub const UHID_DEV_NUMBERED_INPUT_REPORTS: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// UHID sizes
// ---------------------------------------------------------------------------

/// Max HID report descriptor size.
pub const UHID_DATA_MAX: u32 = 4096;

// ---------------------------------------------------------------------------
// UHID bus types
// ---------------------------------------------------------------------------

/// USB bus.
pub const BUS_USB_UHID: u32 = 0x03;
/// Bluetooth bus.
pub const BUS_BLUETOOTH_UHID: u32 = 0x05;
/// I2C bus.
pub const BUS_I2C_UHID: u32 = 0x18;
/// Virtual bus.
pub const BUS_VIRTUAL_UHID: u32 = 0x06;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let events = [
            UHID_CREATE,
            UHID_DESTROY,
            UHID_START,
            UHID_STOP,
            UHID_OPEN,
            UHID_CLOSE,
            UHID_OUTPUT,
            UHID_INPUT,
            UHID_GET_REPORT,
            UHID_GET_REPORT_REPLY,
            UHID_CREATE2,
            UHID_INPUT2,
            UHID_SET_REPORT,
            UHID_SET_REPORT_REPLY,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_dev_flags_power_of_two() {
        let flags = [
            UHID_DEV_NUMBERED_FEATURE_REPORTS,
            UHID_DEV_NUMBERED_OUTPUT_REPORTS,
            UHID_DEV_NUMBERED_INPUT_REPORTS,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "{} not power of two", f);
        }
    }

    #[test]
    fn test_data_max() {
        assert_eq!(UHID_DATA_MAX, 4096);
    }

    #[test]
    fn test_bus_types_distinct() {
        let buses = [
            BUS_USB_UHID,
            BUS_BLUETOOTH_UHID,
            BUS_I2C_UHID,
            BUS_VIRTUAL_UHID,
        ];
        for i in 0..buses.len() {
            for j in (i + 1)..buses.len() {
                assert_ne!(buses[i], buses[j]);
            }
        }
    }

    #[test]
    fn test_create2_v2() {
        assert_eq!(UHID_CREATE2, 12);
        assert!(UHID_CREATE2 > UHID_CREATE);
    }
}
