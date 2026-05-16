//! `<linux/usb/ch9.h>` — USB device constants from Chapter 9 of the USB spec.
//!
//! Defines USB descriptor types, class codes, endpoint types, and
//! standard request codes. Used by USB host controller drivers,
//! gadget drivers, and tools like lsusb.

// ---------------------------------------------------------------------------
// USB descriptor types
// ---------------------------------------------------------------------------

/// Device descriptor.
pub const USB_DT_DEVICE: u8 = 0x01;
/// Configuration descriptor.
pub const USB_DT_CONFIG: u8 = 0x02;
/// String descriptor.
pub const USB_DT_STRING: u8 = 0x03;
/// Interface descriptor.
pub const USB_DT_INTERFACE: u8 = 0x04;
/// Endpoint descriptor.
pub const USB_DT_ENDPOINT: u8 = 0x05;
/// Device qualifier descriptor.
pub const USB_DT_DEVICE_QUALIFIER: u8 = 0x06;
/// Other-speed configuration.
pub const USB_DT_OTHER_SPEED_CONFIG: u8 = 0x07;
/// Interface power descriptor.
pub const USB_DT_INTERFACE_POWER: u8 = 0x08;
/// OTG descriptor.
pub const USB_DT_OTG: u8 = 0x09;
/// Interface association descriptor.
pub const USB_DT_INTERFACE_ASSOCIATION: u8 = 0x0B;
/// BOS descriptor.
pub const USB_DT_BOS: u8 = 0x0F;
/// SuperSpeed endpoint companion.
pub const USB_DT_SS_ENDPOINT_COMP: u8 = 0x30;

// ---------------------------------------------------------------------------
// USB class codes
// ---------------------------------------------------------------------------

/// Per-interface class.
pub const USB_CLASS_PER_INTERFACE: u8 = 0x00;
/// Audio class.
pub const USB_CLASS_AUDIO: u8 = 0x01;
/// Communications class (CDC).
pub const USB_CLASS_COMM: u8 = 0x02;
/// Human Interface Device (HID).
pub const USB_CLASS_HID: u8 = 0x03;
/// Physical.
pub const USB_CLASS_PHYSICAL: u8 = 0x05;
/// Still imaging.
pub const USB_CLASS_STILL_IMAGE: u8 = 0x06;
/// Printer.
pub const USB_CLASS_PRINTER: u8 = 0x07;
/// Mass storage.
pub const USB_CLASS_MASS_STORAGE: u8 = 0x08;
/// Hub.
pub const USB_CLASS_HUB: u8 = 0x09;
/// CDC data.
pub const USB_CLASS_CDC_DATA: u8 = 0x0A;
/// Smart card.
pub const USB_CLASS_CSCID: u8 = 0x0B;
/// Content security.
pub const USB_CLASS_CONTENT_SEC: u8 = 0x0D;
/// Video.
pub const USB_CLASS_VIDEO: u8 = 0x0E;
/// Personal Healthcare.
pub const USB_CLASS_PERSONAL_HEALTHCARE: u8 = 0x0F;
/// Audio/Video.
pub const USB_CLASS_AV: u8 = 0x10;
/// Billboard.
pub const USB_CLASS_BILLBOARD: u8 = 0x11;
/// Wireless controller.
pub const USB_CLASS_WIRELESS_CONTROLLER: u8 = 0xE0;
/// Miscellaneous.
pub const USB_CLASS_MISC: u8 = 0xEF;
/// Application-specific.
pub const USB_CLASS_APP_SPEC: u8 = 0xFE;
/// Vendor-specific.
pub const USB_CLASS_VENDOR_SPEC: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Endpoint types
// ---------------------------------------------------------------------------

/// Control endpoint.
pub const USB_ENDPOINT_XFER_CONTROL: u8 = 0;
/// Isochronous endpoint.
pub const USB_ENDPOINT_XFER_ISOC: u8 = 1;
/// Bulk endpoint.
pub const USB_ENDPOINT_XFER_BULK: u8 = 2;
/// Interrupt endpoint.
pub const USB_ENDPOINT_XFER_INT: u8 = 3;

/// Endpoint transfer type mask.
pub const USB_ENDPOINT_XFERTYPE_MASK: u8 = 0x03;
/// Endpoint direction mask.
pub const USB_ENDPOINT_DIR_MASK: u8 = 0x80;
/// Endpoint number mask.
pub const USB_ENDPOINT_NUMBER_MASK: u8 = 0x0F;

/// IN direction.
pub const USB_DIR_IN: u8 = 0x80;
/// OUT direction.
pub const USB_DIR_OUT: u8 = 0x00;

// ---------------------------------------------------------------------------
// Standard request codes
// ---------------------------------------------------------------------------

/// Get status.
pub const USB_REQ_GET_STATUS: u8 = 0x00;
/// Clear feature.
pub const USB_REQ_CLEAR_FEATURE: u8 = 0x01;
/// Set feature.
pub const USB_REQ_SET_FEATURE: u8 = 0x03;
/// Set address.
pub const USB_REQ_SET_ADDRESS: u8 = 0x05;
/// Get descriptor.
pub const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;
/// Set descriptor.
pub const USB_REQ_SET_DESCRIPTOR: u8 = 0x07;
/// Get configuration.
pub const USB_REQ_GET_CONFIGURATION: u8 = 0x08;
/// Set configuration.
pub const USB_REQ_SET_CONFIGURATION: u8 = 0x09;
/// Get interface.
pub const USB_REQ_GET_INTERFACE: u8 = 0x0A;
/// Set interface.
pub const USB_REQ_SET_INTERFACE: u8 = 0x0B;
/// Synch frame.
pub const USB_REQ_SYNCH_FRAME: u8 = 0x0C;

// ---------------------------------------------------------------------------
// USB speeds
// ---------------------------------------------------------------------------

/// Unknown speed.
pub const USB_SPEED_UNKNOWN: u32 = 0;
/// Low speed (1.5 Mbps).
pub const USB_SPEED_LOW: u32 = 1;
/// Full speed (12 Mbps).
pub const USB_SPEED_FULL: u32 = 2;
/// High speed (480 Mbps).
pub const USB_SPEED_HIGH: u32 = 3;
/// SuperSpeed (5 Gbps).
pub const USB_SPEED_SUPER: u32 = 5;
/// SuperSpeed+ (10+ Gbps).
pub const USB_SPEED_SUPER_PLUS: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor_types_distinct() {
        let types = [
            USB_DT_DEVICE, USB_DT_CONFIG, USB_DT_STRING,
            USB_DT_INTERFACE, USB_DT_ENDPOINT, USB_DT_DEVICE_QUALIFIER,
            USB_DT_OTHER_SPEED_CONFIG, USB_DT_INTERFACE_POWER,
            USB_DT_OTG, USB_DT_INTERFACE_ASSOCIATION,
            USB_DT_BOS, USB_DT_SS_ENDPOINT_COMP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_class_codes_distinct() {
        let classes = [
            USB_CLASS_PER_INTERFACE, USB_CLASS_AUDIO, USB_CLASS_COMM,
            USB_CLASS_HID, USB_CLASS_MASS_STORAGE, USB_CLASS_HUB,
            USB_CLASS_VIDEO, USB_CLASS_VENDOR_SPEC,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_endpoint_types() {
        assert_eq!(USB_ENDPOINT_XFER_CONTROL, 0);
        assert_eq!(USB_ENDPOINT_XFER_ISOC, 1);
        assert_eq!(USB_ENDPOINT_XFER_BULK, 2);
        assert_eq!(USB_ENDPOINT_XFER_INT, 3);
    }

    #[test]
    fn test_direction() {
        assert_eq!(USB_DIR_IN, 0x80);
        assert_eq!(USB_DIR_OUT, 0x00);
    }

    #[test]
    fn test_speeds_distinct() {
        let speeds = [
            USB_SPEED_UNKNOWN, USB_SPEED_LOW, USB_SPEED_FULL,
            USB_SPEED_HIGH, USB_SPEED_SUPER, USB_SPEED_SUPER_PLUS,
        ];
        for i in 0..speeds.len() {
            for j in (i + 1)..speeds.len() {
                assert_ne!(speeds[i], speeds[j]);
            }
        }
    }

    #[test]
    fn test_request_codes_distinct() {
        let reqs = [
            USB_REQ_GET_STATUS, USB_REQ_CLEAR_FEATURE,
            USB_REQ_SET_FEATURE, USB_REQ_SET_ADDRESS,
            USB_REQ_GET_DESCRIPTOR, USB_REQ_SET_DESCRIPTOR,
            USB_REQ_GET_CONFIGURATION, USB_REQ_SET_CONFIGURATION,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }
}
