//! `<linux/usb/ch9.h>` — USB specification chapter 9 constants.
//!
//! Chapter 9 of the USB specification defines the standard device
//! framework: descriptors, requests, and device states. These
//! constants are used by USB host controllers, device drivers,
//! and gadget drivers alike.

// ---------------------------------------------------------------------------
// Descriptor types
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
/// Other speed configuration.
pub const USB_DT_OTHER_SPEED_CONFIG: u8 = 0x07;
/// Interface power descriptor.
pub const USB_DT_INTERFACE_POWER: u8 = 0x08;
/// BOS (Binary Object Store) descriptor.
pub const USB_DT_BOS: u8 = 0x0F;
/// SuperSpeed endpoint companion.
pub const USB_DT_SS_ENDPOINT_COMP: u8 = 0x30;

// ---------------------------------------------------------------------------
// Standard requests (bRequest)
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

// ---------------------------------------------------------------------------
// Device speed
// ---------------------------------------------------------------------------

/// Low speed (1.5 Mbit/s).
pub const USB_SPEED_LOW: u8 = 1;
/// Full speed (12 Mbit/s).
pub const USB_SPEED_FULL: u8 = 2;
/// High speed (480 Mbit/s).
pub const USB_SPEED_HIGH: u8 = 3;
/// SuperSpeed (5 Gbit/s).
pub const USB_SPEED_SUPER: u8 = 5;
/// SuperSpeed+ (10/20 Gbit/s).
pub const USB_SPEED_SUPER_PLUS: u8 = 6;

// ---------------------------------------------------------------------------
// Endpoint transfer types
// ---------------------------------------------------------------------------

/// Control transfer.
pub const USB_ENDPOINT_XFER_CONTROL: u8 = 0;
/// Isochronous transfer.
pub const USB_ENDPOINT_XFER_ISOC: u8 = 1;
/// Bulk transfer.
pub const USB_ENDPOINT_XFER_BULK: u8 = 2;
/// Interrupt transfer.
pub const USB_ENDPOINT_XFER_INT: u8 = 3;

/// Transfer type mask in bmAttributes.
pub const USB_ENDPOINT_XFERTYPE_MASK: u8 = 0x03;
/// Direction mask (bit 7 of bEndpointAddress).
pub const USB_ENDPOINT_DIR_MASK: u8 = 0x80;

// ---------------------------------------------------------------------------
// Device classes
// ---------------------------------------------------------------------------

/// Per-interface class.
pub const USB_CLASS_PER_INTERFACE: u8 = 0x00;
/// Audio class.
pub const USB_CLASS_AUDIO: u8 = 0x01;
/// Communications (CDC).
pub const USB_CLASS_COMM: u8 = 0x02;
/// HID (Human Interface Device).
pub const USB_CLASS_HID: u8 = 0x03;
/// Mass storage.
pub const USB_CLASS_MASS_STORAGE: u8 = 0x08;
/// Hub.
pub const USB_CLASS_HUB: u8 = 0x09;
/// Video.
pub const USB_CLASS_VIDEO: u8 = 0x0E;
/// Vendor specific.
pub const USB_CLASS_VENDOR_SPEC: u8 = 0xFF;

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
            USB_DT_BOS, USB_DT_SS_ENDPOINT_COMP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_requests_distinct() {
        let reqs = [
            USB_REQ_GET_STATUS, USB_REQ_CLEAR_FEATURE,
            USB_REQ_SET_FEATURE, USB_REQ_SET_ADDRESS,
            USB_REQ_GET_DESCRIPTOR, USB_REQ_SET_DESCRIPTOR,
            USB_REQ_GET_CONFIGURATION, USB_REQ_SET_CONFIGURATION,
            USB_REQ_GET_INTERFACE, USB_REQ_SET_INTERFACE,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_speeds_distinct() {
        let speeds = [
            USB_SPEED_LOW, USB_SPEED_FULL, USB_SPEED_HIGH,
            USB_SPEED_SUPER, USB_SPEED_SUPER_PLUS,
        ];
        for i in 0..speeds.len() {
            for j in (i + 1)..speeds.len() {
                assert_ne!(speeds[i], speeds[j]);
            }
        }
    }

    #[test]
    fn test_xfer_types_distinct() {
        let types = [
            USB_ENDPOINT_XFER_CONTROL, USB_ENDPOINT_XFER_ISOC,
            USB_ENDPOINT_XFER_BULK, USB_ENDPOINT_XFER_INT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_classes_distinct() {
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
}
