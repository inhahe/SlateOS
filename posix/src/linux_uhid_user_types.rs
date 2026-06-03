//! `<linux/uhid.h>` — `/dev/uhid` virtual HID transport.
//!
//! uhid lets userspace pretend to be a USB-HID device by writing
//! `struct uhid_event` records to `/dev/uhid`. BlueZ uses it to bridge
//! Bluetooth-HID into the input subsystem; OBD-II / joystick emulators
//! use it for testing.

// ---------------------------------------------------------------------------
// Event-type IDs (struct uhid_event.type)
// ---------------------------------------------------------------------------

/// User→kernel: bring up a new HID device.
pub const UHID_CREATE: u32 = 0;
/// User→kernel: tear down the HID device.
pub const UHID_DESTROY: u32 = 1;
/// Kernel→user: hid_start called (device active).
pub const UHID_START: u32 = 2;
/// Kernel→user: hid_stop called.
pub const UHID_STOP: u32 = 3;
/// Kernel→user: hid_open (someone read())d the input dev).
pub const UHID_OPEN: u32 = 4;
/// Kernel→user: hid_close.
pub const UHID_CLOSE: u32 = 5;
/// Kernel→user: hid_output_raw_report.
pub const UHID_OUTPUT: u32 = 6;
/// Kernel→user: hid_output_report (legacy).
pub const UHID_OUTPUT_EV: u32 = 7;
/// User→kernel: deliver an input report.
pub const UHID_INPUT: u32 = 8;
/// Kernel→user: GetReport request.
pub const UHID_GET_REPORT: u32 = 9;
/// User→kernel: reply to GetReport.
pub const UHID_GET_REPORT_REPLY: u32 = 10;
/// User→kernel: create_v2 with extended fields.
pub const UHID_CREATE2: u32 = 11;
/// User→kernel: input_v2 with the new layout.
pub const UHID_INPUT2: u32 = 12;
/// Kernel→user: SetReport request.
pub const UHID_SET_REPORT: u32 = 13;
/// User→kernel: reply to SetReport.
pub const UHID_SET_REPORT_REPLY: u32 = 14;

// ---------------------------------------------------------------------------
// Report types (uhid_get_report_req.rtype / uhid_set_report_req.rtype)
// ---------------------------------------------------------------------------

/// Input report (host reads from device).
pub const UHID_INPUT_REPORT: u8 = 0;
/// Output report (host writes to device, e.g. LEDs).
pub const UHID_OUTPUT_REPORT: u8 = 1;
/// Feature report (bidirectional config).
pub const UHID_FEATURE_REPORT: u8 = 2;

// ---------------------------------------------------------------------------
// Size limits
// ---------------------------------------------------------------------------

/// Maximum HID report-descriptor size.
pub const UHID_DATA_MAX: usize = 4096;
/// Maximum name string in struct uhid_create2_req.
pub const UHID_NAME_MAX: usize = 128;
/// Maximum physical-path string.
pub const UHID_PHYS_MAX: usize = 64;
/// Maximum uniq (serial) string.
pub const UHID_UNIQ_MAX: usize = 64;

// ---------------------------------------------------------------------------
// Bus types (uhid_create2_req.bus)
// ---------------------------------------------------------------------------

/// `BUS_USB` — pretend to be a USB device.
pub const UHID_BUS_USB: u16 = 0x03;
/// `BUS_BLUETOOTH`.
pub const UHID_BUS_BLUETOOTH: u16 = 0x05;
/// `BUS_VIRTUAL` — software-only device.
pub const UHID_BUS_VIRTUAL: u16 = 0x06;
/// `BUS_I2C` — I2C-HID.
pub const UHID_BUS_I2C: u16 = 0x18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_dense() {
        let e = [
            UHID_CREATE,
            UHID_DESTROY,
            UHID_START,
            UHID_STOP,
            UHID_OPEN,
            UHID_CLOSE,
            UHID_OUTPUT,
            UHID_OUTPUT_EV,
            UHID_INPUT,
            UHID_GET_REPORT,
            UHID_GET_REPORT_REPLY,
            UHID_CREATE2,
            UHID_INPUT2,
            UHID_SET_REPORT,
            UHID_SET_REPORT_REPLY,
        ];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_report_types_dense() {
        assert_eq!(UHID_INPUT_REPORT, 0);
        assert_eq!(UHID_OUTPUT_REPORT, 1);
        assert_eq!(UHID_FEATURE_REPORT, 2);
    }

    #[test]
    fn test_size_limits() {
        assert!(UHID_DATA_MAX.is_power_of_two());
        assert_eq!(UHID_DATA_MAX, 4096);
        // NAME_MAX is the dominant string size — keep it at 128.
        assert_eq!(UHID_NAME_MAX, 128);
        assert!(UHID_PHYS_MAX <= UHID_NAME_MAX);
        assert!(UHID_UNIQ_MAX <= UHID_NAME_MAX);
    }

    #[test]
    fn test_bus_types_distinct() {
        let b = [UHID_BUS_USB, UHID_BUS_BLUETOOTH, UHID_BUS_VIRTUAL, UHID_BUS_I2C];
        for i in 0..b.len() {
            for j in (i + 1)..b.len() {
                assert_ne!(b[i], b[j]);
            }
        }
        // USB is the most common bus and is hard-coded to BUS_USB=3.
        assert_eq!(UHID_BUS_USB, 3);
    }
}
