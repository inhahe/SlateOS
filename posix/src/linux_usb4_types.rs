//! `<linux/usb/ch9.h>` — Additional USB constants (part 4).
//!
//! Supplementary USB constants covering request types,
//! descriptor types, and endpoint attributes.

// ---------------------------------------------------------------------------
// USB request types (bmRequestType direction)
// ---------------------------------------------------------------------------

/// Host to device.
pub const USB_DIR_OUT: u8 = 0x00;
/// Device to host.
pub const USB_DIR_IN: u8 = 0x80;

// ---------------------------------------------------------------------------
// USB request types (bmRequestType type)
// ---------------------------------------------------------------------------

/// Standard request.
pub const USB_TYPE_STANDARD: u8 = 0x00;
/// Class request.
pub const USB_TYPE_CLASS: u8 = 0x20;
/// Vendor request.
pub const USB_TYPE_VENDOR: u8 = 0x40;
/// Reserved.
pub const USB_TYPE_RESERVED: u8 = 0x60;

// ---------------------------------------------------------------------------
// USB request types (bmRequestType recipient)
// ---------------------------------------------------------------------------

/// Device recipient.
pub const USB_RECIP_DEVICE: u8 = 0x00;
/// Interface recipient.
pub const USB_RECIP_INTERFACE: u8 = 0x01;
/// Endpoint recipient.
pub const USB_RECIP_ENDPOINT: u8 = 0x02;
/// Other recipient.
pub const USB_RECIP_OTHER: u8 = 0x03;

// ---------------------------------------------------------------------------
// USB standard request codes
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
/// BOS descriptor.
pub const USB_DT_BOS: u8 = 0x0F;
/// Device capability.
pub const USB_DT_DEVICE_CAPABILITY: u8 = 0x10;
/// SS endpoint companion.
pub const USB_DT_SS_ENDPOINT_COMP: u8 = 0x30;
/// SSP isoc endpoint companion.
pub const USB_DT_SSP_ISOC_ENDPOINT_COMP: u8 = 0x31;

// ---------------------------------------------------------------------------
// USB endpoint transfer types
// ---------------------------------------------------------------------------

/// Control endpoint.
pub const USB_ENDPOINT_XFER_CONTROL: u8 = 0;
/// Isochronous endpoint.
pub const USB_ENDPOINT_XFER_ISOC: u8 = 1;
/// Bulk endpoint.
pub const USB_ENDPOINT_XFER_BULK: u8 = 2;
/// Interrupt endpoint.
pub const USB_ENDPOINT_XFER_INT: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direction() {
        assert_eq!(USB_DIR_OUT, 0);
        assert_eq!(USB_DIR_IN, 0x80);
    }

    #[test]
    fn test_request_types_distinct() {
        let types = [USB_TYPE_STANDARD, USB_TYPE_CLASS, USB_TYPE_VENDOR, USB_TYPE_RESERVED];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_recipients_distinct() {
        let recips = [
            USB_RECIP_DEVICE, USB_RECIP_INTERFACE,
            USB_RECIP_ENDPOINT, USB_RECIP_OTHER,
        ];
        for i in 0..recips.len() {
            for j in (i + 1)..recips.len() {
                assert_ne!(recips[i], recips[j]);
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
            USB_REQ_SYNCH_FRAME,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_descriptors_distinct() {
        let descs = [
            USB_DT_DEVICE, USB_DT_CONFIG, USB_DT_STRING,
            USB_DT_INTERFACE, USB_DT_ENDPOINT, USB_DT_BOS,
            USB_DT_DEVICE_CAPABILITY, USB_DT_SS_ENDPOINT_COMP,
            USB_DT_SSP_ISOC_ENDPOINT_COMP,
        ];
        for i in 0..descs.len() {
            for j in (i + 1)..descs.len() {
                assert_ne!(descs[i], descs[j]);
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
}
