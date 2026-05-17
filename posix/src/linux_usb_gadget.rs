//! `<linux/usb/gadget.h>` — USB gadget (device-side) constants.
//!
//! The USB gadget subsystem allows Linux to act as a USB device
//! (peripheral). Gadget drivers implement device-side USB functions
//! such as mass storage, serial ports, ethernet adapters, and HID
//! devices. These constants define speeds, endpoint capabilities,
//! and composite gadget configuration.

// ---------------------------------------------------------------------------
// Gadget speed
// ---------------------------------------------------------------------------

/// Unknown speed (not yet enumerated).
pub const USB_SPEED_UNKNOWN: u8 = 0;
/// Low speed (1.5 Mbit/s).
pub const USB_GADGET_SPEED_LOW: u8 = 1;
/// Full speed (12 Mbit/s).
pub const USB_GADGET_SPEED_FULL: u8 = 2;
/// High speed (480 Mbit/s).
pub const USB_GADGET_SPEED_HIGH: u8 = 3;
/// Wireless USB (deprecated).
pub const USB_GADGET_SPEED_WIRELESS: u8 = 4;
/// SuperSpeed (5 Gbit/s).
pub const USB_GADGET_SPEED_SUPER: u8 = 5;
/// SuperSpeed+ (10/20 Gbit/s).
pub const USB_GADGET_SPEED_SUPER_PLUS: u8 = 6;

// ---------------------------------------------------------------------------
// Endpoint capabilities
// ---------------------------------------------------------------------------

/// Supports control transfers.
pub const USB_EP_CAPS_TYPE_CONTROL: u32 = 1 << 0;
/// Supports isochronous transfers.
pub const USB_EP_CAPS_TYPE_ISO: u32 = 1 << 1;
/// Supports bulk transfers.
pub const USB_EP_CAPS_TYPE_BULK: u32 = 1 << 2;
/// Supports interrupt transfers.
pub const USB_EP_CAPS_TYPE_INT: u32 = 1 << 3;
/// Supports IN direction.
pub const USB_EP_CAPS_DIR_IN: u32 = 1 << 4;
/// Supports OUT direction.
pub const USB_EP_CAPS_DIR_OUT: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Gadget states
// ---------------------------------------------------------------------------

/// Not attached to host.
pub const USB_STATE_NOTATTACHED: u8 = 0;
/// Attached, waiting for bus reset.
pub const USB_STATE_ATTACHED: u8 = 1;
/// Bus reset received, default address.
pub const USB_STATE_POWERED: u8 = 2;
/// After SET_ADDRESS but before SET_CONFIGURATION.
pub const USB_STATE_DEFAULT: u8 = 3;
/// Address assigned.
pub const USB_STATE_ADDRESS: u8 = 4;
/// Configuration set, fully operational.
pub const USB_STATE_CONFIGURED: u8 = 5;
/// Suspended (low power).
pub const USB_STATE_SUSPENDED: u8 = 6;

// ---------------------------------------------------------------------------
// Standard function classes (for composite gadgets)
// ---------------------------------------------------------------------------

/// CDC ACM (serial port).
pub const USB_FUNC_ACM: u8 = 0x02;
/// CDC ECM (ethernet).
pub const USB_FUNC_ECM: u8 = 0x06;
/// CDC NCM (network).
pub const USB_FUNC_NCM: u8 = 0x0D;
/// Mass storage.
pub const USB_FUNC_MASS_STORAGE: u8 = 0x08;
/// HID function.
pub const USB_FUNC_HID: u8 = 0x03;
/// RNDIS (Windows-compatible ethernet).
pub const USB_FUNC_RNDIS: u8 = 0xEF;
/// UAC (USB Audio Class).
pub const USB_FUNC_UAC: u8 = 0x01;
/// UVC (USB Video Class).
pub const USB_FUNC_UVC: u8 = 0x0E;
/// Printer.
pub const USB_FUNC_PRINTER: u8 = 0x07;

// ---------------------------------------------------------------------------
// Max packet sizes (standard)
// ---------------------------------------------------------------------------

/// Max packet for control endpoint (all speeds).
pub const USB_MAX_CTRL_PACKET: u16 = 64;
/// Max packet for full-speed bulk.
pub const USB_MAX_FS_BULK_PACKET: u16 = 64;
/// Max packet for high-speed bulk.
pub const USB_MAX_HS_BULK_PACKET: u16 = 512;
/// Max packet for SuperSpeed bulk.
pub const USB_MAX_SS_BULK_PACKET: u16 = 1024;
/// Max packet for full-speed isochronous.
pub const USB_MAX_FS_ISO_PACKET: u16 = 1023;
/// Max packet for high-speed isochronous.
pub const USB_MAX_HS_ISO_PACKET: u16 = 1024;
/// Max packet for full-speed interrupt.
pub const USB_MAX_FS_INT_PACKET: u16 = 64;
/// Max packet for high-speed interrupt.
pub const USB_MAX_HS_INT_PACKET: u16 = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gadget_speeds_distinct() {
        let speeds = [
            USB_SPEED_UNKNOWN, USB_GADGET_SPEED_LOW, USB_GADGET_SPEED_FULL,
            USB_GADGET_SPEED_HIGH, USB_GADGET_SPEED_WIRELESS,
            USB_GADGET_SPEED_SUPER, USB_GADGET_SPEED_SUPER_PLUS,
        ];
        for i in 0..speeds.len() {
            for j in (i + 1)..speeds.len() {
                assert_ne!(speeds[i], speeds[j]);
            }
        }
    }

    #[test]
    fn test_ep_caps_no_overlap() {
        let caps = [
            USB_EP_CAPS_TYPE_CONTROL, USB_EP_CAPS_TYPE_ISO,
            USB_EP_CAPS_TYPE_BULK, USB_EP_CAPS_TYPE_INT,
            USB_EP_CAPS_DIR_IN, USB_EP_CAPS_DIR_OUT,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_ep_caps_power_of_two() {
        let caps = [
            USB_EP_CAPS_TYPE_CONTROL, USB_EP_CAPS_TYPE_ISO,
            USB_EP_CAPS_TYPE_BULK, USB_EP_CAPS_TYPE_INT,
            USB_EP_CAPS_DIR_IN, USB_EP_CAPS_DIR_OUT,
        ];
        for c in &caps {
            assert!(c.is_power_of_two());
        }
    }

    #[test]
    fn test_gadget_states_distinct() {
        let states = [
            USB_STATE_NOTATTACHED, USB_STATE_ATTACHED, USB_STATE_POWERED,
            USB_STATE_DEFAULT, USB_STATE_ADDRESS, USB_STATE_CONFIGURED,
            USB_STATE_SUSPENDED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_func_classes_distinct() {
        let funcs = [
            USB_FUNC_ACM, USB_FUNC_ECM, USB_FUNC_NCM,
            USB_FUNC_MASS_STORAGE, USB_FUNC_HID, USB_FUNC_RNDIS,
            USB_FUNC_UAC, USB_FUNC_UVC, USB_FUNC_PRINTER,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_max_packet_sizes() {
        // SuperSpeed > High Speed > Full Speed for bulk
        assert!(USB_MAX_SS_BULK_PACKET > USB_MAX_HS_BULK_PACKET);
        assert!(USB_MAX_HS_BULK_PACKET > USB_MAX_FS_BULK_PACKET);
    }
}
